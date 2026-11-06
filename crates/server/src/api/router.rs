//! Maps each request op to a typed handler function.

use crate::{
    account::user::User,
    app::App,
    protocol::{
        ApiError, Call, DeviceKind, Duplex, Op, Platform, Stream,
        http::{self, ERR, OK},
    },
};
use anyhow::{Result, bail};
use bytes::Bytes;
use futures::{
    StreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use iroh::EndpointId;
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, future::Future, io, marker::PhantomData, sync::Arc};
use tokio::sync::mpsc;
use tokio_util::{
    codec::{FramedRead, LengthDelimitedCodec},
    io::StreamReader,
};

/// Frames waiting for a slow client.
const FRAMES_QUEUE: usize = 16;

/// What every handler gets besides its request.
#[derive(Clone)]
pub struct Cx {
    pub app: Arc<App>,
    pub user: Arc<User>,
    /// What kind of device the caller's key belongs to.
    pub device: DeviceKind,
    /// Whether the caller's key made the account.
    pub primary: bool,
    /// The caller's key.
    pub key: EndpointId,
    /// The server's own key, which link offers are signed for.
    pub server: EndpointId,
    /// The app that sent the request, when it says.
    pub platform: Option<Platform>,
}

/// The frames a `Duplex` request's body goes on with, after the request.
pub type UpFrames =
    FramedRead<StreamReader<BoxStream<'static, io::Result<Bytes>>, Bytes>, LengthDelimitedCodec>;

/// How a request is answered.
pub enum Answer {
    /// One reply, or the error.
    Once(Result<Vec<u8>, ApiError>),
    /// Tagged frames until the stream ends.
    Frames(BoxStream<'static, Bytes>),
}

/// What kind of body a request has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Call,
    Stream,
    /// The body is frames: the request, then what follows it.
    Duplex,
}

type Handler = Box<dyn Fn(Cx, Bytes, Option<UpFrames>) -> BoxFuture<'static, Answer> + Send + Sync>;

struct Route {
    kind: Kind,
    handler: Handler,
}

#[derive(Default)]
pub struct Router {
    routes: HashMap<Op, Route>,
    duplicates: Vec<Op>,
}

impl Router {
    /// Routes `R` to `handler`, which answers with one reply. It runs to the
    /// end even when the client goes away, so nothing is left half done.
    pub fn call<R, F, Fut>(self, handler: F) -> Self
    where
        R: Call,
        F: Fn(Cx, R) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R::Reply, ApiError>> + Send + 'static,
    {
        self.route(R::OP, Kind::Call, move |cx, body, _up| {
            let reply = decode::<R>(&body).map(|request| tokio::spawn(handler(cx, request)));
            Box::pin(async move {
                let reply = match reply {
                    Ok(task) => match task.await {
                        Ok(reply) => reply,
                        Err(e) => {
                            log::warn!("a request handler failed: {e}");
                            Err(ApiError::Internal {})
                        }
                    },
                    Err(e) => Err(e),
                };
                Answer::Once(reply.and_then(|reply| encode(&reply)))
            })
        })
    }

