//! The git credentials a user saved, opened: which token reads which host.

use crate::model::{ForgejoUrl, ForgejoUrlError};
use beep_proto::{AgentAccount, ApiProvider, CredentialKind, GITHUB_ENV_VAR};
use secrecy::SecretString;
use std::str::FromStr;
use thiserror::Error;

/// The last four characters of `secret`, never more than a quarter of one shorter than 16.
pub fn last4(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    let shown = if chars.len() >= 16 {
        4
    } else {
        chars.len() / 4
    };
    chars[chars.len() - shown..].iter().collect()
}

/// Which credential a value is; a user has at most one of each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    GitHub,
    Forgejo(ForgejoUrl),
    ApiKey(ApiProvider),
    /// A coding agent's own token; set in Agent accounts, never listed with the others.
    AgentToken(AgentAccount),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SlotError {
    #[error("Beep does not know the provider {0:?}.")]
    UnknownProvider(String),
    #[error("{0}")]
    BadUrl(&'static str),
}

impl From<ForgejoUrlError> for SlotError {
    fn from(error: ForgejoUrlError) -> Self {
        Self::BadUrl(error.0)
    }
}

impl Slot {
    /// The slot a request names, with a Forgejo URL made canonical.
    pub fn from_wire(kind: &CredentialKind) -> Result<Self, SlotError> {
        Ok(match kind {
            CredentialKind::GitHub {} => Self::GitHub,
            CredentialKind::Forgejo { url } => Self::Forgejo(url.parse()?),
            CredentialKind::ApiKey { provider } => Self::ApiKey(
                ApiProvider::from_str(provider)
                    .map_err(|_| SlotError::UnknownProvider(provider.clone()))?,
            ),
        })
    }

    /// The env var the machines get it as; `None` for Forgejo (teacli's config).
    pub fn env_var(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some(GITHUB_ENV_VAR),
            Self::Forgejo(_) => None,
            Self::ApiKey(provider) => Some(provider.env_var()),
            Self::AgentToken(agent) => Some(agent.env_var()),
        }
    }

    /// Git hosts always go to the machines, for gh and teacli.
    pub fn always_to_machines(&self) -> bool {
        matches!(self, Self::GitHub | Self::Forgejo(_))
    }
}

/// One credential opened for the machines.
pub struct Opened {
    pub slot: Slot,
    pub secret: SecretString,
    /// Which account a GitHub token is; `None` for every other slot.
    pub github: Option<GitHubLogin>,
}

/// Who a saved GitHub token signs in as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubLogin {
    pub login: String,
    /// The orgs the token could see when it was saved.
    pub orgs: Vec<String>,
    /// The account machines get as `GH_TOKEN`.
    pub default: bool,
}

impl GitHubLogin {
    /// Whether `owner` is this account or one of its orgs.
    fn owns(&self, owner: &str) -> bool {
        self.login.eq_ignore_ascii_case(owner)
            || self.orgs.iter().any(|org| org.eq_ignore_ascii_case(owner))
    }
}

/// The GitHub token for `owner`'s repos: that account, else one with `owner` among its orgs, else the default.
pub fn github_for<'a>(opened: &'a [Opened], owner: &str) -> Option<&'a Opened> {
    let accounts = || {
        opened
            .iter()
            .filter_map(|o| o.github.as_ref().map(|login| (o, login)))
    };
    accounts()
        .find(|(_, login)| login.login.eq_ignore_ascii_case(owner))
        .or_else(|| accounts().find(|(_, login)| login.owns(owner)))
        .or_else(|| accounts().find(|(_, login)| login.default))
        .map(|(opened, _)| opened)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn account(login: &str, orgs: &[&str], default: bool, token: &str) -> Opened {
        Opened {
            slot: Slot::GitHub,
            secret: SecretString::from(token),
            github: Some(GitHubLogin {
                login: login.into(),
                orgs: orgs.iter().map(|org| (*org).to_owned()).collect(),
                default,
            }),
        }
    }

    fn token(opened: Option<&Opened>) -> &str {
        opened.map_or("", |o| o.secret.expose_secret())
    }

    #[test]
    fn a_repository_owner_picks_its_github_account() {
        let opened = [
            account("octo-one", &["acme", "shared"], true, "tok-one"),
            account("octo-two", &["widget-co", "shared"], false, "tok-two"),
            account("octo-three", &["widget-co"], false, "tok-three"),
        ];
        assert_eq!(token(github_for(&opened, "octo-two")), "tok-two");
        assert_eq!(token(github_for(&opened, "OCTO-THREE")), "tok-three");
        assert_eq!(token(github_for(&opened, "Widget-Co")), "tok-two");
        assert_eq!(token(github_for(&opened, "acme")), "tok-one");
        assert_eq!(token(github_for(&opened, "shared")), "tok-one");
        assert_eq!(token(github_for(&opened, "someone-else")), "tok-one");
        assert!(github_for(&[], "acme").is_none());
    }

    #[test]
    fn last4_shows_little_of_short_secrets() {
        assert_eq!(last4("ghp_abcdefghijkl4f2a"), "4f2a");
        assert_eq!(last4("short123"), "23");
        assert_eq!(last4(""), "");
    }
}
