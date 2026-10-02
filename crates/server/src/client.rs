use crate::{
    protocol::{Auth, Call, Rpc, Stream},
    transport::{ALPN, Rx, framed},
};
use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, presets},
};
use serde::de::DeserializeOwned;
use std::{marker::PhantomData, time::Duration};

/// One authenticated iroh connection. Requests run concurrently, one stream each.
/// Errors the server sends back are `ApiError`, recoverable by downcast.
pub struct Client {
    _endpoint: Endpoint,
    connection: Connection,
}

impl Client {
    pub async fn connect(addr: impl Into<EndpointAddr>, token: &str) -> Result<Self> {
        let endpoint = Endpoint::bind(presets::N0).await?;
        let connection =
            tokio::time::timeout(Duration::from_secs(15), endpoint.connect(addr, ALPN))
                .await
                .context("connect timed out")??;
        let client = Self {
            _endpoint: endpoint,
            connection,
        };
        let auth = Auth {
            token: token.to_owned(),
        };
        client.call(&auth).await.context("authentication failed")?;
        Ok(client)
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
