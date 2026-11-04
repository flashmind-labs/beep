//! Linking a machine: setup tokens and the check codes a node shows.

use super::Nodes;
use crate::{
    account::user::User,
    app::App,
    protocol::{
        DeviceKind,
        link::{LinkOffer, MAX_NAME},
        setup::{SetupToken, TOKEN_TTL},
    },
    store::{LinkKey, NewKey},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use std::time::SystemTime;

/// Links the machine whose `beep-node link` printed `text` to `user`, giving its name.
pub async fn link_machine(nodes: &Nodes, user: &User, text: &str) -> Result<String> {
    let text = text.trim();
    let offer = match LinkOffer::parse(text) {
        Ok(offer) => offer,
        Err(_) if is_check_code(text) => bail!(
            "That is the six-character check code. Pass the whole `beep-link:` link that \
             `beep-node link` printed below it."
        ),
        Err(e) => bail!("Not a `beep-link:` link: {e}."),
    };
    let server = nodes
        .server()
        .context("This server does not take machines.")?;
    offer
        .verify(server, SystemTime::now())
        .map_err(|e| anyhow::anyhow!("Could not link it: {e}."))?;
    if offer.device == server {
        bail!("That is this server's own link code, not a machine's.");
    }
    let name = offer.name.trim();
    let key = NewKey {
        endpoint: offer.device,
        name,
        kind: DeviceKind::Desktop {},
    };
    match user.link_key(key).await? {
        LinkKey::Linked(_) => {
            user.changed("");
            Ok(name.to_owned())
        }
        LinkKey::Taken => bail!("That machine is already linked to an account."),
    }
}

/// A setup token `create_token` made.
pub struct NewToken {
    pub token: SetupToken,
    pub expires_at: DateTime<Utc>,
}

/// Why `create_token` made none.
#[derive(Debug, thiserror::Error)]
pub enum TokenRefusal {
    #[error("This server does not take machines.")]
    NoServer,
    #[error("The machine's name is empty or too long.")]
    BadName,
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

/// A one-time token, valid for `TOKEN_TTL`, that links a `beep-node setup` machine to `user`.
pub async fn create_token(
    app: &App,
    user: &User,
    name: Option<&str>,
) -> Result<NewToken, TokenRefusal> {
    let server = app.nodes.server().ok_or(TokenRefusal::NoServer)?;
    let name = name.map(str::trim).filter(|name| !name.is_empty());
    if name.is_some_and(|name| name.len() > MAX_NAME) {
        return Err(TokenRefusal::BadName);
    }
    let token = SetupToken::generate(server);
    let expires_at = user
        .create_node_token(&token.hash(), name, TOKEN_TTL)
        .await?;
    Ok(NewToken { token, expires_at })
}

/// Six letters and digits, as the check code `beep-node link` prints.
pub(super) fn is_check_code(text: &str) -> bool {
    let code: String = text.split_whitespace().collect();
    code.len() == 6 && code.chars().all(|c| c.is_ascii_alphanumeric())
}
