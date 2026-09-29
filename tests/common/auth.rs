#![allow(dead_code)]
use chrono::{DateTime, Utc};
use skill_bom::{
    auth::*,
    domain::{Result, auth::ExpirySource},
    env::Clock,
};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    },
};

pub const ORIGIN: &str = AGENTCENTER_ORIGIN;
pub const EPOCH: i64 = 1_790_640_000;
pub struct FakeClock(pub AtomicI64);
impl Default for FakeClock {
    fn default() -> Self {
        Self(AtomicI64::new(EPOCH))
    }
}
impl FakeClock {
    pub fn advance(&self, seconds: i64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}
impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.0.load(Ordering::SeqCst), 0).unwrap()
    }
}
#[derive(Default)]
pub struct MemorySecrets {
    pub entries: Mutex<BTreeMap<(String, String), String>>,
    pub calls: AtomicUsize,
    pub fail_get: AtomicBool,
    pub fail_set: AtomicBool,
    pub fail_delete: AtomicBool,
}
impl SecretStore for MemorySecrets {
    fn get(&self, namespace: &str, reference: &str) -> Result<Option<Secret>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_get.load(Ordering::SeqCst) {
            return Err(store_unavailable());
        }
        Ok(self
            .entries
            .lock()
            .unwrap()
            .get(&(namespace.into(), reference.into()))
            .cloned()
            .map(Secret::new))
    }
    fn set(&self, namespace: &str, reference: &str, secret: &Secret) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_set.load(Ordering::SeqCst) {
            return Err(store_unavailable());
        }
        self.entries
            .lock()
            .unwrap()
            .insert((namespace.into(), reference.into()), secret.expose().into());
        Ok(())
    }
    fn delete(&self, namespace: &str, reference: &str) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(store_unavailable());
        }
        self.entries
            .lock()
            .unwrap()
            .remove(&(namespace.into(), reference.into()));
        Ok(())
    }
}
#[derive(Default)]
pub struct Gateway {
    pub calls: AtomicUsize,
    pub rejected: AtomicBool,
}
impl LoginGateway for Gateway {
    fn login(&self, username: &str, password: &Secret, now: DateTime<Utc>) -> Result<LoginResult> {
        assert!(!username.is_empty());
        assert_eq!(password.expose(), "fixture-password");
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.rejected.load(Ordering::SeqCst) {
            return Err(required("fixture-password must not escape"));
        }
        Ok(LoginResult {
            token: Secret::new(format!("fixture-token-{n}")),
            expires_at: now + chrono::Duration::hours(4),
            expiry_source: ExpirySource::LocalPolicy,
        })
    }
}
pub fn password() -> Secret {
    Secret::new("fixture-password".into())
}
pub struct NoCredentials;
impl CredentialProvider for NoCredentials {
    fn acquire(&self, _: &str, _: Option<String>) -> Result<Credential> {
        panic!("authentication must not be called")
    }
    fn refresh(&self, _: &str, _: &Credential) -> Result<Credential> {
        panic!("authentication must not be called")
    }
}
