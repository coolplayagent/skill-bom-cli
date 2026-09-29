use super::{LOGIN_HINT, Secret, required};
use crate::{
    domain::{
        Error, Result,
        auth::{ExpirySource, expiry},
    },
    net::Transport,
};
use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct LoginResult {
    pub token: Secret,
    pub expires_at: DateTime<Utc>,
    pub expiry_source: ExpirySource,
}
pub trait LoginGateway {
    fn login(&self, username: &str, password: &Secret, now: DateTime<Utc>) -> Result<LoginResult>;
}
pub struct W3Gateway<'a, T: Transport + ?Sized>(pub &'a T);
impl<T: Transport + ?Sized> LoginGateway for W3Gateway<'_, T> {
    fn login(&self, username: &str, password: &Secret, now: DateTime<Utc>) -> Result<LoginResult> {
        let response = self
            .0
            .secure_login(username, password.expose())
            .map_err(|mut error| {
                error.exit_code = 2;
                error.phase("authentication").hint(LOGIN_HINT)
            })?;
        let value: serde_json::Value = serde_json::from_slice(&response.bytes).map_err(|_| {
            Error::new("PROTOCOL", "W3 login returned invalid JSON", 2).hint(LOGIN_HINT)
        })?;
        let token = value["cloudDragonTokens"]["authToken"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                Error::new("MISSING_AUTH_TOKEN", "W3 login returned no authToken", 2)
                    .hint(LOGIN_HINT)
            })?;
        if token.len() > 16384 || !token.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
            return Err(required("W3 login returned an unusable authToken"));
        }
        let (expires_at, expiry_source) = expiry(&value, now);
        if expires_at <= now {
            return Err(required("W3 login returned an expired token"));
        }
        Ok(LoginResult {
            token: Secret::new(token.into()),
            expires_at,
            expiry_source,
        })
    }
}
