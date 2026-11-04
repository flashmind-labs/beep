//! `beep-node setup <token>`: the machine links itself to the account that
//! made the token (see `beep_proto::setup`).

use crate::{
    link::state::{self, Search, State},
    link::{self, Claimed},
};
use anyhow::{Context, Result, bail};
use beep_proto::{link::MAX_NAME, node::NodeSetup, setup::SetupToken};
use iroh::{Endpoint, SecretKey, endpoint::presets};
use std::path::Path;
use tokio::process::Command;

/// The name when the host has none to give.
const NO_HOSTNAME: &str = "Machine";

/// A token checked against this machine, with the key to claim it.
pub struct Prepared {
    pub token: SetupToken,
    pub key: SecretKey,
}

/// Reads `token` and makes this machine's key if it has none. Refuses,
/// before anything connects, a token for another server than the one this
/// machine is already set up for.
pub fn prepare(dir: &Path, token: &str) -> Result<Prepared> {
    let token = SetupToken::parse(token).map_err(|e| anyhow::anyhow!("{e}"))?;
    if let Some(server) = state::saved_server(dir)?
        && server != token.server
    {
        bail!(
            "This setup token is for another server than the one this machine is set up \
             for. To move it, delete {} and run setup again; to keep both, pass another --dir.",
            dir.join("node.toml").display()
        );
    }
    let key = state::ensure_key(dir)?;
    Ok(Prepared { token, key })
}

/// Claims `token` for this machine and saves the server and name it was
/// linked as. `name` wins over the token's; without either, the host name.
/// `search` replaces where it looks for git checkouts.
pub async fn setup(
    dir: &Path,
    token: &str,
    name: Option<&str>,
    search: Option<Search>,
) -> Result<(State, Claimed)> {
    let name = name.map(str::trim);
    if name.is_some_and(|name| name.is_empty() || name.len() > MAX_NAME) {
        bail!("--name is empty or longer than {MAX_NAME} bytes");
    }
    if let Some(search) = &search {
        search.existing_root().context("--search")?;
    }
    let Prepared { token, key } = prepare(dir, token)?;
    let fallback = host_name().await;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(key.clone())
        .bind()
        .await?;
    let setup = NodeSetup {
        token: token.to_string(),
        name: name.map(str::to_owned),
    };
    let claimed = link::claim(&endpoint, token.server.into(), &fallback, &setup).await;
    // `run` binds its own endpoint with the same key.
    endpoint.close().await;
    let claimed = claimed?;
    let state = State::save(dir, key, Some(token.server), &claimed.name, search)?;
    Ok((state, claimed))
}

/// What the machine calls itself, cut to fit a name.
pub async fn host_name() -> String {
    let output = Command::new("hostname").output().await;
    let name = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    let name = name.split('.').next().unwrap_or_default();
    if name.is_empty() || name.len() > MAX_NAME {
        return NO_HOSTNAME.to_owned();
    }
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_for_another_server_is_refused_before_connecting() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let ours = SecretKey::generate().public();
        let theirs = SecretKey::generate().public();
        let first = State::create(dir, &ours.to_string(), "box").unwrap();

        let elsewhere = SetupToken::generate(theirs).to_string();
        let refused = prepare(dir, &elsewhere).err().expect("refused");
        assert!(
            format!("{refused:#}").contains("another server"),
            "{refused:#}"
        );

        // A token for its own server keeps the machine's key.
        let again = prepare(dir, &SetupToken::generate(ours).to_string()).unwrap();
        assert_eq!(again.key.public(), first.key.public());
    }
}
