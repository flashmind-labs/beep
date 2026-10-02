use anyhow::Result;
use bytes::Bytes;
use iroh::EndpointAddr;
use iroh::endpoint::presets;
use std::time::Duration;

pub const ALPN: &[u8] = b"beep/1";

pub struct Client {
    _endpoint: iroh::Endpoint,
    connection: iroh::endpoint::Connection,
}

impl Client {
    pub async fn connect(addr: impl Into<EndpointAddr>) -> Result<Self> {
        let endpoint = iroh::Endpoint::bind(presets::N0).await?;
        let connection =
            tokio::time::timeout(Duration::from_secs(15), endpoint.connect(addr, ALPN)).await??;
        Ok(Self {
            _endpoint: endpoint,
            connection,
        })
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Bytes>,
        mut on_chunk: impl FnMut(&[u8]),
    ) -> Result<u16> {
        let (mut send, mut recv) = self.connection.open_bi().await?;

        let header = serde_json::to_vec(&serde_json::json!({
            "m": method,
            "p": path,
            "a": format!("Bearer {token}"),
        }))?;
        send.write_all(&(header.len() as u32).to_be_bytes()).await?;
        send.write_all(&header).await?;

        if let Some(body) = body {
            send.write_all(&body).await?;
        }
        send.finish()?;

        let mut status_buf = [0u8; 2];
        recv.read_exact(&mut status_buf).await?;
        let status = u16::from_be_bytes(status_buf);

        let mut buf = vec![0u8; 8192];
        while let Ok(Some(n)) = recv.read(&mut buf).await {
            on_chunk(&buf[..n]);
        }

        Ok(status)
    }
}
