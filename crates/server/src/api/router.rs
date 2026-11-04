//! Maps each request op to a typed handler function.

use crate::{
    AsyncMutex,
    account::user::User,
    api::transport::READ_TIMEOUT,
    app::App,
    protocol::{
        ApiError, Call, ClientInfo, DeviceKind, Duplex, Feature, Op, Stream,
        wire::{Frame, Rx, Tx, framed},
    },
    push::Pusher,
};
use anyhow::{Context, Result, bail};
use futures::future::BoxFuture;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, future::Future, marker::PhantomData, sync::Arc};
use tokio::time::timeout;

/// What every handler gets besides its request.
#[derive(Clone)]
pub struct Cx {
    pub app: Arc<App>,
    pub user: Arc<User>,
    /// What kind of device the caller's key belongs to.
    pub device: DeviceKind,
    /// Whether the caller's key made the account.
    pub primary: bool,
    pub connection: Connection,
    pub pusher: Arc<Pusher>,
    /// What the app on this connection said it reads.
    pub client: Arc<ClientInfo>,
}

impl Cx {
    /// Whether the client reads `feature`; check before sending a shape newer than `crates/proto/tests/wire`.
    pub fn supports(&self, feature: Feature) -> bool {
        self.client.supports(feature)
    }
}

type Handler = Box<dyn Fn(Cx, Frame, Tx, Rx) -> BoxFuture<'static, Result<()>> + Send + Sync>;

#[derive(Default)]
pub struct Router {
    routes: HashMap<Op, Handler>,
    duplicates: Vec<Op>,
}

impl Router {
    /// Routes `R` to `handler`, which answers with one reply.
    pub fn call<R, F, Fut>(self, handler: F) -> Self
    where
        R: Call,
        F: Fn(Cx, R) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R::Reply, ApiError>> + Send + 'static,
    {
        self.route(R::OP, move |cx, frame, mut tx, _rx| {
            let reply = frame.decode::<R>().map(|request| handler(cx, request));
            Box::pin(async move {
                let reply = match reply {
                    Ok(reply) => reply.await,
                    Err(e) => Err(e),
                };
                match reply {
                    Ok(reply) => tx.ok(&reply).await?,
                    Err(e) => tx.err(&e).await?,
                }
                tx.finish()
            })
        })
    }

    /// Routes `R` to `handler`, which streams replies into a `Sink`.
    pub fn stream<R, F, Fut>(self, handler: F) -> Self
    where
        R: Stream,
        F: Fn(Cx, R, Sink<R::Reply>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), ApiError>> + Send + 'static,
    {
        self.route(R::OP, move |cx, frame, tx, _rx| {
            let tx = Arc::new(AsyncMutex::new(tx));
            let sink = Sink {
                tx: tx.clone(),
                _reply: PhantomData,
            };
            let done = frame
                .decode::<R>()
                .map(|request| handler(cx, request, sink));
            Box::pin(finish_stream(done, tx))
        })
    }

    /// Routes `R` to `handler`, which reads an `Uplink` and replies into a `Sink`.
    pub fn duplex<R, F, Fut>(self, handler: F) -> Self
    where
        R: Duplex,
        F: Fn(Cx, R, Uplink<R::Up>, Sink<R::Reply>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), ApiError>> + Send + 'static,
    {
        self.route(R::OP, move |cx, frame, tx, rx| {
            let tx = Arc::new(AsyncMutex::new(tx));
            let sink = Sink {
                tx: tx.clone(),
                _reply: PhantomData,
            };
            let uplink = Uplink {
                rx,
                _up: PhantomData,
            };
            let done = frame
                .decode::<R>()
                .map(|request| handler(cx, request, uplink, sink));
            Box::pin(finish_stream(done, tx))
        })
    }

    fn route(
        mut self,
        op: Op,
        handler: impl Fn(Cx, Frame, Tx, Rx) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    ) -> Self {
        if self.routes.insert(op, Box::new(handler)).is_some() {
            self.duplicates.push(op);
        }
        self
    }

    /// Fails if an op has no handler or more than one.
    pub fn check(&self) -> Result<()> {
        let missing: Vec<_> = Op::ALL
            .iter()
            .filter(|op| !self.routes.contains_key(op))
            .collect();
        if !missing.is_empty() || !self.duplicates.is_empty() {
            bail!(
                "unrouted ops {missing:?}, routed twice {:?}",
                self.duplicates
            );
        }
        Ok(())
    }

    /// Reads the request on one bi stream and runs its handler.
    pub async fn serve(&self, cx: Cx, send: SendStream, recv: RecvStream) -> Result<()> {
        let (tx, mut rx) = framed(send, recv);
        let frame = read_request(&mut rx).await?;
        self.dispatch(cx, frame, tx, rx).await
    }

    /// Runs the handler for a request already read; only `Duplex` reads `rx`.
    pub async fn dispatch(&self, cx: Cx, frame: Frame, mut tx: Tx, rx: Rx) -> Result<()> {
        let handler = frame.op().and_then(|op| {
            self.routes
                .get(&op)
                .ok_or_else(|| ApiError::bad_request(format!("{op:?} is not served")))
        });
        match handler {
            Ok(handler) => handler(cx, frame, tx, rx).await,
            Err(e) => {
                tx.err(&e).await?;
                tx.finish()
            }
        }
    }
}

/// Sends a stream handler's error, if any, and ends the stream.
async fn finish_stream(
    done: Result<impl Future<Output = Result<(), ApiError>>, ApiError>,
    tx: Arc<AsyncMutex<Tx>>,
) -> Result<()> {
    let done = match done {
        Ok(done) => done.await,
        Err(e) => Err(e),
    };
    let mut tx = tx.lock().await;
    if let Err(e) = done {
        tx.err(&e).await?;
    }
    tx.finish()
}

/// What the client sends after a `Duplex` request.
pub struct Uplink<T> {
    rx: Rx,
    _up: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Uplink<T> {
    /// The client's next frame, `None` once it finished; a bad frame is the client's fault.
    pub async fn next(&mut self) -> Result<Option<T>, ApiError> {
        match self.rx.frame().await {
            Ok(Some(frame)) => frame.decode().map(Some),
            Ok(None) => Ok(None),
            Err(e) => {
                log::debug!("uplink ended: {e:#}");
                Ok(None)
            }
        }
    }
}

/// The one request frame a stream opens with.
pub async fn read_request(rx: &mut Rx) -> Result<Frame> {
    timeout(READ_TIMEOUT, rx.frame())
        .await
        .context("request timed out")??
        .context("empty stream")
}

/// Where a stream handler sends its replies.
pub struct Sink<T> {
    tx: Arc<AsyncMutex<Tx>>,
    _reply: PhantomData<fn(T)>,
}

/// The client went away; the stream handler should stop.
#[derive(Debug)]
pub struct Closed;

impl From<Closed> for ApiError {
    fn from(_: Closed) -> Self {
        Self::Internal {}
    }
}

impl<T: Serialize> Sink<T> {
    pub async fn send(&self, reply: &T) -> Result<(), Closed> {
        self.tx.lock().await.ok(reply).await.map_err(|_| Closed)
    }
}
