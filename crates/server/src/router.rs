//! Maps each request op to a typed handler function.

use crate::{
    AsyncMutex,
    app::App,
    protocol::{ApiError, Call, Op, Stream},
    transport::{Frame, READ_TIMEOUT, Tx, framed},
};
use anyhow::{Context, Result, bail};
use futures::future::BoxFuture;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::Serialize;
use std::{collections::HashMap, future::Future, marker::PhantomData, sync::Arc};
use tokio::time::timeout;

/// What every handler gets besides its request.
#[derive(Clone)]
pub struct Cx {
    pub app: Arc<App>,
    pub user: Arc<str>,
    pub connection: Connection,
}

type Handler = Box<dyn Fn(Cx, Frame, Tx) -> BoxFuture<'static, Result<()>> + Send + Sync>;

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
        self.route(R::OP, move |cx, frame, mut tx| {
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

    /// Routes `R` to `handler`, which sends replies into a `Sink` and returns
    /// when the stream is done.
    pub fn stream<R, F, Fut>(self, handler: F) -> Self
    where
        R: Stream,
        F: Fn(Cx, R, Sink<R::Reply>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), ApiError>> + Send + 'static,
    {
        self.route(R::OP, move |cx, frame, tx| {
            let tx = Arc::new(AsyncMutex::new(tx));
            let sink = Sink {
                tx: tx.clone(),
                _reply: PhantomData,
            };
            let done = frame
                .decode::<R>()
                .map(|request| handler(cx, request, sink));
            Box::pin(async move {
                let done = match done {
                    Ok(done) => done.await,
                    Err(e) => Err(e),
                };
                let mut tx = tx.lock().await;
                if let Err(e) = done {
                    tx.err(&e).await?;
                }
                tx.finish()
            })
        })
    }

    fn route(
        mut self,
        op: Op,
        handler: impl Fn(Cx, Frame, Tx) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
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
        let (mut tx, mut rx) = framed(send, recv);
        let frame = timeout(READ_TIMEOUT, rx.frame())
            .await
            .context("request timed out")??
            .context("empty stream")?;
        let handler = Op::try_from(frame.tag).and_then(|op| {
            self.routes
                .get(&op)
                .ok_or_else(|| ApiError::bad_request(format!("{op:?} is not served")))
        });
        match handler {
            Ok(handler) => handler(cx, frame, tx).await,
            Err(e) => {
                tx.err(&e).await?;
                tx.finish()
            }
        }
    }
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