    /// Routes `R` to `handler`, which streams replies into a `Sink`. It stops
    /// when the client goes away.
    pub fn stream<R, F, Fut>(self, handler: F) -> Self
    where
        R: Stream,
        F: Fn(Cx, R, Sink<R::Reply>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), ApiError>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        self.route(R::OP, Kind::Stream, move |cx, body, _up| {
            let answer = match decode::<R>(&body) {
                Ok(request) => {
                    let handler = handler.clone();
                    Answer::Frames(frames(move |tx| {
                        let sink = Sink::new(tx.clone());
                        finish(handler(cx, request, sink), tx)
                    }))
                }
                Err(e) => Answer::Once(Err(e)),
            };
            Box::pin(async move { answer })
        })
    }

    /// Routes `R` to `handler`, which reads an `Uplink` and replies into a `Sink`.
    pub fn duplex<R, F, Fut>(self, handler: F) -> Self
    where
        R: Duplex,
        F: Fn(Cx, R, Uplink<R::Up>, Sink<R::Reply>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), ApiError>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        self.route(R::OP, Kind::Duplex, move |cx, body, up| {
            let answer = match (decode::<R>(&body), up) {
                (Ok(request), Some(up)) => {
                    let handler = handler.clone();
                    Answer::Frames(frames(move |tx| {
                        let uplink = Uplink {
                            frames: up,
                            _up: PhantomData,
                        };
                        let sink = Sink::new(tx.clone());
                        finish(handler(cx, request, uplink, sink), tx)
                    }))
                }
                (Ok(_), None) => Answer::Once(Err(ApiError::bad_request(
                    "a duplex request without its frames",
                ))),
                (Err(e), _) => Answer::Once(Err(e)),
            };
            Box::pin(async move { answer })
        })
    }

    fn route(
        mut self,
        op: Op,
        kind: Kind,
        handler: impl Fn(Cx, Bytes, Option<UpFrames>) -> BoxFuture<'static, Answer>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let route = Route {
            kind,
            handler: Box::new(handler),
        };
        if self.routes.insert(op, route).is_some() {
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

    /// What kind of body `op` takes, `None` when it is not served.
    pub fn kind(&self, op: Op) -> Option<Kind> {
        self.routes.get(&op).map(|route| route.kind)
    }

    /// Runs the handler for `op` on `body`, the request; only a `Duplex`
    /// reads `up`.
    pub async fn dispatch(&self, op: Op, cx: Cx, body: Bytes, up: Option<UpFrames>) -> Answer {
        match self.routes.get(&op) {
            Some(route) => (route.handler)(cx, body, up).await,
            None => Answer::Once(Err(ApiError::bad_request(format!("{op:?} is not served")))),
        }
    }
}

/// A request body that does not decode is the client's fault.
fn decode<R: DeserializeOwned>(body: &[u8]) -> Result<R, ApiError> {
    http::decode(body).map_err(|e| ApiError::bad_request(e.to_string()))
}

fn encode<T: Serialize>(reply: &T) -> Result<Vec<u8>, ApiError> {
    http::encode(reply).map_err(|e| {
        log::warn!("a reply did not encode: {e}");
        ApiError::Internal {}
    })
}

/// Runs `done` and sends its error, if any, as the stream's last frame.
async fn finish(done: impl Future<Output = Result<(), ApiError>>, tx: mpsc::Sender<Bytes>) {
    if let Err(e) = done.await
        && let Ok(body) = http::encode(&e)
    {
        // The client may be gone already; then nobody needs the error.
        let _ = tx.send(http::tagged(ERR, &body)).await;
    }
}

/// The frames `run` sends, as a response body. The body drives `run`, so it
/// stops when the client goes away and the body is dropped.
pub(crate) fn frames<F>(run: impl FnOnce(mpsc::Sender<Bytes>) -> F) -> BoxStream<'static, Bytes>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel(FRAMES_QUEUE);
    let run = run(tx);
    let sent = stream::poll_fn(move |cx| rx.poll_recv(cx));
    let driver = stream::once(run).filter_map(|()| async { None::<Bytes> });
    stream::select(sent, driver).boxed()
}

/// What the client sends after a `Duplex` request.
pub struct Uplink<T> {
    frames: UpFrames,
    _up: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Uplink<T> {
    /// The client's next frame, `None` once it finished; a bad frame is the client's fault.
    pub async fn next(&mut self) -> Result<Option<T>, ApiError> {
        match self.frames.next().await {
            Some(Ok(frame)) => decode(&frame).map(Some),
            Some(Err(e)) => {
                log::debug!("uplink ended: {e:#}");
                Ok(None)
            }
            None => Ok(None),
        }
    }
}

/// Where a stream handler sends its replies.
pub struct Sink<T> {
    tx: mpsc::Sender<Bytes>,
    _reply: PhantomData<fn(T)>,
}

impl<T> Sink<T> {
    fn new(tx: mpsc::Sender<Bytes>) -> Self {
        Self {
            tx,
            _reply: PhantomData,
        }
    }
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
    /// Resolves once the client stops reading the stream.
    pub async fn closed(&self) {
        self.tx.closed().await
    }

    pub async fn send(&self, reply: &T) -> Result<(), Closed> {
        let body = http::encode(reply).map_err(|e| {
            log::warn!("a streamed reply did not encode: {e}");
            Closed
        })?;
        self.tx
            .send(http::tagged(OK, &body))
            .await
            .map_err(|_| Closed)
    }
}
