//! Official keyring-core backends, with no global default or plaintext fallback.
use super::{Secret, SecretStore, store_unavailable};
use crate::domain::Result;
use keyring_core::{CredentialStore, Entry};
use std::sync::Arc;

pub struct SystemSecrets;
fn backend() -> Result<Arc<CredentialStore>> {
    #[cfg(target_os = "linux")]
    let store = zbus_secret_service_keyring_store::Store::new();
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new();
    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new();
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    return store
        .map(|s| s as Arc<CredentialStore>)
        .map_err(|_| store_unavailable());
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    Err(store_unavailable())
}
fn entry(store: &CredentialStore, namespace: &str, reference: &str) -> Result<Entry> {
    #[cfg(target_os = "windows")]
    let modifiers = Some(std::collections::HashMap::from([("persistence", "Local")]));
    #[cfg(not(target_os = "windows"))]
    let modifiers = None;
    store
        .build(namespace, reference, modifiers.as_ref())
        .map_err(|_| store_unavailable())
}
fn read(entry: &Entry) -> Result<Option<Secret>> {
    match entry.get_password() {
        Ok(secret) => Ok(Some(Secret::new(secret))),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(_) => Err(store_unavailable()),
    }
}
fn delete(entry: &Entry) -> Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(_) => Err(store_unavailable()),
    }
}
impl SecretStore for SystemSecrets {
    fn get(&self, namespace: &str, reference: &str) -> Result<Option<Secret>> {
        read(&entry(backend()?.as_ref(), namespace, reference)?)
    }
    fn set(&self, namespace: &str, reference: &str, secret: &Secret) -> Result<()> {
        entry(backend()?.as_ref(), namespace, reference)?
            .set_password(secret.expose())
            .map_err(|_| store_unavailable())
    }
    fn delete(&self, namespace: &str, reference: &str) -> Result<()> {
        delete(&entry(backend()?.as_ref(), namespace, reference)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyring_adapter_maps_missing_and_redacts_backend_errors() {
        let store: Arc<CredentialStore> = keyring_core::mock::Store::new().unwrap();
        let configured = entry(store.as_ref(), "isolated-test", "ref");
        // The official mock rejects Windows-specific persistence modifiers.
        // Check the adapter result, then exercise read/delete without modifiers.
        if cfg!(target_os = "windows") {
            assert_eq!(configured.unwrap_err().code, "AUTH_STORE_UNAVAILABLE");
        } else {
            configured.unwrap();
        }
        let entry = store.build("isolated-test", "ref", None).unwrap();
        assert!(read(&entry).unwrap().is_none());
        entry.set_password("private-password").unwrap();
        assert_eq!(read(&entry).unwrap().unwrap().expose(), "private-password");
        let mock: &keyring_core::mock::Cred = entry.as_any().downcast_ref().unwrap();
        mock.set_error(keyring_core::Error::Invalid(
            "private-password".into(),
            "private-token".into(),
        ));
        let error = read(&entry).unwrap_err();
        assert_eq!(error.code, "AUTH_STORE_UNAVAILABLE");
        assert!(!format!("{error:?}").contains("private-password"));
        mock.set_error(keyring_core::Error::NoDefaultStore);
        assert!(delete(&entry).is_err());
        delete(&entry).unwrap();
        delete(&entry).unwrap();
    }
}
