//! Credentials a user adds once, kept sealed and handed to their machines (`Down::Credentials`).

mod check;
pub(crate) mod machines;
mod net;
mod seal;

pub use check::{Checked, Checker, MAX_ORGS, host_client};
pub use seal::{CredentialKey, HarnessOwner, Owner, WebhookOwner};

use crate::protocol::{
    AgentAccount, ApiProvider, CredentialKind, GITHUB_ENV_VAR, SecretText,
    node::{EnvSecret, ForgejoLogin, MachineCredentials},
};
use reqwest::Url;

pub use beep_projects::github_owner_var;
use secrecy::{ExposeSecret, SecretString};
use std::{fmt, str::FromStr};
use thiserror::Error;

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

/// A Forgejo or Gitea base URL: https (http only on the user's network), no query, fragment or trailing slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgejoUrl(Url);

impl ForgejoUrl {
    pub fn as_str(&self) -> &str {
        self.0.as_str().trim_end_matches('/')
    }

    /// Whether every address its host resolves to is public.
    pub async fn is_public(&self) -> bool {
        net::resolves_public(&self.0).await
    }

    /// Its host name alone, without the port.
    pub fn hostname(&self) -> &str {
        self.0.host_str().unwrap_or_default()
    }

    /// Its host, with the port when it has one, such as `git.example.com`.
    pub fn host(&self) -> String {
        let host = self.0.host_str().unwrap_or_default();
        match self.0.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        }
    }
}

impl FromStr for ForgejoUrl {
    type Err = SlotError;

    fn from_str(text: &str) -> Result<Self, SlotError> {
        let text = text.trim();
        let mut url = Url::parse(text).map_err(|_| {
            SlotError::BadUrl("The server URL is not a URL, such as https://git.example.com.")
        })?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(SlotError::BadUrl(
                "The server URL must start with https:// or http://.",
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(SlotError::BadUrl(
                "Put the token in its own field, not in the URL.",
            ));
        }
        url.set_query(None);
        url.set_fragment(None);
        // Pasted from the browser: the API sits at the server's root or
        // under its sub-path, never under `/api/v1` itself.
        let path = url.path().trim_end_matches('/').to_owned();
        let path = path.strip_suffix("/api/v1").unwrap_or(&path).to_owned();
        url.set_path(&path);
        Ok(Self(url))
    }
}

impl fmt::Display for ForgejoUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

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

/// What the machines get: `GH_TOKEN` for the default, plus a [`github_owner_var`] per owner on another account.
pub fn for_machines(opened: Vec<Opened>) -> MachineCredentials {
    let mut out = MachineCredentials::default();
    let mut owners: Vec<&str> = Vec::new();
    for login in opened.iter().filter_map(|o| o.github.as_ref()) {
        for owner in std::iter::once(&login.login).chain(&login.orgs) {
            if !owners.iter().any(|seen| seen.eq_ignore_ascii_case(owner)) {
                owners.push(owner);
            }
        }
    }
    for owner in owners {
        let Some(picked) = github_for(&opened, owner) else {
            continue;
        };
        let is_default = picked.github.as_ref().is_some_and(|login| login.default);
        if let (false, Some(name)) = (is_default, github_owner_var(owner)) {
            out.env.push(EnvSecret {
                name,
                value: SecretText::new(picked.secret.expose_secret()),
            });
        }
    }
    for Opened {
        slot,
        secret,
        github,
    } in opened
    {
        let value = SecretText::new(secret.expose_secret());
        match slot {
            Slot::Forgejo(url) => out.forgejo.push(ForgejoLogin {
                url: url.as_str().to_owned(),
                token: value,
            }),
            // Only the default account is GH_TOKEN.
            Slot::GitHub if !github.is_some_and(|login| login.default) => {}
            other => {
                if let Some(name) = other.env_var() {
                    out.env.push(EnvSecret {
                        name: name.to_owned(),
                        value,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgejo_urls_are_made_canonical() {
        let url: ForgejoUrl = " https://git.example.com/api/v1/?x=1 ".parse().unwrap();
        assert_eq!(url.as_str(), "https://git.example.com");
        let url: ForgejoUrl = "https://code.example.org:3000/forge/".parse().unwrap();
        assert_eq!(url.as_str(), "https://code.example.org:3000/forge");
        assert_eq!(url.host(), "code.example.org:3000");
        let url: ForgejoUrl = "http://forge.local:3000".parse().unwrap();
        assert_eq!(url.as_str(), "http://forge.local:3000");
        // Over a VPN a server named like one on the internet can be plain http.
        let url: ForgejoUrl = "http://git.example.com:8087".parse().unwrap();
        assert_eq!(url.as_str(), "http://git.example.com:8087");
        assert!("ftp://git.example.com".parse::<ForgejoUrl>().is_err());
        assert!("git.example.com".parse::<ForgejoUrl>().is_err());
        assert!(
            "https://me:pw@git.example.com"
                .parse::<ForgejoUrl>()
                .is_err()
        );
    }

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
    fn machines_get_the_default_as_gh_token_and_others_by_owner() {
        let opened = vec![
            account("octo-one", &["acme"], true, "tok-one"),
            account("octo-two", &["widget-co", "acme"], false, "tok-two"),
        ];
        let out = for_machines(opened);
        let env: Vec<(&str, &str)> = out
            .env
            .iter()
            .map(|var| (var.name.as_str(), var.value.expose()))
            .collect();
        assert_eq!(
            env,
            [
                ("BEEP_GH_TOKEN_OCTO_TWO", "tok-two"),
                ("BEEP_GH_TOKEN_WIDGET_CO", "tok-two"),
                ("GH_TOKEN", "tok-one"),
            ]
        );
    }

    #[test]
    fn last4_shows_little_of_short_secrets() {
        assert_eq!(last4("ghp_abcdefghijkl4f2a"), "4f2a");
        assert_eq!(last4("short123"), "23");
        assert_eq!(last4(""), "");
    }
}
