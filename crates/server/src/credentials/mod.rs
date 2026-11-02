//! Credentials a user adds once (a GitHub token, a token per Forgejo or
//! Gitea server, API keys) that Beep keeps sealed and hands to the user's
//! machines (see `Down::Credentials`).

mod check;
mod net;
mod seal;

pub use check::{Checked, Checker, host_client};
pub use seal::{CredentialKey, Owner};

use crate::protocol::{
    ApiProvider, CredentialKind, GITHUB_ENV_VAR,
    node::{EnvSecret, ForgejoLogin, MachineCredentials},
};
use reqwest::Url;
use std::{fmt, str::FromStr};
use thiserror::Error;

/// Which credential a value is; a user has at most one of each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    GitHub,
    Forgejo(ForgejoUrl),
    ApiKey(ApiProvider),
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

    pub fn to_wire(&self) -> CredentialKind {
        match self {
            Self::GitHub => CredentialKind::GitHub {},
            Self::Forgejo(url) => CredentialKind::Forgejo {
                url: url.as_str().to_owned(),
            },
            Self::ApiKey(provider) => CredentialKind::ApiKey {
                provider: provider.slug().to_owned(),
            },
        }
    }

    /// The env var the machines get it as; `None` for Forgejo, whose
    /// tokens go into teacli's config.
    pub fn env_var(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some(GITHUB_ENV_VAR),
            Self::Forgejo(_) => None,
            Self::ApiKey(provider) => Some(provider.env_var()),
        }
    }

    /// Git hosts always go to the machines: gh and teacli are why they are
    /// there.
    pub fn always_to_machines(&self) -> bool {
        !matches!(self, Self::ApiKey(_))
    }
}

/// A Forgejo or Gitea server's base URL: https, or http for a host that is
/// plainly on the user's own network, no query or fragment, no trailing
/// slash. teacli appends `/api/v1/...` to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgejoUrl(Url);

impl ForgejoUrl {
    pub fn as_str(&self) -> &str {
        self.0.as_str().trim_end_matches('/')
    }

    /// Whether the server could probe it: every address its host resolves
    /// to is public. A server on the user's own network is saved unchecked.
    pub async fn is_public(&self) -> bool {
        net::resolves_public(&self.0).await
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
        if url.scheme() == "http" && !net::is_local_name(&url) {
            return Err(SlotError::BadUrl(
                "Use https:// for a server on the internet: over http the token travels \
                 in the clear.",
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

/// The last four characters of `secret`, all of it when it is short. Never
/// more than a quarter of a secret shorter than 16 characters.
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
    pub secret: secrecy::SecretString,
}

/// What the machines get from `opened`, the credentials marked for them.
pub fn for_machines(opened: Vec<Opened>) -> MachineCredentials {
    use secrecy::ExposeSecret;
    let mut out = MachineCredentials::default();
    for Opened { slot, secret } in opened {
        let value = crate::protocol::SecretText::new(secret.expose_secret());
        match slot {
            Slot::Forgejo(url) => out.forgejo.push(ForgejoLogin {
                url: url.as_str().to_owned(),
                token: value,
            }),
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
        assert_eq!(
            "http://git.example.com".parse::<ForgejoUrl>(),
            Err(SlotError::BadUrl(
                "Use https:// for a server on the internet: over http the token travels \
                 in the clear."
            ))
        );
        assert!("ftp://git.example.com".parse::<ForgejoUrl>().is_err());
        assert!("git.example.com".parse::<ForgejoUrl>().is_err());
        assert!(
            "https://me:pw@git.example.com"
                .parse::<ForgejoUrl>()
                .is_err()
        );
    }

    #[test]
    fn slots_come_from_the_wire() {
        let slot = Slot::from_wire(&CredentialKind::ApiKey {
            provider: "zai".into(),
        })
        .unwrap();
        assert_eq!(slot, Slot::ApiKey(ApiProvider::Zai));
        assert_eq!(slot.env_var(), Some("ZAI_API_KEY"));
        assert!(
            Slot::from_wire(&CredentialKind::ApiKey {
                provider: "nope".into()
            })
            .is_err()
        );
        assert_eq!(Slot::GitHub.env_var(), Some("GH_TOKEN"));
    }

    #[test]
    fn last4_shows_little_of_short_secrets() {
        assert_eq!(last4("ghp_abcdefghijkl4f2a"), "4f2a");
        assert_eq!(last4("short123"), "23");
        assert_eq!(last4(""), "");
    }
}
