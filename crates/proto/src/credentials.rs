//! Credentials as the apps list them: which service a secret is for, never the secret.

use crate::{AgentAccount, ApiKey, GITHUB_ENV_VAR};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr, time::SystemTime};
use thiserror::Error;
use url::Url;

/// What a credential signs in to. The apps read it as `{"kind": "github"}`,
/// `{"kind": "forgejo", "url": ...}`, `{"kind": "api_key", "provider": ...}` or
/// `{"kind": "agent", "agent": ...}`; the mesh keeps it as `[op, field]` (`wire`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Service {
    #[serde(rename = "github")]
    GitHub,
    Forgejo {
        url: ForgejoUrl,
    },
    ApiKey {
        provider: ApiKey,
    },
    /// A coding agent's own token; set in Agent accounts, never listed with the others.
    Agent {
        agent: AgentAccount,
    },
}

impl Service {
    /// The env var the machines get it as; `None` for Forgejo (teacli's config).
    pub fn env_var(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some(GITHUB_ENV_VAR),
            Self::Forgejo { .. } => None,
            Self::ApiKey { provider } => Some(provider.env_var()),
            Self::Agent { agent } => Some(agent.env_var()),
        }
    }
}

wire_structs! {
    /// A credential as the apps list it, without its secret.
    pub struct Credential {
        pub id: String,
        #[serde(with = "wire")]
        pub service: Service,
        /// The secret's last four characters.
        pub last4: String,
        /// Who the token signs in as, such as a GitHub login. `None` for API
        /// keys, and for a Forgejo server the server could not reach to check.
        pub account: Option<String>,
        /// The env var the machines get it as, such as `GH_TOKEN`. Empty for
        /// Forgejo, whose tokens go into teacli's config instead.
        pub env_var: String,
        /// Whether the caller's machines get it.
        pub to_machines: bool,
        pub created_at: SystemTime,
        /// Beep can no longer open it; the user has to enter it again.
        /// Machines do not get it.
        pub unreadable: bool,
    }
}

/// A [`Service`] in the mesh: `[op]` or `[op, text]`, as released apps wrote it.
mod wire {
    use super::*;
    use serde::de::Error as _;

    pub fn serialize<S: Serializer>(service: &Service, s: S) -> Result<S::Ok, S::Error> {
        match service {
            Service::GitHub => (0u8,).serialize(s),
            Service::Forgejo { url } => (1u8, url.as_ref()).serialize(s),
            Service::ApiKey { provider } => (2u8, provider.to_string()).serialize(s),
            Service::Agent { agent } => (3u8, agent.as_ref()).serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Service, D::Error> {
        let (op, text): (u8, Option<String>) = Wire::deserialize(d)?.into();
        let text = || {
            text.clone()
                .ok_or_else(|| D::Error::custom("missing field"))
        };

        match op {
            0 => Ok(Service::GitHub),
            1 => Ok(Service::Forgejo {
                url: text()?.parse().map_err(D::Error::custom)?,
            }),
            2 => Ok(Service::ApiKey {
                provider: text()?.parse().map_err(D::Error::custom)?,
            }),
            3 => Ok(Service::Agent {
                agent: text()?.parse().map_err(D::Error::custom)?,
            }),
            other => Err(D::Error::custom(format!("unknown service {other}"))),
        }
    }

    /// `[op]` or `[op, text]`.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Bare((u8,)),
        WithText(u8, String),
    }

    impl From<Wire> for (u8, Option<String>) {
        fn from(wire: Wire) -> Self {
            match wire {
                Wire::Bare((op,)) => (op, None),
                Wire::WithText(op, text) => (op, Some(text)),
            }
        }
    }
}

/// Why text is not a Forgejo URL, for the user.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ForgejoUrlError(pub &'static str);

/// A Forgejo or Gitea base URL: https (http only on the user's network), no query, fragment or trailing slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgejoUrl(Url);

impl AsRef<str> for ForgejoUrl {
    fn as_ref(&self) -> &str {
        self.0.as_str().trim_end_matches('/')
    }
}

impl ForgejoUrl {
    /// The parsed URL.
    pub fn url(&self) -> &Url {
        &self.0
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
    type Err = ForgejoUrlError;

    fn from_str(text: &str) -> Result<Self, ForgejoUrlError> {
        let text = text.trim();
        let mut url = Url::parse(text).map_err(|_| {
            ForgejoUrlError("The server URL is not a URL, such as https://git.example.com.")
        })?;

        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ForgejoUrlError(
                "The server URL must start with https:// or http://.",
            ));
        }

        if !url.username().is_empty() || url.password().is_some() {
            return Err(ForgejoUrlError(
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
        f.write_str(self.as_ref())
    }
}

impl Serialize for ForgejoUrl {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_ref())
    }
}

impl<'de> Deserialize<'de> for ForgejoUrl {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "tests/credentials.rs"]
mod tests;
