//! Names of coding agents and their models as they cross the wire. Both
//! encode as plain text, so a field that held a `String` can hold one of
//! these without changing its bytes.

use crate::AgentAccount;
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, borrow::Cow, convert::Infallible, fmt, str::FromStr};

/// What every id made of text has: `new`, `as_str`, and reading and comparing
/// it as text.
macro_rules! text_id {
    ($id:ident) => {
        impl $id {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into().into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $id {
            type Err = Infallible;

            fn from_str(id: &str) -> Result<Self, Self::Err> {
                Ok(Self::new(id))
            }
        }

        impl From<&str> for $id {
            fn from(id: &str) -> Self {
                Self::new(id)
            }
        }

        impl From<String> for $id {
            fn from(id: String) -> Self {
                Self::new(id)
            }
        }

        impl Borrow<str> for $id {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $id {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $id {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    };
}

/// A coding agent harness by its slug, the name the node gives it, such as
/// `claude` or `codex`. The set is open: a node may run an agent of its
/// own from `node.toml`, so any slug passes through.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentSlug(Cow<'static, str>);

text_id!(AgentSlug);

impl AgentSlug {
    pub const CLAUDE: AgentSlug = AgentSlug(Cow::Borrowed("claude"));
    pub const CODEX: AgentSlug = AgentSlug(Cow::Borrowed("codex"));
    pub const FLSH: AgentSlug = AgentSlug(Cow::Borrowed("flsh"));
}

impl From<AgentAccount> for AgentSlug {
    fn from(account: AgentAccount) -> Self {
        Self(Cow::Borrowed(account.slug()))
    }
}

/// A model as a coding agent names it, such as `claude-opus-4-1` or
/// `gpt-5-codex`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(String);

text_id!(ModelId);

// Kotlin and Swift see a model id as its text.
#[cfg(feature = "uniffi")]
uniffi::custom_type!(ModelId, String, {
    try_lift: |text| Ok(ModelId(text)),
    lower: |id| id.0,
});
