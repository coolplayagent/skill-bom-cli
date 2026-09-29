mod common;
#[path = "common/auth.rs"]
mod fixture;
use fixture::*;
use serde_json::{Value, json};
use skill_bom::{
    agentcenter,
    auth::*,
    config::Manifest,
    domain::{Error, Result, auth::ExpirySource},
    env::Clock,
    net::{Response, Transport},
    sources::{Provider, SourceProvider},
};
use std::cell::Cell;
use std::sync::atomic::Ordering;

struct LoginFixture(Value);
impl Transport for LoginFixture {
    fn get(&self, _: &str, _: Option<&str>) -> Result<Response> {
        panic!("not GET")
    }
    fn secure_login(&self, username: &str, password: &str) -> Result<Response> {
        assert_eq!(username, "alice");
        assert_eq!(password, "fixture-password");
        response(self.0.clone())
    }
}
fn response(value: Value) -> Result<Response> {
    Ok(Response {
        bytes: serde_json::to_vec(&value).unwrap(),
        content_type: "application/json".into(),
    })
}

#[test]
fn login_token_and_expiry_contract_fixtures() {
    let clock = FakeClock::default();
    for (value, seconds, source) in [
        (
            json!({"cloudDragonTokens":{"authToken":"token"}}),
            14400,
            ExpirySource::LocalPolicy,
        ),
        (
            json!({"cloudDragonTokens":{"authToken":"token","expiresIn":180}}),
            180,
            ExpirySource::Server,
        ),
        (
            json!({"cloudDragonTokens":{"authToken":"token","authTokenExpiresIn":240}}),
            240,
            ExpirySource::Server,
        ),
        (
            json!({"cloudDragonTokens":{"authToken":"token"},"authTokenExpiresIn":300}),
            300,
            ExpirySource::Server,
        ),
        (
            json!({"cloudDragonTokens":{"authToken":"token","expiresIn":-1,"authTokenExpiresIn":"30","expireTime":123}}),
            14400,
            ExpirySource::LocalPolicy,
        ),
        (
            json!({"cloudDragonTokens":{"authToken":"token","expiresIn":999999999999999999_i64}}),
            14400,
            ExpirySource::LocalPolicy,
        ),
        (
            json!({"cloudDragonTokens":{"authToken":"token","expiresIn":0,"authTokenExpiresIn":2.5,"expireTime":"yesterday"},"expiresAt":null}),
            14400,
            ExpirySource::LocalPolicy,
        ),
        (
            json!({"cloudDragonTokens":{"authToken":"token","expiresIn":300,"expireTime":(clock.now()+chrono::Duration::seconds(100)).to_rfc3339()},"expiresAt":(clock.now()+chrono::Duration::seconds(200)).to_rfc3339()}),
            100,
            ExpirySource::Server,
        ),
    ] {
        let http = LoginFixture(value);
        let result = W3Gateway(&http)
            .login("alice", &password(), clock.now())
            .unwrap();
        assert_eq!(result.token.expose(), "token");
        assert_eq!((result.expires_at - clock.now()).num_seconds(), seconds);
        assert_eq!(result.expiry_source, source);
    }
    for value in [
        json!(null),
        json!({}),
        json!({"cloudDragonTokens":{"authToken":" "}}),
        json!({"cloudDragonTokens":{"authToken":false}}),
    ] {
        let http = LoginFixture(value);
        let error = W3Gateway(&http)
            .login("alice", &password(), clock.now())
            .unwrap_err();
        assert_eq!(error.code, "MISSING_AUTH_TOKEN");
        assert!(error.hint.contains("skill-bom auth login"));
    }
    for value in [
        json!({"cloudDragonTokens":{"authToken":"unsafe\r\nheader"}}),
        json!({"cloudDragonTokens":{"authToken":"token"},"expiresAt":clock.now().to_rfc3339()}),
    ] {
        assert_eq!(
            W3Gateway(&LoginFixture(value))
                .login("alice", &password(), clock.now())
                .unwrap_err()
                .code,
            "AUTH_REQUIRED"
        );
    }
}

