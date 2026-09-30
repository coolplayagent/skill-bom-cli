//! Local HTTPS fixture with ephemeral certificates and bounded sockets; tests only.
use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

pub struct Server {
    pub url: String,
    pub certificate: Vec<u8>,
    pub connections: Arc<AtomicUsize>,
    pub requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub fn new(handler: impl Fn(&str) -> Vec<u8> + Send + 'static) -> Self {
        let generated = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let certificate = generated.cert.der().to_vec();
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![generated.cert.der().clone()],
                PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der()).into(),
            )
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let (flag, connected, requested) = (stop.clone(), connections.clone(), requests.clone());
        let thread = std::thread::spawn(move || {
            let config = Arc::new(config);
            while !flag.load(Ordering::SeqCst) {
                let (socket, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("TLS fixture accept: {error}"),
                };
                connected.fetch_add(1, Ordering::SeqCst);
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let session = ServerConnection::new(config.clone()).unwrap();
                let mut stream = StreamOwned::new(session, socket);
                let Some(request) = read_request(&mut stream) else {
                    continue;
                };
                requested.fetch_add(1, Ordering::SeqCst);
                let reply = handler(&request);
                let _ = stream.write_all(&reply);
                let _ = stream.flush();
            }
        });
        Self {
            url,
            certificate,
            connections,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}

fn read_request(stream: &mut StreamOwned<ServerConnection, TcpStream>) -> Option<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let size = stream.read(&mut buffer).ok()?;
        if size == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..size]);
        assert!(bytes.len() <= 16384, "TLS fixture request exceeds 16 KiB");
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")?
                        .trim()
                        .parse::<usize>()
                        .ok()
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return Some(String::from_utf8(bytes).unwrap());
            }
        }
    }
}

pub fn response(content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ).into_bytes();
    reply.extend_from_slice(body);
    reply
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
