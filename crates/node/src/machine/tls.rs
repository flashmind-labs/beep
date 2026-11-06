//! The TLS setup the node's HTTPS clients share, the same iroh uses: ring
//! and webpki roots.

use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;

pub(crate) fn config() -> Result<ClientConfig, rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth())
}
