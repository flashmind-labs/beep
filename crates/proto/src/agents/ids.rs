//! Agent, model and session identifiers encode as plain text, preserving
//! the wire representation of older string fields.

use crate::AgentAccount;
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, borrow::Cow, convert::Infallible, fmt, str::FromStr};

/// Construction, borrowed access, formatting, parsing and text comparison.
macro_rules! text_id {
    ($id:ident) => {
        impl $id {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into().into())
            }

            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl AsRef<str> for $id {
            fn as_ref(&self) -> &str {
                &self.0
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
    pub const AGY: AgentSlug = AgentSlug(Cow::Borrowed("agy"));
    pub const CLAUDE: AgentSlug = AgentSlug(Cow::Borrowed("claude"));
    pub const CODEX: AgentSlug = AgentSlug(Cow::Borrowed("codex"));
    pub const FLSH: AgentSlug = AgentSlug(Cow::Borrowed("flsh"));
    pub const PI: AgentSlug = AgentSlug(Cow::Borrowed("pi"));
}

impl From<AgentAccount> for AgentSlug {
    fn from(account: AgentAccount) -> Self {
        Self(Cow::Borrowed(Into::<&'static str>::into(account)))
    }
}

/// A model as a coding agent names it, such as `claude-opus-4-1` or
/// `gpt-5-codex`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(String);

text_id!(ModelId);

/// A session's identifier, assigned by Beep or by a coding agent harness.
/// Harnesses do not all use UUIDs, so its text is preserved on the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

text_id!(SessionId);

impl From<SessionId> for String {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

impl From<&SessionId> for String {
    fn from(id: &SessionId) -> Self {
        id.0.clone()
    }
}

impl From<&String> for SessionId {
    fn from(id: &String) -> Self {
        Self::new(id.as_str())
    }
}

impl PartialEq<String> for SessionId {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl PartialEq<SessionId> for String {
    fn eq(&self, other: &SessionId) -> bool {
        self == &other.0
    }
}

impl PartialEq<SessionId> for str {
    fn eq(&self, other: &SessionId) -> bool {
        self == other.0
    }
}
