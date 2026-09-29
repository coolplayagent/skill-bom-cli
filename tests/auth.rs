#[path = "common/auth.rs"]
mod fixture;
use fixture::*;
use skill_bom::{auth::*, domain::auth::ExpirySource, env::Clock};
use std::sync::atomic::Ordering;

#[test]
fn login_status_half_life_refresh_and_idempotent_logout() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(temp.path()).unwrap();
    let secrets = MemorySecrets::default();
    let gateway = Gateway::default();
    let clock = FakeClock::default();
    assert!(!sessions.status(&clock).unwrap().logged_in);
    assert!(!temp.path().join("auth-v1").exists());
    let service = sessions.service(&secrets, &gateway, &clock);
    assert_eq!(
        service.acquire(ORIGIN, None).unwrap_err().code,
        "AUTH_REQUIRED"
    );
    let status = service.login("alice", &password()).unwrap();
    assert_eq!(status.username.as_deref(), Some("alice"));
    assert_eq!(status.expiry_source, Some(ExpirySource::LocalPolicy));
    assert_eq!(
        status.expires_at.unwrap(),
        clock.now() + chrono::Duration::hours(4)
    );
    let metadata = std::fs::read_to_string(temp.path().join("auth-v1/session.json")).unwrap();
    assert!(!metadata.contains("fixture-password") && !metadata.contains("fixture-token"));
    clock.advance(7199);
    assert_eq!(
        service.acquire(ORIGIN, None).unwrap().token.expose(),
        "fixture-token-1"
    );
    clock.advance(1);
    assert_eq!(
        service.acquire(ORIGIN, None).unwrap().token.expose(),
        "fixture-token-2"
    );
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 2);
    assert_eq!(secrets.entries.lock().unwrap().len(), 2);
    clock.advance(14401);
    let calls = secrets.calls.load(Ordering::SeqCst);
    assert!(sessions.status(&clock).unwrap().expired);
    assert_eq!(secrets.calls.load(Ordering::SeqCst), calls);
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 2);
    sessions.logout(&secrets).unwrap();
    sessions.logout(&secrets).unwrap();
    assert!(secrets.entries.lock().unwrap().is_empty());
    assert!(!sessions.status(&clock).unwrap().logged_in);
    assert_eq!(
        service.acquire(ORIGIN, None).unwrap_err().code,
        "AUTH_REQUIRED"
    );
}

#[test]
fn rejected_login_retains_session_and_failed_refresh_has_login_hint() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(temp.path()).unwrap();
    let secrets = MemorySecrets::default();
    let gateway = Gateway::default();
    let clock = FakeClock::default();
    let service = sessions.service(&secrets, &gateway, &clock);
    service.login("alice", &password()).unwrap();
    let before = std::fs::read(temp.path().join("auth-v1/session.json")).unwrap();
    gateway.rejected.store(true, Ordering::SeqCst);
    assert!(service.login("bob", &password()).is_err());
    assert_eq!(
        std::fs::read(temp.path().join("auth-v1/session.json")).unwrap(),
        before
    );
    clock.advance(7200);
    let error = service.acquire(ORIGIN, None).unwrap_err();
    assert!(error.hint.contains("skill-bom auth login"));
    assert!(!format!("{error:?}").contains("fixture-password"));
    assert_eq!(
        std::fs::read(temp.path().join("auth-v1/session.json")).unwrap(),
        before
    );
}

#[test]
fn origin_restriction_and_explicit_tokens_never_consult_storage() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(temp.path()).unwrap();
    let secrets = MemorySecrets::default();
    let gateway = Gateway::default();
    let clock = FakeClock::default();
    let service = sessions.service(&secrets, &gateway, &clock);
    for origin in [
        "https://attacker.test",
        "http://agent.huawei.com",
        "https://agent.huawei.com:444",
        "https://agent.huawei.com.evil",
        "https://user@agent.huawei.com",
        "https://agent.huawei.com/path",
        "https://agent.huawei.com?token=x",
        "https://agent.huawei.com#x",
        "garbage",
    ] {
        assert!(!trusted_origin(origin));
        assert_eq!(
            service.acquire(origin, None).unwrap_err().code,
            "MISSING_AUTH_TOKEN"
        );
        let explicit = service
            .acquire(origin, Some("explicit-secret".into()))
            .unwrap();
        assert!(explicit.is_explicit());
        assert_eq!(explicit.token.expose(), "explicit-secret");
    }
    assert!(trusted_origin("https://agent.huawei.com:443/"));
    let explicit = service
        .acquire(ORIGIN, Some("explicit-secret".into()))
        .unwrap();
    assert!(service.refresh(ORIGIN, &explicit).is_err());
    let error = execute(
        &service,
        ORIGIN,
        Some("explicit-secret".into()),
        Operation::Read,
        |_| Err::<(), _>(required("rejected")),
    )
    .unwrap_err();
    assert!(error.hint.contains("token_env"));
    assert_eq!(secrets.calls.load(Ordering::SeqCst), 0);
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
    assert!(!format!("{explicit:?}").contains("explicit-secret"));
    let system = SystemProvider
        .acquire(ORIGIN, Some("override".into()))
        .unwrap();
    assert!(system.is_explicit());
    assert!(SystemProvider.refresh(ORIGIN, &system).is_err());
}

