use crate::{
    Call, Notice, Rpc, Stream,
    wire::{ALPN, PUSH_ALPN, Rx, framed},
};
use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey,
    endpoint::{Connection, ConnectionError, NetReportConfig, PortmapperConfig, VarInt, presets},
};
use serde::de::DeserializeOwned;
use std::{
    marker::PhantomData,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a connection with no requests stays open. Each open connection
/// sends a QUIC keep-alive every 5s, which keeps a phone's radio awake.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Close code for a connection closed for being idle.
pub const IDLE_CLOSE: u32 = 0x1d1e;
/// Close code the server gives a key the account removed.
pub const CLOSE_REVOKED: u32 = 403;

/// Called with each notice the server sends. Returns false if the notice
/// was not handled, so the server keeps it and sends it again later.
pub type OnNotice = Arc<dyn Fn(Notice) -> bool + Send + Sync>;

/// This device's iroh endpoint, bound once and kept for the life of the
/// process. Its home relay stays up, so the server can dial it with notices
/// while no connection is open.
pub struct Node {
    endpoint: Endpoint,
    server: EndpointAddr,
    on_notice: OnNotice,
    idle: Duration,
    /// Dropped with the node, which stops the accept loop and closes the
    /// endpoint, so a replaced node stops pinging its relay.
    _stop: oneshot::Sender<()>,
}

impl Node {
    /// Binds as `identity`. The server knows the user by its public key, and
    /// a key it has not seen before starts a new account. Only `server` may
    /// push notices.
    pub async fn bind(
        identity: SecretKey,
        server: impl Into<EndpointAddr>,
        on_notice: OnNotice,
    ) -> Result<Self> {
        let server = server.into();
        // HTTPS probes stay on: without them a network that blocks QUIC never
        // finds a home relay, and the server could not reach this node.
        let mut net_report = NetReportConfig::default();
        net_report.captive_portal_check = false;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(identity)
            .alpns(vec![PUSH_ALPN.to_vec()])
            // UPnP and PCP never work on cellular, and each probe wakes the radio.
            .portmapper_config(PortmapperConfig::Disabled)
            .net_report_config(net_report)
            .bind()
            .await?;
        let (stop, stopped) = oneshot::channel();
        tokio::spawn(accept_pushes(
            endpoint.clone(),
            server.id,
            on_notice.clone(),
            stopped,
        ));
        Ok(Self {
            endpoint,
            server,
            on_notice,
            idle: IDLE_TIMEOUT,
            _stop: stop,
        })
    }

    /// Sets how long an idle connection stays open; tests shorten it.
    pub fn with_idle_timeout(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Opens a connection to the server. It closes itself once idle for
    /// `IDLE_TIMEOUT`, and `Client::is_closed` then says to make a new one.
    pub async fn connect(&self) -> Result<Client> {
        let connect = self.endpoint.connect(self.server.clone(), ALPN);
        let connection = timeout(CONNECT_TIMEOUT, connect)
            .await
            .context("connect timed out")??;
        let activity = Arc::new(Activity::new());
        tokio::spawn(close_when_idle(
            connection.clone(),
            activity.clone(),
            self.idle,
        ));
        tokio::spawn(serve_notices(connection.clone(), self.on_notice.clone()));
        Ok(Client {
            connection,
            activity,
        })
    }

    /// Tells iroh the network changed. Android gives iroh no way to watch
    /// for this itself, so the app calls it from its network callback.
    pub async fn network_change(&self) {
        self.endpoint.network_change().await;
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

/// One iroh connection to the server. Requests run concurrently, one stream each.
/// Errors the server sends back are `ApiError`, recoverable by downcast.
pub struct Client {
    connection: Connection,
    activity: Arc<Activity>,
}

impl Client {
    /// True once the connection is gone and a new one is needed.
    pub fn is_closed(&self) -> bool {
        self.connection.close_reason().is_some()
    }

    /// True once the server closed the connection because the account
    /// removed this key.
    pub fn is_revoked(&self) -> bool {
        matches!(
            self.connection.close_reason(),
            Some(ConnectionError::ApplicationClosed(close))
                if close.error_code == VarInt::from_u32(CLOSE_REVOKED)
        )
    }

    /// Sends a request and returns its reply.
    pub async fn call<R: Call>(&self, request: &R) -> Result<R::Reply> {
        let mut replies = self.open(request).await?;
        replies.next().await?.context("stream closed without reply")
    }

    /// Sends a request and returns its replies as they arrive. The
    /// connection stays open while the returned `Replies` lives.
    pub async fn stream<R: Stream>(&self, request: &R) -> Result<Replies<R::Reply>> {
        self.open(request).await
    }

    async fn open<R: Rpc, T>(&self, request: &R) -> Result<Replies<T>> {
        let busy = Busy::new(self.activity.clone());
        let (send, recv) = self.connection.open_bi().await?;
        let (mut tx, rx) = framed(send, recv);
        tx.request(request).await?;
        tx.finish()?;
        Ok(Replies {
            rx,
            _busy: busy,
            _reply: PhantomData,
        })
    }
}

pub struct Replies<T> {
    rx: Rx,
    _busy: Busy,
    _reply: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Replies<T> {
    /// Next reply, `None` at end of stream.
    pub async fn next(&mut self) -> Result<Option<T>> {
        self.rx.reply().await
    }
}

/// Requests in flight on a connection and when the last one ended.
struct Activity {
    open: AtomicUsize,
    last: Mutex<Instant>,
}

impl Activity {
    fn new() -> Self {
        Self {
            open: AtomicUsize::new(0),
            last: Mutex::new(Instant::now()),
        }
    }

    fn touch(&self) {
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
    }

    /// How long until the connection has been idle for `idle`, zero if it
    /// already has. A connection with requests in flight is never idle.
    fn until_idle(&self, idle: Duration) -> Duration {
        if self.open.load(Ordering::Acquire) > 0 {
            return idle;
        }
        let last = *self.last.lock().unwrap_or_else(PoisonError::into_inner);
        idle.saturating_sub(last.elapsed())
    }
}

/// Marks a request in flight until dropped.
struct Busy(Arc<Activity>);

impl Busy {
    fn new(activity: Arc<Activity>) -> Self {
        activity.open.fetch_add(1, Ordering::AcqRel);
        activity.touch();
        Self(activity)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.touch();
        self.0.open.fetch_sub(1, Ordering::AcqRel);
    }
}

async fn close_when_idle(connection: Connection, activity: Arc<Activity>, idle: Duration) {
    loop {
        let wait = activity.until_idle(idle);
        if wait.is_zero() {
            connection.close(VarInt::from_u32(IDLE_CLOSE), b"idle");
            return;
        }
        tokio::select! {
            _ = connection.closed() => return,
            _ = sleep(wait) => {}
        }
    }
}

/// Accepts the server's connections for pushing notices. Any other peer is
/// turned away. Closes the endpoint once `stopped` fires.
async fn accept_pushes(
    endpoint: Endpoint,
    server: EndpointId,
    on_notice: OnNotice,
    mut stopped: oneshot::Receiver<()>,
) {
    loop {
        let incoming = tokio::select! {
            incoming = endpoint.accept() => incoming,
            _ = &mut stopped => break,
        };
        let Some(incoming) = incoming else { return };
        let on_notice = on_notice.clone();
        tokio::spawn(async move {
            let connection = match incoming.await {
                Ok(connection) => connection,
                Err(e) => {
                    log::debug!("push connection failed: {e:#}");
                    return;
                }
            };
            if connection.remote_id() != server {
                log::warn!("refused a push from {}", connection.remote_id());
                connection.close(VarInt::from_u32(1), b"not the server");
                return;
            }
            serve_notices(connection, on_notice).await;
        });
    }
    endpoint.close().await;
}

/// Answers the notice streams the server opens on `connection`. They do not
/// count as activity, so the server cannot keep a connection open.
async fn serve_notices(connection: Connection, on_notice: OnNotice) {
    while let Ok((send, recv)) = connection.accept_bi().await {
        let on_notice = on_notice.clone();
        tokio::spawn(async move {
            let (mut tx, mut rx) = framed(send, recv);
            let notice = match rx.reply::<Notice>().await {
                Ok(Some(notice)) => notice,
                Ok(None) => return,
                Err(e) => {
                    log::debug!("bad notice stream: {e:#}");
                    return;
                }
            };
            // An unhandled notice gets no ack, so the server sends it again.
            if !on_notice(notice) {
                return;
            }
            let acked = async {
                tx.ok(&()).await?;
                tx.finish()
            };
            if let Err(e) = acked.await {
                log::debug!("notice ack failed: {e:#}");
            }
        });
    }
}
