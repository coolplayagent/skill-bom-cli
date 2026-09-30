mod common;
#[path = "common/auth.rs"]
mod fixture;
use fixture::*;
use skill_bom::{
    auth::*,
    config::{self, Manifest},
    domain::{Error, Result, auth::AuthMethod},
    net::{Response, Transport},
    sources::{agentcenter, clawhub},
};
use std::{cell::Cell, path::Path, sync::atomic::Ordering};

const HUB: &str = "https://clawhub.example";
const PRIVATE: &str = "https://registry.example:8443";
fn token(value: &str) -> Secret {
    Secret::new(value.into())
}
fn metadata(root: &Path, origin: &str) -> std::path::PathBuf {
    root.join("auth-v1/origins")
        .join(skill_bom::domain::digest(origin.as_bytes()))
        .join("session.json")
}

#[test]
fn origins_coexist_and_replacement_logout_and_refresh_are_independent() {
    let temp = tempfile::tempdir().unwrap();
    let w3 = Sessions::new(temp.path()).unwrap();
    let hub = Sessions::for_origin(temp.path(), "https://CLAWHub.example:443/").unwrap();
    let private = Sessions::for_origin(temp.path(), PRIVATE).unwrap();
    let secrets = MemorySecrets::default();
    let clock = FakeClock::default();
    let gateway = Gateway::default();
    let w3_service = w3.service(&secrets, &gateway, &clock);
    let hub_service = hub.tokens(&secrets, &clock);
    let private_service = private.tokens(&secrets, &clock);
    w3_service.login("alice", &password()).unwrap();
    hub_service
        .login_token("bob", &token("hub-secret"))
        .unwrap();
    private_service
        .login_token("carol", &token("private-secret"))
        .unwrap();
    assert_eq!(hub.origin(), HUB);
    assert_eq!(
        w3_service.acquire(ORIGIN, None).unwrap().token.expose(),
        "fixture-token-1"
    );
    assert_eq!(
        hub_service.acquire(HUB, None).unwrap().token.expose(),
        "hub-secret"
    );
    assert_eq!(
        private_service
            .acquire(PRIVATE, None)
            .unwrap()
            .token
            .expose(),
        "private-secret"
    );
    assert_eq!(
        secrets
            .entries
            .lock()
            .unwrap()
            .keys()
            .map(|key| &key.0)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    let legacy = std::fs::read_to_string(temp.path().join("auth-v1/session.json")).unwrap();
    assert!(!legacy.contains("\"origin\"") && !legacy.contains("\"method\""));
    let raw = std::fs::read_to_string(metadata(temp.path(), HUB)).unwrap();
    assert!(!raw.contains("hub-secret"));
    assert!(raw.contains(HUB));
    let old_w3 = w3_service.acquire(ORIGIN, None).unwrap();
    hub_service
        .login_token("other-bob", &token("replacement"))
        .unwrap();
    assert_eq!(secrets.entries.lock().unwrap().len(), 4);
    assert_eq!(
        hub.status(&clock).unwrap().username.as_deref(),
        Some("other-bob")
    );
    hub.logout(&secrets).unwrap();
    hub.logout(&secrets).unwrap();
    assert_eq!(
        private_service
            .acquire(PRIVATE, None)
            .unwrap()
            .token
            .expose(),
        "private-secret"
    );
    assert!(w3_service.refresh(ORIGIN, &old_w3).is_ok());
    assert_eq!(
        w3.status(&clock).unwrap().username.as_deref(),
        Some("alice")
    );
    assert!(!hub.status(&clock).unwrap().logged_in);
    assert_eq!(
        Sessions::all_statuses(temp.path(), &clock)
            .unwrap()
            .iter()
            .map(|s| s.origin.as_str())
            .collect::<Vec<_>>(),
        vec![ORIGIN, PRIVATE]
    );
}

#[test]
fn tokens_have_unknown_expiry_never_refresh_and_never_cross_origins() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::for_origin(temp.path(), HUB).unwrap();
    let secrets = MemorySecrets::default();
    let clock = FakeClock::default();
    let service = sessions.tokens(&secrets, &clock);
    assert!(service.acquire_optional(HUB, None).unwrap().is_none());
    assert!(!temp.path().join("auth-v1").exists());
    assert_eq!(secrets.calls.load(Ordering::SeqCst), 0);
    let status = service.login_token("bob", &token("hub-token")).unwrap();
    assert_eq!(status.method, AuthMethod::Token);
    assert!(
        status.expired.is_none() && status.expires_at.is_none() && status.expiry_source.is_none()
    );
    clock.advance(100 * 86400);
    let saved = service.acquire(HUB, None).unwrap();
    assert!(!saved.is_explicit() && !saved.can_refresh());
    assert!(service.refresh(HUB, &saved).unwrap_err().hint.contains(HUB));
    assert!(SystemProvider.refresh(HUB, &saved).is_err());
    assert!(service.acquire(PRIVATE, None).is_err());
    let calls = Cell::new(0);
    let error = execute(&service, HUB, None, Operation::Read, |_| {
        calls.set(calls.get() + 1);
        Err::<(), _>(required("rejected"))
    })
    .unwrap_err();
    assert_eq!(calls.get(), 1);
    assert!(error.hint.contains(HUB));
    for invalid in ["", "  ", "bad\r\nheader", "非ASCII"] {
        assert_eq!(
            service
                .login_token("bob", &token(invalid))
                .unwrap_err()
                .code,
            "AUTH_INPUT"
        );
    }
    assert!(
        service
            .login_token("bob", &token(&"a".repeat(16385)))
            .is_err()
    );
    assert!(service.login_token("a\nb", &token("valid")).is_err());
    assert!(service.login("bob", &password()).is_err());
    let official = Sessions::new(temp.path()).unwrap();
    assert!(
        official
            .tokens(&secrets, &clock)
            .login_token("alice", &token("valid"))
            .is_err()
    );
    std::fs::write(metadata(temp.path(), HUB), "broken").unwrap();
    assert!(
        service
            .acquire(HUB, Some("override".into()))
            .unwrap()
            .is_explicit()
    );
    assert!(sessions.status(&clock).is_err());
}

