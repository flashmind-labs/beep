//! What a memory is about: the user, a machine, a project or an account.

use super::MAX_SCOPE_NAME;
use anyhow::{Result, ensure};
use iroh::EndpointId;

/// What a memory is about; pre-scope memories are about the user.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    #[default]
    User,
    /// A beep-node machine by key, so renaming keeps its memories.
    Machine(EndpointId),
    /// A project, such as a repository, by the name Beep knows it by.
    Project(String),
    /// One of the user's accounts, by app, such as `github`.
    Account(String),
}

impl Scope {
    /// A project scope, its name trimmed and lowercased so "Beep" and "beep" are one.
    pub fn project(name: &str) -> Result<Self> {
        Ok(Self::Project(scope_name(name)?))
    }

    pub fn account(app: &str) -> Result<Self> {
        Ok(Self::Account(scope_name(app)?))
    }

    /// What kind of scope it is, safe to log where its name is not.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Machine(_) => "machine",
            Self::Project(_) => "project",
            Self::Account(_) => "account",
        }
    }

    /// The value kept in the `scope` metadata key, `None` for the user.
    pub(super) fn stored(&self) -> Option<String> {
        match self {
            Self::User => None,
            Self::Machine(node) => Some(format!("machine:{node}")),
            Self::Project(name) => Some(format!("project:{name}")),
            Self::Account(app) => Some(format!("account:{app}")),
        }
    }

    /// A stored scope; an unreadable one counts as the user's, the widest.
    pub(super) fn from_stored(text: Option<&str>) -> Self {
        let Some((kind, name)) = text.and_then(|text| text.split_once(':')) else {
            return Self::User;
        };
        match kind {
            "machine" => name.parse().map_or(Self::User, Self::Machine),
            "project" => Self::Project(name.to_owned()),
            "account" => Self::Account(name.to_owned()),
            _ => Self::User,
        }
    }
}

pub(super) fn scope_name(name: &str) -> Result<String> {
    let name = name.trim().to_lowercase();
    ensure!(
        !name.is_empty() && name.chars().count() <= MAX_SCOPE_NAME && !name.contains(':'),
        "a scope name has 1 to {MAX_SCOPE_NAME} characters and no colon"
    );
    Ok(name)
}
