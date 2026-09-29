//! Journaled metadata and a bounded process lock for login, refresh and logout.
use super::{
    Credential, CredentialProvider, LoginGateway, LoginResult, Secret, SecretStore, Stamp,
    check_origin, required, store_unavailable,
};
use crate::{
    domain::{
        Result,
        auth::{AuthStatus, Session, SessionState},
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
}
impl Sessions {
    pub fn new(config_root: &Path) -> Result<Self> {
        let root = paths::absolute(config_root).map_err(|_| store_unavailable())?;
        Ok(Self {
            namespace: format!(
                "org.skill-bom.w3.v1.{}",
                crate::domain::digest(root.as_os_str().as_encoded_bytes())
            ),
            root: root.join("auth-v1"),
            lock_timeout: Duration::from_secs(100),
        })
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
            gateway,
            clock,
        }
    }
    fn read(&self) -> Result<SessionState> {
        let path = self.root.join("session.json");
        paths::no_symlink(&path).map_err(|_| store_unavailable())?;
        if !path.try_exists().map_err(|_| store_unavailable())? {
            return Ok(SessionState::default());
        }
        let bytes = paths::read(&path, 32768).map_err(|_| store_unavailable())?;
        let state: SessionState =
            serde_json::from_slice(&bytes).map_err(|_| store_unavailable())?;
        let valid_ref = |s: &str| uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == s);
        if state.cleanup.len() > 4
            || state.cleanup.iter().any(|r| !valid_ref(r))
            || state.session.as_ref().is_some_and(|s| {
                !valid_ref(&s.credential_ref)
                    || !valid_ref(&s.login_id)
                    || state.cleanup.contains(&s.credential_ref)
                    || s.expires_at <= s.issued_at
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
            logged_in: state.session.is_some(),
            username: state.session.as_ref().map(|s| s.username.clone()),
            expires_at: state.session.as_ref().map(|s| s.expires_at),
            expiry_source: state.session.as_ref().map(|s| s.expiry_source),
            expired: state
                .session
                .as_ref()
                .is_some_and(|s| s.expires_at <= clock.now()),
            cleanup_pending: !state.cleanup.is_empty(),
        })
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
            for kind in ["password", "token"] {
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
    gateway: &'a dyn LoginGateway,
    clock: &'a dyn Clock,
}
impl Service<'_> {
    pub fn login(&self, username: &str, password: &Secret) -> Result<AuthStatus> {
        if !valid_username(username)
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
        let result = self.gateway.login(username, password, now)?;
        let session = Session {
            username: username.into(),
            issued_at: now,
            expires_at: result.expires_at,
            expiry_source: result.expiry_source,
            credential_ref: uuid::Uuid::new_v4().to_string(),
            login_id: uuid::Uuid::new_v4().to_string(),
        };
        self.publish(&mut state, session, password, &result)?;
        self.sessions.status(self.clock)
    }
    fn publish(
        &self,
        state: &mut SessionState,
        session: Session,
        password: &Secret,
        result: &LoginResult,
    ) -> Result<()> {
        // Journal the new reference first. Interrupted writes stay discoverable by logout.
        state.cleanup.push(session.credential_ref.clone());
        self.sessions.write(state)?;
        for (kind, secret) in [("password", password), ("token", &result.token)] {
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
            .ok_or_else(|| required("The local W3 credential is missing"))
    }
    fn credential(&self, state: &SessionState) -> Result<Credential> {
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| required("No local W3 login"))?;
        Ok(Credential {
            token: self.get(session, "token")?,
            stamp: Some(Stamp {
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
            .login(&session.username, &password, now)
            .map_err(|error| {
                // A gateway failure must never echo the password, token or response body.
                crate::domain::Error::new(&error.code, "W3 reauthentication failed", 2)
                    .phase("authentication")
                    .hint(super::LOGIN_HINT)
            })?;
        session.issued_at = now;
        session.expires_at = result.expires_at;
        session.expiry_source = result.expiry_source;
        session.credential_ref = uuid::Uuid::new_v4().to_string();
        self.publish(state, session, &password, &result)?;
        self.credential(state)
    }
}
impl CredentialProvider for Service<'_> {
    fn acquire(&self, origin: &str, explicit: Option<String>) -> Result<Credential> {
        if let Some(token) = explicit.filter(|t| !t.is_empty()) {
            return Ok(Credential::explicit(token));
        }
        check_origin(origin)?;
        let _guard = self.sessions.lock()?;
        let mut state = self.sessions.read()?;
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| required("No local W3 login"))?;
        if session.refresh_due(self.clock.now()) {
            self.renew(&mut state)
        } else {
            self.credential(&state)
        }
    }
    fn refresh(&self, origin: &str, rejected: &Credential) -> Result<Credential> {
        check_origin(origin)?;
        let stamp = rejected
            .stamp
            .as_ref()
            .ok_or_else(|| required("Explicit tokens are never refreshed"))?;
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
