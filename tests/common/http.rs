use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub headers: Vec<(String, String)>,
}
impl Reply {
    pub fn bytes(content_type: &str, bytes: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: content_type.into(),
            bytes,
            headers: vec![],
        }
    }
    pub fn json(value: serde_json::Value) -> Self {
        Self::bytes("application/json", serde_json::to_vec(&value).unwrap())
    }
    pub fn status(status: u16, text: &str) -> Self {
        Self {
            status,
            ..Self::bytes("text/plain", text.as_bytes().into())
        }
    }
}
pub struct Server {
    pub url: String,
    stop: Arc<AtomicBool>,
    pub requests: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    pub fn new(handler: impl Fn(&str, &str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = vec![];
                let mut buffer = [0; 1024];
                loop {
                    let n = socket.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.windows(4).any(|b| b == b"\r\n\r\n") || bytes.len() > 16384 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&bytes);
                let path = text.split_whitespace().nth(1).unwrap_or("/");
                count.fetch_add(1, Ordering::SeqCst);
                let reply = handler(path, &text);
                let mut header = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    reply.status,
                    reply.content_type,
                    reply.bytes.len()
                );
                for (k, v) in reply.headers {
                    header.push_str(&format!("{k}: {v}\r\n"));
                }
                header.push_str("\r\n");
                let _ = socket.write_all(header.as_bytes());
                let _ = socket.write_all(&reply.bytes);
            }
        });
        Self {
            url,
            stop,
            requests,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