#[test]
fn token_publication_failure_and_pending_cleanup_preserve_other_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let first = Sessions::for_origin(temp.path(), HUB).unwrap();
    let other = Sessions::for_origin(temp.path(), PRIVATE).unwrap();
    let secrets = MemorySecrets::default();
    let clock = FakeClock::default();
    let service = first.tokens(&secrets, &clock);
    service.login_token("bob", &token("first")).unwrap();
    other
        .tokens(&secrets, &clock)
        .login_token("carol", &token("second"))
        .unwrap();
    secrets.fail_set.store(true, Ordering::SeqCst);
    assert!(service.login_token("replacement", &token("new")).is_err());
    assert_eq!(
        first.status(&clock).unwrap().username.as_deref(),
        Some("bob")
    );
    assert!(first.status(&clock).unwrap().cleanup_pending);
    secrets.fail_set.store(false, Ordering::SeqCst);
    secrets.fail_delete.store(true, Ordering::SeqCst);
    assert!(first.logout(&secrets).is_err());
    assert!(!first.status(&clock).unwrap().logged_in);
    let statuses = Sessions::all_statuses(temp.path(), &clock).unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses[0].cleanup_pending);
    assert_eq!(
        other
            .tokens(&secrets, &clock)
            .acquire(PRIVATE, None)
            .unwrap()
            .token
            .expose(),
        "second"
    );
    secrets.fail_delete.store(false, Ordering::SeqCst);
    first.logout(&secrets).unwrap();
    assert_eq!(
        Sessions::all_statuses(temp.path(), &clock).unwrap().len(),
        1
    );
}

#[test]
fn catalog_and_canonical_origins_reject_misbinding_and_corrupt_state() {
    let temp = tempfile::tempdir().unwrap();
    let clock = FakeClock::default();
    assert!(
        Sessions::all_statuses(temp.path(), &clock)
            .unwrap()
            .is_empty()
    );
    assert!(!temp.path().join("auth-v1").exists());
    for invalid in [
        "garbage",
        "http://example.com",
        "https://user:secret@example.com",
        "https://example.com/path",
        "https://example.com?secret=x",
        "https://example.com#fragment",
    ] {
        assert!(Sessions::for_origin(temp.path(), invalid).is_err());
    }
    assert_eq!(
        config::auth_origin("https://EXAMPLE.com:443/").unwrap(),
        "https://example.com"
    );
    assert_eq!(
        config::auth_origin("http://127.0.0.1:1234").unwrap(),
        "http://127.0.0.1:1234"
    );
    assert_eq!(
        config::registry_origin("https://EXAMPLE.com/base/").unwrap(),
        "https://example.com"
    );
    assert!(config::auth_origin(&format!("https://{}.example", "a".repeat(2048))).is_err());
    let sessions = Sessions::for_origin(temp.path(), HUB).unwrap();
    sessions
        .tokens(&MemorySecrets::default(), &clock)
        .login_token("bob", &token("secret"))
        .unwrap();
    let path = metadata(temp.path(), HUB);
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for field in [
        "origin",
        "method",
        "expires_at",
        "expiry_source",
        "credential_ref",
        "username",
    ] {
        let mut bad = original.clone();
        if field == "origin" {
            bad[field] = PRIVATE.into();
        } else {
            bad["session"][field] = match field {
                "method" => "w3".into(),
                "expires_at" => "2099-01-01T00:00:00Z".into(),
                "expiry_source" => "server".into(),
                "username" => "".into(),
                _ => "not-a-ref".into(),
            };
        }
        std::fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(sessions.status(&clock).is_err(), "{field}");
        assert!(
            Sessions::all_statuses(temp.path(), &clock).is_err(),
            "{field}"
        );
    }
    std::fs::remove_file(&path).unwrap();
    assert!(
        Sessions::all_statuses(temp.path(), &clock)
            .unwrap()
            .is_empty()
    );
    std::fs::write(
        path.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("foreign-file"),
        "x",
    )
    .unwrap();
    assert!(Sessions::all_statuses(temp.path(), &clock).is_err());
}

