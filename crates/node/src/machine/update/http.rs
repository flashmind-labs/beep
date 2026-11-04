//! Downloads from the release folder.

use super::Error;
use reqwest::{Client, Url};
use rustls::{ClientConfig, RootCertStore};
use std::{env, sync::Arc, time::Duration};

/// Where releases are published; `BEEP_NODE_BASE` replaces it, as for the
/// installers.
const BASE: &str = "https://releases.flashmind.com/beep/node";

/// The release folder, without a trailing slash.
pub(super) fn base() -> String {
    env::var("BEEP_NODE_BASE")
        .ok()
        .filter(|base| !base.trim().is_empty())
        .map_or_else(
            || BASE.to_owned(),
            |base| base.trim_end_matches('/').to_owned(),
        )
}

pub(super) fn url(base: &str, path: &str) -> Result<Url, Error> {
    let text = format!("{base}/{path}");
    Url::parse(&text).map_err(|source| Error::BadBase { text, source })
}

/// An HTTPS client with the same TLS setup iroh uses: ring, webpki roots.
pub(super) fn client() -> Result<Client, Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(Error::Tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Client::builder()
        .use_preconfigured_tls(tls)
        .user_agent(concat!("beep-node/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(Error::Client)
}

pub(super) async fn fetch(client: &Client, url: &Url) -> Result<Vec<u8>, Error> {
    let network = |source| Error::Network {
        url: url.clone(),
        source,
    };
    let response = client.get(url.clone()).send().await.map_err(network)?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::Status {
            url: url.clone(),
            status,
        });
    }
    Ok(response.bytes().await.map_err(network)?.to_vec())
}
