//! Journaled metadata and a bounded process lock for login, refresh and logout.
use super::{
    AGENTCENTER_ORIGIN, Credential, CredentialProvider, LoginGateway, Secret, SecretStore, Stamp,
    required, required_for, store_unavailable,
};
use crate::{
    domain::{
        Result,
        auth::{AuthMethod, AuthStatus, Session, SessionState},
    },
    env::Clock,
    paths,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct Sessions {
    root: PathBuf,
    namespace: String,
    lock_timeout: Duration,
    origin: String,
}
impl Sessions {
    pub fn new(config_root: &Path) -> Result<Self> {
        Self::for_origin(config_root, AGENTCENTER_ORIGIN)
    }
    pub fn for_origin(config_root: &Path, origin: &str) -> Result<Self> {
        let origin = crate::config::auth_origin(origin)?;
        let root = paths::absolute(config_root).map_err(|_| store_unavailable())?;
        let config_hash = crate::domain::digest(root.as_os_str().as_encoded_bytes());
        let origin_hash = crate::domain::digest(origin.as_bytes());
        let official = origin == AGENTCENTER_ORIGIN;
        Ok(Self {
            namespace: if official {
                format!("org.skill-bom.w3.v1.{config_hash}")
            } else {
                format!("org.skill-bom.auth.v1.{config_hash}.{origin_hash}")
            },
            root: if official {
                root.join("auth-v1")
            } else {
                root.join("auth-v1/origins").join(origin_hash)
            },
            lock_timeout: Duration::from_secs(100),
            origin,
        })
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn method(&self) -> AuthMethod {
        if self.origin == AGENTCENTER_ORIGIN {
            AuthMethod::W3
        } else {
            AuthMethod::Token
        }
    }
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout.min(Duration::from_secs(120));
        self
    }
    pub fn service<'a>(
        &'a self,
        secrets: &'a dyn SecretStore,
        gateway: &'a dyn LoginGateway,
        clock: &'a dyn Clock,
    ) -> Service<'a> {
        Service {
            sessions: self,
            secrets,
            gateway: Some(gateway),
            clock,
        }
    }
    /// Token storage and acquisition never construct a login gateway or HTTP client.
    pub fn tokens<'a>(&'a self, secrets: &'a dyn SecretStore, clock: &'a dyn Clock) -> Service<'a> {
        Service {
            sessions: self,
            secrets,
            gateway: None,
            clock,
        }
    }
    fn read(&self) -> Result<SessionState> {
        let path = self.root.join("session.json");
        paths::no_symlink(&path).map_err(|_| store_unavailable())?;
        if !path.try_exists().map_err(|_| store_unavailable())? {
            return Ok(SessionState {
                origin: (self.method() == AuthMethod::Token).then(|| self.origin.clone()),
                ..SessionState::default()
            });
        }
        let bytes = paths::read(&path, 32768).map_err(|_| store_unavailable())?;
        let state: SessionState =
            serde_json::from_slice(&bytes).map_err(|_| store_unavailable())?;
        let valid_ref = |s: &str| uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == s);
        let expected_origin = (self.method() == AuthMethod::Token).then_some(self.origin.as_str());
        if state.origin.as_deref() != expected_origin
            || state.cleanup.len() > 4
            || state.cleanup.iter().any(|r| !valid_ref(r))
            || state.session.as_ref().is_some_and(|s| {
                !valid_ref(&s.credential_ref)
                    || !valid_ref(&s.login_id)
                    || state.cleanup.contains(&s.credential_ref)
                    || s.method != self.method()
                    || match s.method {
                        AuthMethod::W3 => {
                            s.expires_at.is_none_or(|expires| expires <= s.issued_at)
                                || s.expiry_source.is_none()
                        }
                        AuthMethod::Token => s.expires_at.is_some() || s.expiry_source.is_some(),
                    }
                    || !valid_username(&s.username)
            })
        {
            return Err(store_unavailable());
        }
        Ok(state)
    }
    fn write(&self, state: &SessionState) -> Result<()> {
        let bytes = serde_json::to_vec(state).map_err(|_| store_unavailable())?;
        paths::atomic_write(&self.root.join("session.json"), &bytes)
            .map_err(|_| store_unavailable())
    }
    fn lock(&self) -> Result<std::fs::File> {
        let path = self.root.join("session.lock");
        paths::no_symlink(&path).map_err(|_| store_unavailable())?;
        std::fs::create_dir_all(&self.root).map_err(|_| store_unavailable())?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| store_unavailable())?;
        let start = Instant::now();
        loop {
            crate::process::check_interrupt()?;
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(file),
                Err(e) if e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                    && start.elapsed() < self.lock_timeout => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return Err(store_unavailable().hint("Authentication is busy or its lock is unavailable. Retry skill-bom auth login or auth logout.")),
            }
        }
    }
    /// Local metadata only: no lock creation, keyring access, HTTP or refresh.
    pub fn status(&self, clock: &dyn Clock) -> Result<AuthStatus> {
        let state = self.read()?;
        Ok(AuthStatus {
            origin: self.origin.clone(),
            method: self.method(),
            logged_in: state.session.is_some(),
            username: state.session.as_ref().map(|s| s.username.clone()),
            expires_at: state.session.as_ref().and_then(|s| s.expires_at),
            expiry_source: state.session.as_ref().and_then(|s| s.expiry_source),
            expired: self.method().is_w3().then(|| {
                state
                    .session
                    .as_ref()
                    .is_some_and(|s| s.expires_at.is_some_and(|expires| expires <= clock.now()))
            }),
            cleanup_pending: !state.cleanup.is_empty(),
        })
    }
    pub fn all_statuses(config_root: &Path, clock: &dyn Clock) -> Result<Vec<AuthStatus>> {
        let official = Self::new(config_root)?;
        let mut statuses = vec![];
        let status = official.status(clock)?;
        if status.logged_in || status.cleanup_pending {
            statuses.push(status);
        }
        let directory = official.root.join("origins");
        paths::no_symlink(&directory).map_err(|_| store_unavailable())?;
        if !directory.try_exists().map_err(|_| store_unavailable())? {
            return Ok(statuses);
        }
        for (index, entry) in std::fs::read_dir(&directory)
            .map_err(|_| store_unavailable())?
            .enumerate()
        {
            if index >= 1024 {
                return Err(crate::domain::Error::new(
                    "RESOURCE_LIMIT",
                    "Authentication catalog exceeds 1024 origins",
                    2,
                ));
            }
            let entry = entry.map_err(|_| store_unavailable())?;
            if !entry.file_type().map_err(|_| store_unavailable())?.is_dir() {
                return Err(store_unavailable());
            }
            let path = entry.path().join("session.json");
            paths::no_symlink(&path).map_err(|_| store_unavailable())?;
            if !path.try_exists().map_err(|_| store_unavailable())? {
                continue;
            }
            let bytes = paths::read(&path, 32768).map_err(|_| store_unavailable())?;
            let state: SessionState =
                serde_json::from_slice(&bytes).map_err(|_| store_unavailable())?;
            let origin = state.origin.ok_or_else(store_unavailable)?;
            let sessions =
                Self::for_origin(config_root, &origin).map_err(|_| store_unavailable())?;
            if sessions.root != entry.path() {
                return Err(store_unavailable());
            }
            let status = sessions.status(clock)?;
            if status.logged_in || status.cleanup_pending {
                statuses.push(status);
            }
        }
        statuses.sort_by(|a, b| a.origin.cmp(&b.origin));
        Ok(statuses)
    }
    pub fn logout(&self, secrets: &dyn SecretStore) -> Result<()> {
        let _guard = self.lock()?;
        let mut state = self.read()?;
        if let Some(session) = state.session.take() {
            state.cleanup.push(session.credential_ref);
            advance(&mut state)?;
            // Publish the tombstone before deleting: a waiting refresh cannot resurrect it.
            self.write(&state)?;
        }
        self.cleanup(&mut state, secrets)
    }
    fn cleanup(&self, state: &mut SessionState, secrets: &dyn SecretStore) -> Result<()> {
        if state.cleanup.is_empty() {
            return Ok(());
        }
        for reference in &state.cleanup {
            for kind in if self.method().is_w3() {
                &["password", "token"][..]
            } else {
                &["token"][..]
            } {
                secrets
                    .delete(&self.namespace, &format!("{reference}.{kind}"))
                    .map_err(|_| store_unavailable())?;
            }
        }
        state.cleanup.clear();
        self.write(state)
    }
}