#[test]
fn catalog_limit_and_symlinks_fail_without_keyring_access() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("auth-v1/origins");
    std::fs::create_dir_all(&directory).unwrap();
    for index in 0..1025 {
        std::fs::create_dir(directory.join(format!("{index:064x}"))).unwrap();
    }
    assert_eq!(
        Sessions::all_statuses(temp.path(), &FakeClock::default())
            .unwrap_err()
            .code,
        "RESOURCE_LIMIT"
    );
    #[cfg(unix)]
    {
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join("auth-v1")).unwrap();
        std::os::unix::fs::symlink(&directory, other.path().join("auth-v1/origins")).unwrap();
        assert_eq!(
            Sessions::all_statuses(other.path(), &FakeClock::default())
                .unwrap_err()
                .code,
            "AUTH_STORE_UNAVAILABLE"
        );
    }
}

#[test]
fn registry_http_authentication_and_redirects_keep_archive_behavior_separate() {
    use common::http::{Reply, Server};
    let server = Server::new(|path, headers| {
        if path == "/json" {
            return Reply::json(serde_json::json!({"ok":true}));
        }
        if path == "/invalid" {
            return Reply::bytes("application/json", b"invalid".to_vec());
        }
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-token")
        );
        Reply::status(401, "secret response body")
    });
    let http = skill_bom::net::Http::new(false).unwrap();
    let error = http
        .get_registry(&server.url, Some("fixture-token"))
        .err()
        .unwrap();
    assert_eq!(error.code, "AUTH_REQUIRED");
    assert!(!format!("{error:?}").contains("secret response body"));
    assert_eq!(
        http.get(&server.url, Some("fixture-token"))
            .err()
            .unwrap()
            .code,
        "HTTP_STATUS"
    );
    assert_eq!(
        http.registry_json(&format!("{}/json", server.url), None)
            .unwrap()["ok"],
        true
    );
    assert_eq!(
        http.registry_json(&format!("{}/invalid", server.url), None)
            .unwrap_err()
            .code,
        "PROTOCOL"
    );
    let destination = Server::new(|_, headers| {
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        Reply::status(401, "different origin")
    });
    let target = destination.url.clone();
    let redirect = Server::new(move |_, headers| {
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-token")
        );
        let mut reply = Reply::status(302, "redirect");
        reply.headers.push(("Location".into(), target.clone()));
        reply
    });
    assert_eq!(
        http.get_registry(&redirect.url, Some("fixture-token"))
            .err()
            .unwrap()
            .code,
        "HTTP_STATUS"
    );
    assert_eq!(destination.requests.load(Ordering::SeqCst), 1);
}

struct RegistryHttp {
    expected: Option<String>,
    calls: Cell<usize>,
    reject: bool,
}
impl RegistryHttp {
    fn json(&self, value: serde_json::Value) -> Result<Response> {
        Ok(Response {
            bytes: serde_json::to_vec(&value).unwrap(),
            content_type: "application/json".into(),
        })
    }
}
impl Transport for RegistryHttp {
    fn get(&self, url: &str, token: Option<&str>) -> Result<Response> {
        assert_eq!(token, self.expected.as_deref());
        self.calls.set(self.calls.get() + 1);
        if self.reject {
            return Err(required("denied"));
        }
        if url.contains("/download?") {
            return Ok(Response {
                bytes: common::archive("review", "1.0.0"),
                content_type: "application/zip".into(),
            });
        }
        if url.contains("/versions/1.0.0?") {
            return self.json(
                serde_json::json!({"skill":{"slug":"review"},"version":{"version":"1.0.0"}}),
            );
        }
        if url.contains("/versions?") {
            return self.json(serde_json::json!({"items":[{"version":"1.0.0"}],"nextCursor":null}));
        }
        self.json(serde_json::json!({"owner":{"handle":"owner"},"skill":{"slug":"review"}}))
    }
    fn get_x_auth(&self, _: &str, token: &str) -> Result<Response> {
        assert_eq!(Some(token), self.expected.as_deref());
        self.calls.set(self.calls.get() + 1);
        if self.reject {
            return Err(required("denied"));
        }
        self.json(serde_json::json!({"code":200,"data":{"skillId":"review","version":"1.0.0"}}))
    }
    fn post_json(&self, _: &str, _: &serde_json::Value, token: &str) -> Result<Response> {
        assert_eq!(Some(token), self.expected.as_deref());
        self.calls.set(self.calls.get() + 1);
        Ok(Response {
            bytes: common::archive("review", "1.0.0"),
            content_type: "application/zip".into(),
        })
    }
}

