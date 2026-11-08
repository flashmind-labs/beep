//! Single-user instance mode: one owner account; other keys need `LinkDevice`.

use anyhow::{Context, Result};
use beep_proto::link::LinkOffer;
use iroh::{EndpointId, SecretKey};
use qrcode::{QrCode, render::unicode::Dense1x2};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{PoisonError, RwLock},
    time::SystemTime,
};

/// The config's `[instance]` table. Its presence turns instance mode on.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceConfig {
    /// The owner's key. Without it the first key to connect owns the server.
    pub owner: Option<EndpointId>,
    /// What the server calls itself in its link code.
    #[serde(default = "default_name")]
    pub name: String,
    /// Unused, kept so old configs load.
    #[serde(default = "on")]
    pub events: bool,
    /// Unused, kept so old configs load.
    #[serde(default = "on")]
    pub background: bool,
    /// Unused, kept so old configs load.
    #[serde(default = "on")]
    pub nodes: bool,
    /// Unused, kept so old configs load.
    #[serde(default = "on")]
    pub agents: bool,
    /// Unused, kept so old configs load.
    pub local_node: Option<PathBuf>,
}

fn on() -> bool {
    true
}

fn default_name() -> String {
    "Beep".to_owned()
}

/// A running instance: its settings and its owner once known.
pub struct Instance {
    config: InstanceConfig,
    /// The owner's account id, once claimed or read back.
    owner: RwLock<Option<String>>,
}

impl Instance {
    pub fn new(config: InstanceConfig) -> Self {
        Self {
            config,
            owner: RwLock::default(),
        }
    }

    /// The owner's key from the config, the only one that may claim.
    pub fn owner_key(&self) -> Option<&EndpointId> {
        self.config.owner.as_ref()
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// The owner's account, when known.
    pub(crate) fn owner(&self) -> Option<String> {
        self.owner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_owner(&self, account: &str) {
        *self.owner.write().unwrap_or_else(PoisonError::into_inner) = Some(account.to_owned());
    }
}

/// The server's offer of itself, as a QR code and as text.
pub fn link_code(key: &SecretKey, name: &str, now: SystemTime) -> Result<String> {
    let offer = LinkOffer::for_server(key, name, now)?;
    let uri = offer.to_uri();
    let qr = QrCode::new(uri.as_bytes())
        .context("making the QR code")?
        .render::<Dense1x2>()
        .quiet_zone(true)
        .build();
    Ok(format!(
        "Scan this with Beep on your phone to connect it to this server.\n\
         The first phone to connect owns the server.\n\n\
         {qr}\n\n  Code: {code}\n  Link: {uri}\n  Server: {server}\n\n\
         The code works for 10 minutes; `beep-server link` shows a new one.\n",
        code = offer.code(),
        server = key.public(),
    ))
}

#[cfg(test)]
#[path = "../tests/account_instance.rs"]
mod tests;