pub struct Service<'a> {
    sessions: &'a Sessions,
    secrets: &'a dyn SecretStore,
    gateway: Option<&'a dyn LoginGateway>,
    clock: &'a dyn Clock,
}
impl Service<'_> {
    pub fn login(&self, username: &str, password: &Secret) -> Result<AuthStatus> {
        if !self.sessions.method().is_w3()
            || !valid_username(username)
            || password.expose().is_empty()
            || password.expose().len() > 4096
        {
            return Err(required(
                "A nonempty W3 username and password within the input limits are required",
            ));
        }
        let _guard = self.sessions.lock()?;
        let mut state = self.sessions.read()?;
        self.sessions.cleanup(&mut state, self.secrets)?;
        let now = self.clock.now();
        let result = self
            .gateway
            .ok_or_else(|| required("W3 login gateway is required"))?
            .login(username, password, now)?;
        let session = Session {
            method: AuthMethod::W3,
            username: username.into(),
            issued_at: now,
            expires_at: Some(result.expires_at),
            expiry_source: Some(result.expiry_source),
            credential_ref: uuid::Uuid::new_v4().to_string(),
            login_id: uuid::Uuid::new_v4().to_string(),
        };
        self.publish(
            &mut state,
            session,
            &[("password", password), ("token", &result.token)],
        )?;
        self.sessions.status(self.clock)
    }
    pub fn login_token(&self, username: &str, token: &Secret) -> Result<AuthStatus> {
        if self.sessions.method() != AuthMethod::Token
            || !valid_username(username)
            || !super::valid_token(token.expose())
        {
            return Err(crate::domain::Error::new(
                "AUTH_INPUT",
                "Token login requires a non-W3 origin, an account name and a valid bounded token",
                2,
            )
            .phase("authentication"));
        }
        let _guard = self.sessions.lock()?;
        let mut state = self.sessions.read()?;
        self.sessions.cleanup(&mut state, self.secrets)?;
        let session = Session {
            method: AuthMethod::Token,
            username: username.into(),
            issued_at: self.clock.now(),
            expires_at: None,
            expiry_source: None,
            credential_ref: uuid::Uuid::new_v4().to_string(),
            login_id: uuid::Uuid::new_v4().to_string(),
        };
        self.publish(&mut state, session, &[("token", token)])?;
        self.sessions.status(self.clock)
    }
    fn publish(
        &self,
        state: &mut SessionState,
        session: Session,
        entries: &[(&str, &Secret)],
    ) -> Result<()> {
        // Journal the new reference first. Interrupted writes stay discoverable by logout.
        state.cleanup.push(session.credential_ref.clone());
        self.sessions.write(state)?;
        for (kind, secret) in entries {
            self.secrets
                .set(
                    &self.sessions.namespace,
                    &format!("{}.{kind}", session.credential_ref),
                    secret,
                )
                .map_err(|_| store_unavailable())?;
        }
        state.cleanup.clear();
        if let Some(old) = state.session.replace(session) {
            state.cleanup.push(old.credential_ref);
        }
        advance(state)?;
        self.sessions.write(state)?;
        self.sessions.cleanup(state, self.secrets)
    }
    fn get(&self, session: &Session, kind: &str) -> Result<Secret> {
        self.secrets
            .get(
                &self.sessions.namespace,
                &format!("{}.{kind}", session.credential_ref),
            )
            .map_err(|_| store_unavailable())?
            .filter(|secret| !secret.expose().is_empty())
            .ok_or_else(|| required_for(&self.sessions.origin, "The local credential is missing"))
    }
    fn credential(&self, state: &SessionState) -> Result<Credential> {
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| required_for(&self.sessions.origin, "No local login"))?;
        Ok(Credential {
            token: self.get(session, "token")?,
            stamp: Some(Stamp {
                origin: self.sessions.origin.clone(),
                method: session.method,
                version: state.version,
                login_id: session.login_id.clone(),
            }),
        })
    }
    fn renew(&self, state: &mut SessionState) -> Result<Credential> {
        self.sessions.cleanup(state, self.secrets)?;
        let mut session = state
            .session
            .clone()
            .ok_or_else(|| required("No local W3 login"))?;
        let password = self.get(&session, "password")?;
        let now = self.clock.now();
        let result = self
            .gateway
            .ok_or_else(|| required("W3 login gateway is required"))?
            .login(&session.username, &password, now)
            .map_err(|error| {
                // A gateway failure must never echo the password, token or response body.
                crate::domain::Error::new(&error.code, "W3 reauthentication failed", 2)
                    .phase("authentication")
                    .hint(super::LOGIN_HINT)
            })?;
        session.issued_at = now;
        session.expires_at = Some(result.expires_at);
        session.expiry_source = Some(result.expiry_source);
        session.credential_ref = uuid::Uuid::new_v4().to_string();
        self.publish(
            state,
            session,
            &[("password", &password), ("token", &result.token)],
        )?;
        self.credential(state)
    }
    fn check_scope(&self, origin: &str) -> Result<()> {
        if crate::config::auth_origin(origin).ok().as_deref() == Some(self.sessions.origin.as_str())
        {
            Ok(())
        } else {
            Err(crate::domain::Error::new(
                "MISSING_AUTH_TOKEN",
                "Credential provider belongs to a different origin",
                2,
            )
            .phase("authentication"))
        }
    }
}
impl CredentialProvider for Service<'_> {
    fn acquire(&self, origin: &str, explicit: Option<String>) -> Result<Credential> {
        self.acquire_optional(origin, explicit)?
            .ok_or_else(|| required_for(origin, "No local login"))
    }
    fn acquire_optional(
        &self,
        origin: &str,
        explicit: Option<String>,
    ) -> Result<Option<Credential>> {
        if let Some(token) = explicit.filter(|t| !t.is_empty()) {
            return Ok(Some(Credential::explicit(token)));
        }
        self.check_scope(origin)?;
        // Public registries need no keyring or directory creation when not logged in.
        if self.sessions.read()?.session.is_none() {
            return Ok(None);
        }
        let _guard = self.sessions.lock()?;
        let mut state = self.sessions.read()?;
        let Some(session) = state.session.as_ref() else {
            return Ok(None);
        };
        if session.refresh_due(self.clock.now()) {
            self.renew(&mut state).map(Some)
        } else {
            self.credential(&state).map(Some)
        }
    }
    fn refresh(&self, origin: &str, rejected: &Credential) -> Result<Credential> {
        self.check_scope(origin)?;
        if !rejected.can_refresh() {
            return Err(required_for(
                origin,
                "Token credentials cannot be refreshed",
            ));
        }
        let stamp = rejected
            .stamp
            .as_ref()
            .ok_or_else(|| required("Explicit tokens are never refreshed"))?;
        if stamp.origin != self.sessions.origin {
            return Err(required_for(
                origin,
                "Rejected credential belongs to a different origin",
            ));
        }
        let _guard = self.sessions.lock()?;
        let mut state = self.sessions.read()?;
        let current = state
            .session
            .as_ref()
            .ok_or_else(|| required("W3 session was logged out"))?;
        if stamp.login_id != current.login_id {
            return Err(required(
                "W3 account changed while the request was in flight",
            ));
        }
        if state.version != stamp.version && !current.refresh_due(self.clock.now()) {
            return self.credential(&state);
        }
        self.renew(&mut state)
    }
}
fn advance(state: &mut SessionState) -> Result<()> {
    state.version = state.version.checked_add(1).ok_or_else(store_unavailable)?;
    Ok(())
}
fn valid_username(username: &str) -> bool {
    !username.trim().is_empty() && username.len() <= 256 && !username.chars().any(char::is_control)
}
