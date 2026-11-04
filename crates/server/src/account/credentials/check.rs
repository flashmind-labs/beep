//! The client the server uses to ask git hosts about tokens.

use super::net::PublicOnly;
use reqwest::{Client, redirect::Policy};
use std::{sync::Arc, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(15);

/// A git host client that reaches only public addresses and follows no redirect.
pub fn host_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .timeout(TIMEOUT)
        .user_agent("beep-server")
        // A redirect could send the token, or the probe, anywhere.
        .redirect(Policy::none())
        // A proxy would resolve the name itself, past the resolver.
        .no_proxy()
        .dns_resolver(Arc::new(PublicOnly))
        .build()?)
}
