//! Origin-bound credential policy. Sources consume this boundary; the resolver does not.
mod keyring;
mod login;
mod session;
pub use keyring::SystemSecrets;
pub use login::{LoginGateway, LoginResult, W3Gateway};
pub use session::Sessions;

use crate::domain::{Error, Result, auth::AuthMethod};
use crate::{env, net};
use std::fmt;
use zeroize::Zeroizing;

pub const AGENTCENTER_ORIGIN: &str = "https://agent.huawei.com";
pub const LOGIN_HINT: &str = "Run skill-bom auth login to authenticate again.";

/// Secrets cannot be serialized, and diagnostics never reveal their contents.
pub struct Secret(Zeroizing<String>);
impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

pub trait SecretStore {
    fn get(&self, namespace: &str, reference: &str) -> Result<Option<Secret>>;
    fn set(&self, namespace: &str, reference: &str, secret: &Secret) -> Result<()>;
    /// Missing entries are already deleted; all other failures must propagate.
    fn delete(&self, namespace: &str, reference: &str) -> Result<()>;
}

#[derive(Debug)]
pub struct Credential {
    pub token: Secret,
    pub(crate) stamp: Option<Stamp>,
}
#[derive(Debug, Clone)]
pub(crate) struct Stamp {
    origin: String,
    method: AuthMethod,
    version: u64,
    login_id: String,
}
impl Credential {
    pub fn explicit(token: String) -> Self {
        Self {
            token: Secret::new(token),
            stamp: None,
        }
    }
    pub fn is_explicit(&self) -> bool {
        self.stamp.is_none()
    }
    pub fn can_refresh(&self) -> bool {
        self.stamp
            .as_ref()
            .is_some_and(|stamp| stamp.method.is_w3())
    }
}

pub trait CredentialProvider {
    fn acquire(&self, origin: &str, explicit: Option<String>) -> Result<Credential>;
    fn acquire_optional(
        &self,
        origin: &str,
        explicit: Option<String>,
    ) -> Result<Option<Credential>> {
        self.acquire(origin, explicit).map(Some)
    }
    fn refresh(&self, origin: &str, rejected: &Credential) -> Result<Credential>;
}

pub fn required(message: &str) -> Error {
    Error::new("AUTH_REQUIRED", message, 2)
        .phase("authentication")
        .hint(LOGIN_HINT)
}
pub fn login_hint(origin: &str) -> String {
    crate::config::auth_origin(origin).map_or_else(
        |_| LOGIN_HINT.into(),
        |origin| format!("Run skill-bom auth login --origin {origin} to authenticate again."),
    )
}
pub fn required_for(origin: &str, message: &str) -> Error {
    required(message).hint(&login_hint(origin))
}
pub fn valid_token(token: &str) -> bool {
    !token.trim().is_empty()
        && token.len() <= 16384
        && token.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}
pub fn store_unavailable() -> Error {
    Error::new("AUTH_STORE_UNAVAILABLE", "The skill-bom credential store is unavailable", 2)
        .phase("authentication")
        .hint("Unlock or restore the system credential store and retry skill-bom auth login or auth logout. CI may use an explicit token_env.")
}

pub fn trusted_origin(origin: &str) -> bool {
    url::Url::parse(origin).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("agent.huawei.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none()
    })
}
/// Construction is inert: no directories, keyring or login HTTP until acquisition.
pub struct SystemProvider;
impl CredentialProvider for SystemProvider {
    fn acquire(&self, origin: &str, explicit: Option<String>) -> Result<Credential> {
        self.acquire_optional(origin, explicit)?.ok_or_else(|| {
            required_for(
                origin,
                "No local login; configure token_env or log in to this origin",
            )
        })
    }
    fn acquire_optional(
        &self,
        origin: &str,
        explicit: Option<String>,
    ) -> Result<Option<Credential>> {
        if let Some(token) = explicit.filter(|s| !s.is_empty()) {
            return Ok(Some(Credential::explicit(token)));
        }
        let sessions = Sessions::for_origin(env::directories()?.config_dir(), origin)?;
        if !sessions.status(&env::SystemClock)?.logged_in {
            return Ok(None);
        }
        if sessions.method() == AuthMethod::Token {
            return sessions
                .tokens(&SystemSecrets, &env::SystemClock)
                .acquire_optional(origin, None);
        }
        let http = net::Http::new(false)?;
        let gateway = W3Gateway(&http);
        sessions
            .service(&SystemSecrets, &gateway, &env::SystemClock)
            .acquire_optional(origin, None)
    }
    fn refresh(&self, origin: &str, rejected: &Credential) -> Result<Credential> {
        if !trusted_origin(origin) || !rejected.can_refresh() {
            return Err(required_for(
                origin,
                "Token credentials are never refreshed",
            ));
        }
        let sessions = Sessions::for_origin(env::directories()?.config_dir(), origin)?;
        let http = net::Http::new(false)?;
        let gateway = W3Gateway(&http);
        sessions
            .service(&SystemSecrets, &gateway, &env::SystemClock)
            .refresh(origin, rejected)
    }
}

#[derive(Clone, Copy)]
pub enum Operation {
    Read,
    Write,
}

/// A rejected explicit token is authoritative. Only marked reads may be replayed.
pub fn execute<T>(
    provider: &dyn CredentialProvider,
    origin: &str,
    explicit: Option<String>,
    operation: Operation,
    request: impl Fn(&str) -> Result<T>,
) -> Result<T> {
    let credential = provider.acquire(origin, explicit)?;
    match request(credential.token.expose()) {
        Err(error) if error.code == "AUTH_REQUIRED" => {
            if credential.is_explicit() {
                return Err(error.hint("Replace the explicitly configured token_env value; skill-bom auth login manages only the separate local session."));
            }
            if !credential.can_refresh() {
                return Err(error.hint(&login_hint(origin)));
            }
            let refreshed = provider.refresh(origin, &credential)?;
            match operation {
                Operation::Read => request(refreshed.token.expose()).map_err(|e| {
                    if e.code == "AUTH_REQUIRED" { e.hint(&login_hint(origin)) } else { e }
                }),
                Operation::Write => Err(error.hint("Credentials refreshed. The write was not replayed; check its outcome before retrying.")),
            }
        }
        result => result,
    }
}
