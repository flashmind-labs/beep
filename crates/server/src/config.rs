//! The server's TOML config.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

use iroh::EndpointId;
use secrecy::SecretString;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where the HTTP listens, for Cloudflare or another proxy in front that
    /// holds TLS: `127.0.0.1:SERVER_HTTP_PORT` unset.
    pub http_listen: Option<SocketAddr>,
    /// Where the daily log files go. Defaults to `logs`.
    pub log_dir: Option<PathBuf>,
    /// Where problem reports are kept, and who may send them.
    #[serde(default)]
    pub reports: ReportsConfig,
}

impl Config {
    pub fn log_dir(&self) -> PathBuf {
        self.log_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("logs"))
    }

    pub fn http_listen(&self) -> SocketAddr {
        self.http_listen.unwrap_or(SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            beep_proto::port::SERVER_HTTP_PORT,
        )))
    }
}

/// The config's `[reports]`. With neither `r2` nor `folder` the route
/// refuses every report; `r2` wins when both are set.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReportsConfig {
    /// A local folder to keep reports in, for dev and tests.
    pub folder: Option<PathBuf>,
    pub r2: Option<R2Config>,
    pub per_key_per_day: u32,
    pub blocked_keys: Vec<EndpointId>,
    pub blocked_ips: Vec<IpAddr>,
}

impl Default for ReportsConfig {
    fn default() -> Self {
        Self {
            folder: None,
            r2: None,
            per_key_per_day: 20,
            blocked_keys: Vec::new(),
            blocked_ips: Vec::new(),
        }
    }
}

/// The private R2 bucket reports go to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct R2Config {
    pub account_id: String,
    pub bucket: String,
    pub access_key_id: SecretString,
    pub secret_access_key: SecretString,
}
