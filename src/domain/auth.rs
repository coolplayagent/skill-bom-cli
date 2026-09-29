//! Pure local session contracts and the documented W3 expiry policy.
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpirySource {
    Server,
    LocalPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub username: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub expiry_source: ExpirySource,
    pub credential_ref: String,
    pub login_id: String,
}

impl Session {
    pub fn refresh_due(&self, now: DateTime<Utc>) -> bool {
        // A backwards clock change cannot extend the locally recorded lifetime.
        now < self.issued_at || now >= self.issued_at + (self.expires_at - self.issued_at) / 2
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub version: u64,
    pub session: Option<Session>,
    /// References awaiting deletion, including interrupted credential publications.
    pub cleanup: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthStatus {
    pub logged_in: bool,
    pub username: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub expiry_source: Option<ExpirySource>,
    pub expired: bool,
    pub cleanup_pending: bool,
}

pub fn expiry(value: &Value, now: DateTime<Utc>) -> (DateTime<Utc>, ExpirySource) {
    let tokens = &value["cloudDragonTokens"];
    let relative = [
        &tokens["expiresIn"],
        &tokens["authTokenExpiresIn"],
        &value["authTokenExpiresIn"],
    ]
    .into_iter()
    .filter_map(Value::as_i64)
    .filter(|seconds| *seconds > 0)
    .filter_map(Duration::try_seconds)
    .filter_map(|duration| now.checked_add_signed(duration));
    let absolute = [&tokens["expireTime"], &value["expiresAt"]]
        .into_iter()
        .filter_map(Value::as_str)
        .filter_map(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&Utc));
    relative
        .chain(absolute)
        .min()
        .map(|time| (time, ExpirySource::Server))
        .unwrap_or_else(|| {
            // An unrepresentable local lifetime fails closed at the login boundary.
            let expires_at = now.checked_add_signed(Duration::hours(4)).unwrap_or(now);
            (expires_at, ExpirySource::LocalPolicy)
        })
}
