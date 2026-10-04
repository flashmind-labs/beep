use crate::{
    Call, Rpc, Stream,
    wire::{ALPN, Rx, framed},
};
use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointAddr, SecretKey,
    endpoint::{Connection, presets},
};
use serde::de::DeserializeOwned;
use std::{marker::PhantomData, time::Duration};

/// One iroh connection, made with the user's own key. Requests run concurrently, one stream each.
/// Errors the server sends back are `ApiError`, recoverable by downcast.
pub struct Client {
    _endpoint: Endpoint,
    connection: Connection,
}

impl Client {
    /// True once the connection is gone and a new one is needed.
    pub fn is_closed(&self) -> bool {
        self.connection.close_reason().is_some()
    }

    /// Connects as `identity`. The server knows the user by its public key,
    /// and a key it has not seen before starts a new account.
    pub async fn connect(addr: impl Into<EndpointAddr>, identity: SecretKey) -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(identity)
            .bind()
            .await?;
        let connection =
            tokio::time::timeout(Duration::from_secs(15), endpoint.connect(addr, ALPN))
                .await
                .context("connect timed out")??;
        Ok(Self {
            _endpoint: endpoint,
            connection,
        })
    }

    /// Sends a request and returns its reply.
    pub async fn call<R: Call>(&self, request: &R) -> Result<R::Reply> {
        let mut replies = self.open(request).await?;
        replies.next().await?.context("stream closed without reply")
    }

    /// Sends a request and returns its replies as they arrive.
    pub async fn stream<R: Stream>(&self, request: &R) -> Result<Replies<R::Reply>> {
        self.open(request).await
    }

    async fn open<R: Rpc, T>(&self, request: &R) -> Result<Replies<T>> {
        let (send, recv) = self.connection.open_bi().await?;
        let (mut tx, rx) = framed(send, recv);
        tx.request(request).await?;
        tx.finish()?;
        Ok(Replies {
            rx,
            _reply: PhantomData,
        })
    }
}

pub struct Replies<T> {
    rx: Rx,
    _reply: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Replies<T> {
    /// Next reply, `None` at end of stream.
    pub async fn next(&mut self) -> Result<Option<T>> {
        self.rx.reply().await
    }
}
