use super::*;
#[path = "tls_fixture.rs"]
mod fixture;
use fixture::{Server, response};
use std::sync::atomic::Ordering;

#[test]
fn self_signed_tls_policy_covers_login_detail_and_download_only() {
    let server = Server::new(|request| {
        if request.contains("\"password\"") {
            assert!(request.starts_with("POST "));
            assert!(request.contains("fixture-password"));
            assert!(!request.to_ascii_lowercase().contains("x-auth-token:"));
        } else {
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("x-auth-token: fixture-token")
            );
        }
        response("application/json", b"{}")
    });
    let compatible = Http::with_tls_policy(false, false).unwrap();
    compatible
        .login_at(&server.url, "alice", "fixture-password")
        .unwrap();
    compatible.get_x_auth(&server.url, "fixture-token").unwrap();
    compatible
        .post_json(
            &server.url,
            &serde_json::json!({"version":"1.0.0"}),
            "fixture-token",
        )
        .unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 3);

    let strict = Http::with_tls_policy(false, true).unwrap();
    for result in [
        strict.login_at(&server.url, "alice", "fixture-password"),
        strict.get_x_auth(&server.url, "fixture-token"),
        strict.post_json(&server.url, &serde_json::json!({}), "fixture-token"),
        compatible.get(&server.url, Some("fixture-token")),
        strict.get(&server.url, None),
    ] {
        let error = result.err().unwrap();
        assert_eq!(error.code, "NETWORK_TLS");
        assert_eq!(
            error.message.as_ref(),
            "TLS certificate verification failed"
        );
        assert!(error.hint.contains("certificate chain"));
        let rendered = serde_json::to_string(&error).unwrap();
        for secret in ["fixture-password", "fixture-token", &server.url] {
            assert!(!rendered.contains(secret));
        }
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 3);
    // Certificate failures are terminal and never downgraded or retried.
    assert_eq!(server.connections.load(Ordering::SeqCst), 8);
}

fn trusted(server: &Server, timeout: Duration) -> Http {
    let client = Client::builder()
        .add_root_certificate(reqwest::Certificate::from_der(&server.certificate).unwrap())
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .unwrap();
    Http {
        client: client.clone(),
        loopback: client,
        agentcenter: None,
        offline: false,
    }
}

#[test]
fn verified_tls_accepts_trusted_certificate_and_reports_real_timeouts() {
    let server = Server::new(|_| response("application/json", b"{}"));
    let http = trusted(&server, Duration::from_secs(3));
    http.login_at(&server.url, "alice", "fixture-password")
        .unwrap();
    http.get_x_auth(&server.url, "fixture-token").unwrap();
    http.post_json(&server.url, &serde_json::json!({}), "fixture-token")
        .unwrap();
    http.get(&server.url, None).unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 4);

    // The chain is trusted, but this hostname is absent from the certificate.
    let wrong_name = server.url.replace("127.0.0.1", "localhost");
    let error = http.get_x_auth(&wrong_name, "fixture-token").err().unwrap();
    assert_eq!(error.code, "NETWORK_TLS");
    assert_eq!(server.requests.load(Ordering::SeqCst), 4);
    Http::with_tls_policy(false, false)
        .unwrap()
        .get_x_auth(&wrong_name, "fixture-token")
        .unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 5);

    let slow = Server::new(|_| {
        std::thread::sleep(Duration::from_millis(200));
        response("application/json", b"{}")
    });
    let error = trusted(&slow, Duration::from_millis(30))
        .get_x_auth(&slow.url, "fixture-token")
        .err()
        .unwrap();
    assert_eq!(error.code, "NETWORK_TIMEOUT");
    assert!(!format!("{error:?}").contains("fixture-token"));
}

#[test]
fn tls_compatibility_does_not_allow_redirects_or_offline_requests() {
    let destination = Server::new(|_| panic!("credentials followed a redirect"));
    let location = destination.url.clone();
    let redirect = Server::new(move |_| {
        format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    ).into_bytes()
    });
    for offline in [false, true] {
        let http = Http::with_tls_policy(offline, false).unwrap();
        for result in [
            http.login_at(&redirect.url, "alice", "fixture-password"),
            http.get_x_auth(&redirect.url, "fixture-token"),
            http.post_json(&redirect.url, &serde_json::json!({}), "fixture-token"),
        ] {
            assert_eq!(
                result.err().unwrap().code,
                if offline { "OFFLINE_MISS" } else { "PROTOCOL" }
            );
        }
    }
    assert_eq!(redirect.requests.load(Ordering::SeqCst), 3);
    assert_eq!(destination.connections.load(Ordering::SeqCst), 0);
}