#[test]
fn registries_use_saved_tokens_with_explicit_precedence_and_anonymous_clawhub() {
    let temp = tempfile::tempdir().unwrap();
    let secrets = MemorySecrets::default();
    let clock = FakeClock::default();
    for kind in ["clawhub", "agentcenter"] {
        let url = if kind == "clawhub" {
            format!("{HUB}/base")
        } else {
            HUB.into()
        };
        let package = if kind == "clawhub" {
            "@owner/review"
        } else {
            "review"
        };
        let mut manifest = Manifest::parse(&format!("schema_version=1\n[project]\nname='test'\n[registries.r]\nkind='{kind}'\nurl='{url}'\n[dependencies.review]\nregistry='r'\npackage='{package}'\nversion='=1.0.0'\n")).unwrap();
        let dependency = &manifest.dependencies["review"];
        let source = manifest.source(dependency).unwrap();
        let sessions = Sessions::for_origin(temp.path(), HUB).unwrap();
        let service = sessions.tokens(&secrets, &clock);
        sessions.logout(&secrets).unwrap();
        if kind == "clawhub" {
            let http = RegistryHttp {
                expected: None,
                calls: Cell::new(0),
                reject: false,
            };
            assert_eq!(
                clawhub::candidates_with_auth(&http, &service, &manifest, &source, dependency)
                    .unwrap()
                    .len(),
                1
            );
        }
        service
            .login_token("account", &token("saved-token"))
            .unwrap();
        let http = RegistryHttp {
            expected: Some("saved-token".into()),
            calls: Cell::new(0),
            reject: false,
        };
        let candidates = if kind == "clawhub" {
            clawhub::candidates_with_auth(&http, &service, &manifest, &source, dependency)
        } else {
            agentcenter::candidates_with_auth(&http, &service, &manifest, &source, dependency)
        }
        .unwrap();
        let dest = tempfile::tempdir().unwrap();
        if kind == "clawhub" {
            clawhub::fetch_with_auth(
                &http,
                &service,
                &manifest,
                &source,
                &candidates[0],
                dest.path(),
            )
            .unwrap();
        } else {
            agentcenter::fetch_with_auth(
                &http,
                &service,
                &manifest,
                &source,
                &candidates[0],
                None,
                dest.path(),
            )
            .unwrap();
        }
        assert!(dest.path().join("SKILL.md").is_file());
        let rejected = RegistryHttp {
            reject: true,
            calls: Cell::new(0),
            ..http
        };
        let result = if kind == "clawhub" {
            clawhub::candidates_with_auth(&rejected, &service, &manifest, &source, dependency)
        } else {
            agentcenter::candidates_with_auth(&rejected, &service, &manifest, &source, dependency)
        };
        assert!(result.unwrap_err().hint.contains(HUB));
        assert_eq!(rejected.calls.get(), 1);
        manifest.registries.get_mut("r").unwrap().token_env = Some("PATH".into());
        let dependency = &manifest.dependencies["review"];
        let explicit = RegistryHttp {
            expected: Some(std::env::var("PATH").unwrap()),
            calls: Cell::new(0),
            reject: true,
        };
        secrets.fail_get.store(true, Ordering::SeqCst);
        let error: Error = if kind == "clawhub" {
            clawhub::candidates_with_auth(&explicit, &service, &manifest, &source, dependency)
                .unwrap_err()
        } else {
            agentcenter::candidates_with_auth(&explicit, &service, &manifest, &source, dependency)
                .unwrap_err()
        };
        assert!(error.hint.contains("token_env"));
        assert_eq!(explicit.calls.get(), 1);
        secrets.fail_get.store(false, Ordering::SeqCst);
    }
}
