//! A git host for tests: answers each path from a handler and records every request.

use std::sync::{Arc, Mutex, PoisonError};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub struct Reply {
    pub status: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
}

impl Reply {
    pub fn ok(body: &str) -> Self {
        Self {
            status: "200 OK",
            headers: Vec::new(),
            body: body.to_owned(),
        }
    }

    pub fn status(status: &'static str) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: String::new(),
        }
    }

    pub fn with(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_owned()));
        self
    }
}

/// A request as the host read it: the path and the headers, lowercased.
#[derive(Debug, Clone)]
pub struct Seen {
    pub path: String,
    pub head: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<String> {
        let prefix = format!("{}: ", name.to_ascii_lowercase());
        self.head
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .map(|value| value.trim().to_owned())
    }
}

type Handler = dyn Fn(&Seen) -> Reply + Send + Sync;

/// The running host: its base URL and what it was sent.
pub struct TestHost {
    pub base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl TestHost {
    pub async fn start(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Self {
        let handler: Arc<Handler> = Arc::new(handler);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buf).await.expect("the request reads");
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                let text = String::from_utf8_lossy(&data).to_ascii_lowercase();
                let path = String::from_utf8_lossy(&data)
                    .split(' ')
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let request = Seen { path, head: text };
                let reply = handler(&request);
                kept.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(request);
                let mut out = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    reply.status,
                    reply.body.len()
                );
                for (name, value) in &reply.headers {
                    out.push_str(&format!("{name}: {value}\r\n"));
                }
                out.push_str("\r\n");
                out.push_str(&reply.body);
                stream
                    .write_all(out.as_bytes())
                    .await
                    .expect("the reply writes");
            }
        });
        Self { base, seen }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