struct AgentFixture {
    first: Value,
    empty: bool,
    always_reject: bool,
    details: Cell<usize>,
    downloads: Cell<usize>,
}
impl Transport for AgentFixture {
    fn get(&self, _: &str, _: Option<&str>) -> Result<Response> {
        panic!("not Bearer")
    }
    fn get_x_auth(&self, url: &str, token: &str) -> Result<Response> {
        assert_eq!(
            url,
            "https://agent.huawei.com/mcpService/external/skills/v1/get?skillId=review"
        );
        let n = self.details.get();
        self.details.set(n + 1);
        if n == 0 || self.always_reject {
            assert_eq!(token, "fixture-token-1");
            if self.empty {
                return Ok(Response {
                    bytes: b" \n".to_vec(),
                    content_type: "text/plain".into(),
                });
            }
            return response(self.first.clone());
        }
        assert_eq!(token, "fixture-token-2");
        response(json!({"success":true,"data":{"skillId":"review","latestVersion":"1.0.0"}}))
    }
    fn post_json(&self, url: &str, body: &Value, token: &str) -> Result<Response> {
        assert_eq!(
            url,
            "https://agent.huawei.com/mcpService/external/skills/v1/download"
        );
        assert_eq!(body, &json!({"skillId":"review","version":"1.0.0"}));
        let n = self.downloads.get();
        self.downloads.set(n + 1);
        if n == 0 {
            assert_eq!(token, "fixture-token-2");
            return response(json!(null));
        }
        assert_eq!(token, "fixture-token-3");
        Ok(Response {
            bytes: common::archive("review", "1.0.0"),
            content_type: "application/zip".into(),
        })
    }
}
fn manifest() -> Manifest {
    Manifest::parse("schema_version=1\n[project]\nname='auth-test'\n[registries.w3]\nkind='agentcenter'\nurl='https://agent.huawei.com'\n[dependencies.review]\nregistry='w3'\npackage='review'\nversion='=1.0.0'\n").unwrap()
}

