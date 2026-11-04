//! Names of coding agents and their models as they cross the wire. Both
//! encode as plain text, so a field that held a `String` can hold one of
//! these without changing its bytes.

use crate::AgentAccount;
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, borrow::Cow, convert::Infallible, fmt, str::FromStr};

/// A coding agent harness by its slug, the name the node gives it, such as
/// `claude` or `codex`. The set is open: a node may run an agent of its
/// own from `node.toml`, so any slug passes through.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentSlug(Cow<'static, str>);

impl AgentSlug {
    pub const CLAUDE: AgentSlug = AgentSlug(Cow::Borrowed("claude"));
    pub const CODEX: AgentSlug = AgentSlug(Cow::Borrowed("codex"));
    pub const GEMINI: AgentSlug = AgentSlug(Cow::Borrowed("gemini"));
    pub const PI: AgentSlug = AgentSlug(Cow::Borrowed("pi"));
    pub const FLSH: AgentSlug = AgentSlug(Cow::Borrowed("flsh"));
    pub const OPENCODE: AgentSlug = AgentSlug(Cow::Borrowed("opencode"));

    pub fn new(slug: impl Into<String>) -> Self {
        Self(Cow::Owned(slug.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for AgentSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AgentSlug {
    type Err = Infallible;

    fn from_str(slug: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(slug))
    }
}

impl From<&str> for AgentSlug {
    fn from(slug: &str) -> Self {
        Self::new(slug)
    }
}

impl From<String> for AgentSlug {
    fn from(slug: String) -> Self {
        Self::new(slug)
    }
}

impl From<AgentAccount> for AgentSlug {
    fn from(account: AgentAccount) -> Self {
        Self(Cow::Borrowed(account.slug()))
    }
}

impl Borrow<str> for AgentSlug {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for AgentSlug {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for AgentSlug {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// A model as a coding agent names it, such as `claude-opus-4-1` or
/// `gpt-5-codex`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(String);

// Kotlin and Swift see a model id as its text.
#[cfg(feature = "uniffi")]
uniffi::custom_type!(ModelId, String, {
    try_lift: |text| Ok(ModelId(text)),
    lower: |id| id.0,
});

impl ModelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ModelId {
    type Err = Infallible;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(id))
    }
}

impl From<&str> for ModelId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

impl From<String> for ModelId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl Borrow<str> for ModelId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PartialEq<&str> for ModelId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}
