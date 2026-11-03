use super::*;
use beep_proto::SecretText;
use std::{
    collections::BTreeSet,
    sync::{Mutex, PoisonError},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{mpsc, oneshot},
};

/// A repository's webhooks, kept in memory.
#[derive(Clone)]
struct FakeHost {
    state: Arc<Mutex<HostState>>,
}

struct HostState {
    admin: bool,
    hooks: BTreeSet<u64>,
    next: u64,
    /// Where the webhook made posts to.
    url: Option<String>,
}

impl FakeHost {
    fn new(admin: bool, hooks: &[u64]) -> Self {
        Self {
            state: Arc::new(Mutex::new(HostState {
                admin,
                hooks: hooks.iter().copied().collect(),
                next: 100,
                url: None,
            })),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, HostState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn hooks(&self) -> Vec<u64> {
        self.state().hooks.iter().copied().collect()
    }
}

impl HostApi for FakeHost {
    async fn is_admin(&self) -> Result<bool, Refusal> {
        Ok(self.state().admin)
    }

    async fn make(&self, url: &str, _secret: &SecretString) -> Result<Made, Refusal> {
        let mut state = self.state();
        let id = state.next;
        state.next += 1;
        state.hooks.insert(id);
        state.url = Some(url.to_owned());
        Ok(Made {
            hook: Some(id),
            forward: None,
        })
    }

    async fn delete(&self, id: u64) -> Result<(), Refusal> {
        self.state().hooks.remove(&id);
        Ok(())
    }
}

/// The server's end: what the node replied, and a switch to close it.
struct FakeServer {
    replies: mpsc::UnboundedSender<WatchReply>,
    close: oneshot::Receiver<()>,
}

impl Server for FakeServer {
    async fn reply(&mut self, reply: &WatchReply) -> Result<()> {
        self.replies.send(reply.clone())?;
        Ok(())
    }

    async fn closed(&mut self) {
        if (&mut self.close).await.is_err() {
            pending::<()>().await;
        }
    }
}

const SECRET: &str = "per-hook-secret";

fn watch(stale: &[u64]) -> Watch {
    Watch {
        host: WatchHost::GitHub { token_var: None },
        repo: "acme/widget".into(),
        secret: SecretText::new(SECRET),
        stale: stale.to_vec(),
    }
}

struct Running {
    replies: mpsc::UnboundedReceiver<WatchReply>,
    close: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<()>>,
    inner: Arc<Inner>,
}

fn start(host: FakeHost, stale: &[u64]) -> Running {
    let inner = Webhooks::new(None).expect("no address is fine").inner;
    let (tx, replies) = mpsc::unbounded_channel();
    let (close, closed) = oneshot::channel();
    let watch = watch(stale);
    let task = {
        let inner = inner.clone();
        tokio::spawn(async move {
            let mut server = FakeServer {
                replies: tx,
                close: closed,
            };
            let secret = SecretString::from(SECRET);
            run(&inner, host, Reach::Loopback, &watch, secret, &mut server).await
        })
    };
    Running {
        replies,
        close,
        task,
        inner,
    }
}

async fn post_push(url: &str) -> String {
    let body = r#"{"ref":"refs/heads/main"}"#;
    let signature = format!("sha256={}", signature::sign(SECRET, body.as_bytes()));
    let rest = url.strip_prefix("http://").expect("an http URL");
    let (addr, path) = rest.split_at(rest.find('/').expect("a path"));
    let mut stream = TcpStream::connect(addr).await.expect("connects");
    let request = format!(
        "POST {path} HTTP/1.1\r\nContent-Length: {}\r\nX-GitHub-Event: push\r\n\
         X-Hub-Signature-256: {signature}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.expect("sends");
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.expect("reads");
    reply.lines().next().unwrap_or_default().to_owned()
}

#[tokio::test]
async fn a_watch_removes_only_the_webhooks_beep_made() {
    // 1 and 2 are the user's own; 3 is one Beep made before and lost.
    let host = FakeHost::new(true, &[1, 2, 3]);
    let mut running = start(host.clone(), &[3]);

    assert_eq!(
        running.replies.recv().await,
        Some(WatchReply::Removed { hook: 3 })
    );
    assert_eq!(
        running.replies.recv().await,
        Some(WatchReply::Live { hook: Some(100) })
    );
    assert_eq!(host.hooks(), [1, 2, 100]);

    let url = host.state().url.clone().expect("the webhook has a URL");
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert_eq!(post_push(&url).await, "HTTP/1.1 204 No Content");
    assert_eq!(
        running.replies.recv().await,
        Some(WatchReply::Event {
            event: "push".into()
        })
    );

    running.close.send(()).expect("the watch is running");
    assert_eq!(
        running.replies.recv().await,
        Some(WatchReply::Removed { hook: 100 })
    );
    running.task.await.expect("joins").expect("ends cleanly");
    assert_eq!(host.hooks(), [1, 2], "the user's own webhooks stay");
}

#[tokio::test]
async fn a_watch_cut_off_still_removes_its_webhook() {
    let host = FakeHost::new(true, &[1]);
    let mut running = start(host.clone(), &[]);
    assert_eq!(
        running.replies.recv().await,
        Some(WatchReply::Live { hook: Some(100) })
    );
    // The connection to the server drops: the session is dropped mid-watch.
    running.task.abort();
    let _ = running.task.await;
    running.inner.cleanups.close();
    running.inner.cleanups.wait().await;
    assert_eq!(host.hooks(), [1]);
}

#[tokio::test]
async fn a_repository_the_user_does_not_administer_falls_back_to_polling() {
    let host = FakeHost::new(false, &[1, 3]);
    let mut running = start(host.clone(), &[3]);
    assert_eq!(
        running.replies.recv().await,
        Some(WatchReply::Polling {
            why: PollReason::NotAdmin,
            detail: String::new()
        })
    );
    running.task.await.expect("joins").expect("ends cleanly");
    assert_eq!(host.hooks(), [1, 3], "nothing touched");
    assert!(host.state().url.is_none());
}

#[test]
fn an_unspecified_address_is_refused() {
    assert!(Webhooks::new(Some("0.0.0.0:7480".parse().expect("an address"))).is_err());
    assert!(Webhooks::new(Some("[::]:7480".parse().expect("an address"))).is_err());
    assert!(Webhooks::new(Some("[fd00::1]:7480".parse().expect("an address"))).is_ok());
}