#[test]
fn agentcenter_details_and_download_post_refresh_and_replay_once() {
    for (first, empty) in [
        (json!(null), false),
        (json!({}), true),
        (json!({"code":40100}), false),
        (json!({"code":"40300"}), false),
        (json!({"success":false,"code":401}), false),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let sessions = Sessions::new(temp.path()).unwrap();
        let secrets = MemorySecrets::default();
        let gateway = Gateway::default();
        let clock = FakeClock::default();
        let service = sessions.service(&secrets, &gateway, &clock);
        service.login("alice", &password()).unwrap();
        let http = AgentFixture {
            first,
            empty,
            always_reject: false,
            details: Cell::new(0),
            downloads: Cell::new(0),
        };
        let m = manifest();
        let d = &m.dependencies["review"];
        let source = m.source(d).unwrap();
        let candidate = agentcenter::candidates_with_auth(&http, &service, &m, &source, d)
            .unwrap()
            .remove(0);
        assert_eq!(http.details.get(), 2);
        let dest = tempfile::tempdir().unwrap();
        let (_, _, evidence) = agentcenter::fetch_with_auth(
            &http,
            &service,
            &m,
            &source,
            &candidate,
            None,
            dest.path(),
        )
        .unwrap();
        assert!(evidence.archive_sha256.is_some());
        assert_eq!(http.downloads.get(), 2);
        assert_eq!(gateway.calls.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn environment_override_is_authoritative_at_adapter_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(temp.path()).unwrap();
    let secrets = MemorySecrets::default();
    let gateway = Gateway::default();
    let clock = FakeClock::default();
    let service = sessions.service(&secrets, &gateway, &clock);
    let mut m = manifest();
    // Read an existing nonempty variable; never mutate the process environment.
    m.registries.get_mut("w3").unwrap().token_env = Some("PATH".into());
    struct Deny(Cell<usize>);
    impl Transport for Deny {
        fn get(&self, _: &str, _: Option<&str>) -> Result<Response> {
            unreachable!()
        }
        fn get_x_auth(&self, _: &str, token: &str) -> Result<Response> {
            self.0.set(self.0.get() + 1);
            assert_eq!(token, std::env::var("PATH").unwrap());
            Err(required("rejected"))
        }
    }
    let http = Deny(Cell::new(0));
    let d = &m.dependencies["review"];
    assert_eq!(
        agentcenter::candidates_with_auth(&http, &service, &m, &m.source(d).unwrap(), d)
            .unwrap_err()
            .code,
        "AUTH_REQUIRED"
    );
    assert_eq!(http.0.get(), 1);
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
    assert_eq!(secrets.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn cache_hits_and_offline_misses_never_acquire_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let m = manifest();
    let d = &m.dependencies["review"];
    let source = m.source(d).unwrap();
    let mut provider = Provider::new(&m, temp.path().join("cache"), false, true).unwrap();
    provider.credentials = Box::new(NoCredentials);
    let mut package = common::package(&provider.store, "review", "1.0.0");
    package.source = source.clone();
    package.acquisition = d.clone();
    package.candidate.revision = None;
    provider.ensure(&package).unwrap();
    provider
        .materialize(&source, &package.candidate, d, Some(&package))
        .unwrap();
    provider.http.offline = true;
    provider.ensure(&package).unwrap();
    assert_eq!(
        provider.candidates(&source, d).unwrap_err().code,
        "OFFLINE_MISS"
    );
    let mut absent = Provider::new(&m, temp.path().join("missing"), true, true).unwrap();
    absent.credentials = Box::new(NoCredentials);
    assert!(absent.ensure(&package).is_err());
    assert_eq!(
        absent
            .materialize(&source, &package.candidate, d, None)
            .unwrap_err()
            .code,
        "OFFLINE_MISS"
    );
}

#[test]
fn http_400_401_403_are_auth_signals_without_exposing_bodies() {
    for status in [400, 401, 403] {
        let server = common::http::Server::new(move |_, _| {
            common::http::Reply::status(status, "fixture-password-token")
        });
        let http = skill_bom::net::Http::new(false).unwrap();
        for error in [
            http.get_x_auth(&server.url, "token").err().unwrap(),
            http.post_json(&server.url, &json!({}), "token")
                .err()
                .unwrap(),
        ] {
            assert_eq!(error.code, "AUTH_REQUIRED");
            assert!(!format!("{error:?}").contains("fixture-password-token"));
        }
    }
}

#[test]
fn gateway_protocol_and_transport_failures_have_login_guidance() {
    struct Broken(bool);
    impl Transport for Broken {
        fn get(&self, _: &str, _: Option<&str>) -> Result<Response> {
            unreachable!()
        }
        fn secure_login(&self, _: &str, _: &str) -> Result<Response> {
            if self.0 {
                Err(Error::new("NETWORK", "offline", 2))
            } else {
                Ok(Response {
                    bytes: b"invalid secret body".to_vec(),
                    content_type: "text/plain".into(),
                })
            }
        }
    }
    for transport in [Broken(true), Broken(false)] {
        let error = W3Gateway(&transport)
            .login("alice", &password(), FakeClock::default().now())
            .unwrap_err();
        assert!(error.hint.contains("skill-bom auth login"));
        assert!(!format!("{error:?}").contains("invalid secret body"));
    }
}

#[test]
fn login_transport_errors_use_authentication_phase_and_runtime_exit_code() {
    struct MissingEndpoint;
    impl Transport for MissingEndpoint {
        fn get(&self, _: &str, _: Option<&str>) -> Result<Response> {
            unreachable!()
        }
        fn secure_login(&self, _: &str, _: &str) -> Result<Response> {
            Err(Error::new("SOURCE_VERSION_UNAVAILABLE", "HTTP 404", 1).phase("download"))
        }
    }
    let error = W3Gateway(&MissingEndpoint)
        .login("alice", &password(), FakeClock::default().now())
        .unwrap_err();
    assert_eq!(error.exit_code, 2);
    assert_eq!(&*error.phase, "authentication");
    assert_eq!(error.code, "SOURCE_VERSION_UNAVAILABLE");
    assert!(error.hint.contains("skill-bom auth login"));
}