#[test]
fn retry_is_bounded_writes_are_not_replayed_and_refreshes_merge() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(temp.path()).unwrap();
    let secrets = MemorySecrets::default();
    let gateway = Gateway::default();
    let clock = FakeClock::default();
    let service = sessions.service(&secrets, &gateway, &clock);
    service.login("alice", &password()).unwrap();
    let old = service.acquire(ORIGIN, None).unwrap();
    let new = service.refresh(ORIGIN, &old).unwrap();
    assert_eq!(
        service.refresh(ORIGIN, &old).unwrap().token.expose(),
        new.token.expose()
    );
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 2);
    for operation in [Operation::Read, Operation::Write] {
        let calls = std::cell::Cell::new(0);
        let result = execute(&service, ORIGIN, None, operation, |_| {
            calls.set(calls.get() + 1);
            Err::<(), _>(required("rejected"))
        });
        assert!(result.is_err());
        assert_eq!(
            calls.get(),
            if matches!(operation, Operation::Read) {
                2
            } else {
                1
            }
        );
    }
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 4);
    let calls = std::cell::Cell::new(0);
    let success = execute(&service, ORIGIN, None, Operation::Read, |_| {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            Err(required("expired"))
        } else {
            Ok(42)
        }
    })
    .unwrap();
    assert_eq!(success, 42);
    assert_eq!(calls.get(), 2);
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 5);
    assert!(
        execute(&service, ORIGIN, None, Operation::Read, |_| Err::<(), _>(
            skill_bom::domain::Error::new("PROTOCOL", "invalid", 2)
        ))
        .is_err()
    );
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 5);
}

#[test]
fn logout_tombstone_and_new_login_prevent_old_refresh_resurrection() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(temp.path()).unwrap();
    let secrets = MemorySecrets::default();
    let gateway = Gateway::default();
    let clock = FakeClock::default();
    let service = sessions.service(&secrets, &gateway, &clock);
    service.login("alice", &password()).unwrap();
    let old = service.acquire(ORIGIN, None).unwrap();
    secrets.fail_delete.store(true, Ordering::SeqCst);
    assert_eq!(
        sessions.logout(&secrets).unwrap_err().code,
        "AUTH_STORE_UNAVAILABLE"
    );
    let status = sessions.status(&clock).unwrap();
    assert!(!status.logged_in && status.cleanup_pending);
    assert!(service.refresh(ORIGIN, &old).is_err());
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
    secrets.fail_delete.store(false, Ordering::SeqCst);
    sessions.logout(&secrets).unwrap();
    service.login("bob", &password()).unwrap();
    assert!(service.refresh(ORIGIN, &old).is_err());
    assert_eq!(
        sessions.status(&clock).unwrap().username.as_deref(),
        Some("bob")
    );
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_publication_missing_secrets_and_isolated_config_roots() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(&temp.path().join("a")).unwrap();
    let other = Sessions::new(&temp.path().join("b")).unwrap();
    let secrets = MemorySecrets::default();
    let gateway = Gateway::default();
    let clock = FakeClock::default();
    let service = sessions.service(&secrets, &gateway, &clock);
    service.login("alice", &password()).unwrap();
    secrets.fail_set.store(true, Ordering::SeqCst);
    assert_eq!(
        service.login("bob", &password()).unwrap_err().code,
        "AUTH_STORE_UNAVAILABLE"
    );
    assert_eq!(
        sessions.status(&clock).unwrap().username.as_deref(),
        Some("alice")
    );
    assert!(sessions.status(&clock).unwrap().cleanup_pending);
    secrets.fail_set.store(false, Ordering::SeqCst);
    other
        .service(&secrets, &gateway, &clock)
        .login("alice", &password())
        .unwrap();
    assert_eq!(
        secrets
            .entries
            .lock()
            .unwrap()
            .keys()
            .map(|k| &k.0)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    secrets.fail_get.store(true, Ordering::SeqCst);
    assert_eq!(
        service.acquire(ORIGIN, None).unwrap_err().code,
        "AUTH_STORE_UNAVAILABLE"
    );
    secrets.fail_get.store(false, Ordering::SeqCst);
    secrets.entries.lock().unwrap().clear();
    assert_eq!(
        service.acquire(ORIGIN, None).unwrap_err().code,
        "AUTH_REQUIRED"
    );
    assert!(service.login("", &password()).is_err());
    assert!(service.login("a\nb", &password()).is_err());
    assert!(service.login("alice", &Secret::new(String::new())).is_err());
    sessions.logout(&secrets).unwrap();
}

#[test]
fn corrupt_metadata_and_unwritable_paths_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(temp.path()).unwrap();
    let directory = temp.path().join("auth-v1");
    std::fs::create_dir(&directory).unwrap();
    for value in [
        "not json",
        r#"{"version":0,"session":null,"cleanup":["foreign-ref"]}"#,
        r#"{"version":0,"session":null,"cleanup":[],"password":"never echo"}"#,
    ] {
        std::fs::write(directory.join("session.json"), value).unwrap();
        let error = sessions.status(&FakeClock::default()).unwrap_err();
        assert_eq!(error.code, "AUTH_STORE_UNAVAILABLE");
        assert!(!format!("{error:?}").contains("never echo"));
    }
    let file = temp.path().join("not-directory");
    std::fs::write(&file, "x").unwrap();
    let sessions = Sessions::new(&file).unwrap();
    assert!(sessions.logout(&MemorySecrets::default()).is_err());
}
