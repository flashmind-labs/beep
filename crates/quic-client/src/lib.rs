//! Small HTTP/3-only client for smoke tests and protocol development.
use anyhow::Result;
use bytes::{Buf, Bytes};
use http::{Request, StatusCode};
use rustls::pki_types::CertificateDer;
use std::{net::SocketAddr, sync::Arc, time::Duration};

pub struct Client {
    endpoint: quinn::Endpoint,
    sender: h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>,
    driver: tokio::task::JoinHandle<()>,
    authority: String,
}

impl Client {
    pub async fn connect(
        addr: SocketAddr,
        server_name: &str,
        certs: Vec<CertificateDer<'static>>,
    ) -> Result<Self> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for cert in certs {
            roots.add(cert)?;
        }
        let mut tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        let mut endpoint = quinn::Endpoint::client(
            if addr.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            }
            .parse()?,
        )?;
        endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(tls)?,
        )));
        let connection = tokio::time::timeout(
            Duration::from_secs(10),
            endpoint.connect(addr, server_name)?,
        )
        .await??;
        let (mut driver, sender) = h3::client::new(h3_quinn::Connection::new(connection)).await?;
        let driver = tokio::spawn(async move {
            let _ = futures::future::poll_fn(|cx| driver.poll_close(cx)).await;
        });
        Ok(Self {
            endpoint,
            sender,
            driver,
            authority: format!("{server_name}:{}", addr.port()),
        })
    }

    /// Calls `on_chunk` as bytes arrive. NDJSON callers must buffer incomplete lines.
    pub async fn request(
        &mut self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Bytes>,
        mut on_chunk: impl FnMut(&[u8]),
    ) -> Result<StatusCode> {
        let request = Request::builder()
            .method(method)
            .uri(format!("https://{}{path}", self.authority))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(())?;
        let mut stream = self.sender.send_request(request).await?;
        if let Some(body) = body {
            stream.send_data(body).await?;
        }
        stream.finish().await?;
        let response = stream.recv_response().await?;
        while let Some(mut chunk) = stream.recv_data().await? {
            let data = chunk.copy_to_bytes(chunk.remaining());
            on_chunk(&data);
        }
        Ok(response.status())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.endpoint.close(0u32.into(), b"client closed");
        self.driver.abort();
    }
}
