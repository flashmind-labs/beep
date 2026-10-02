use crate::{
    protocol::{Reply, Request},
    transport::{ALPN, RemoteError, Rx, framed},
};
use anyhow::{Context, Result, bail};
use iroh::{
    Endpoint, EndpointAddr,
    endpoint::{Connection, presets},
};
use std::time::Duration;

/// One authenticated iroh connection. Requests run concurrently, one stream each.
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
        let auth = Request::Auth {
            token: token.to_owned(),
        };
        match client.call(&auth).await.context("authentication failed")? {
            Reply::Done {} => {}
            other => bail!("unexpected auth reply {other:?}"),
        }
        Ok(client)
    }

    /// Sends a request and returns its single reply.
    pub async fn call(&self, request: &Request) -> Result<Reply> {
        let mut replies = self.stream(request).await?;
        replies.next().await?.context("stream closed without reply")
    }

    /// Sends a request and returns its replies as they arrive.
    pub async fn stream(&self, request: &Request) -> Result<Replies> {
        let (send, recv) = self.connection.open_bi().await?;
        let (mut tx, rx) = framed(send, recv);
        tx.send(request).await?;
        tx.finish()?;
        Ok(Replies(rx))
    }
}

pub struct Replies(Rx);

impl Replies {
    /// Next reply, `None` at end of stream. `Reply::Error` becomes a `RemoteError`.
    pub async fn next(&mut self) -> Result<Option<Reply>> {
        match self.0.next().await? {
            Some(Reply::Error { code, message }) => Err(RemoteError { code, message }.into()),
            reply => Ok(reply),
        }
    }
}
