//! The HTTPS client setup every crate shares, the same iroh uses: rustls with
//! ring and webpki roots. It installs no process-wide crypto provider.

use reqwest::ClientBuilder;
use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;

/// A client builder with the shared TLS setup. It offers HTTP/2, when built
/// with it, and HTTP/1.1 over ALPN, as reqwest does with its own TLS setup.
pub fn client() -> ClientBuilder {
    let mut tls = tls();
    tls.alpn_protocols = if cfg!(feature = "http2") {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    } else {
        vec![b"http/1.1".to_vec()]
    };
    reqwest::Client::builder().tls_backend_preconfigured(tls)
}

/// The shared TLS setup, without ALPN.
pub fn tls() -> ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };

    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        // ring's default provider has cipher suites and key exchange groups
        // for TLS 1.2 and 1.3, so the default versions always validate.
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}
