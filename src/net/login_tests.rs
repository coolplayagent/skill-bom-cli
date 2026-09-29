use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn fixture(
    status: u16,
    body: &str,
    headers: &str,
    inspect: impl Fn(&str) + Send + 'static,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let body = body.to_string();
    let headers = headers.to_string();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0 && bytes.len() < 16384);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                let text = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let length = text
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        inspect(std::str::from_utf8(&bytes).unwrap());
        write!(socket, "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    (url, thread)
}

#[test]
fn secure_login_wire_headers_body_and_no_token() {
    assert_eq!(
        W3_LOGIN_URL,
        "https://rnd-idea-api.huawei.com/ideaclientservice/login/v4/secureLogin"
    );
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let (url, thread) = fixture(200, "{}", "", move |request| {
        observed.fetch_add(1, Ordering::SeqCst);
        assert!(request.starts_with("POST / HTTP/1.1"));
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        let headers = headers.to_ascii_lowercase();
        assert!(headers.contains("content-type: application/json"));
        assert_eq!(
            headers
                .lines()
                .filter(|line| line.starts_with("content-type:"))
                .count(),
            1
        );
        assert!(headers.contains("accept: application/json, text/javascript, */*; q=0.01"));
        assert!(headers.contains("accept-language: zh-cn,zh"));
        assert!(headers.contains("user-agent: mozilla/5.0 (windows nt 10.0; win64; x64) applewebkit/537.36 (khtml, like gecko) chrome/120.0.0.0 safari/537.36"));
        assert!(!headers.contains("authorization:") && !headers.contains("x-auth-token:"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap(),
            serde_json::json!({"user":"alice","password":"fixture-password","requireUserInfo":"true"})
        );
    });
    Http::new(false)
        .unwrap()
        .login_at(&url, "alice", "fixture-password")
        .unwrap();
    thread.join().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn login_refuses_redirects_and_sanitizes_rejection_bodies() {
    for status in [301, 302, 307, 308, 400, 401, 403, 404, 204] {
        let (url, thread) = fixture(
            status,
            "echoed-password-token",
            "Location: http://127.0.0.1:1/leak\r\n",
            |_| {},
        );
        let error = Http::new(false)
            .unwrap()
            .login_at(&url, "alice", "echoed-password-token")
            .err()
            .unwrap();
        thread.join().unwrap();
        assert!(!format!("{error:?}").contains("echoed-password-token"));
        assert_eq!(
            error.code,
            if matches!(status, 400 | 401 | 403) {
                "AUTH_REQUIRED"
            } else if status == 404 {
                "SOURCE_VERSION_UNAVAILABLE"
            } else {
                "PROTOCOL"
            }
        );
    }
    assert_eq!(
        Http::new(true)
            .unwrap()
            .secure_login("alice", "secret")
            .err()
            .unwrap()
            .code,
        "OFFLINE_MISS"
    );
}
