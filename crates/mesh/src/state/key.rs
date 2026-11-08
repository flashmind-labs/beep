use crate::MeshError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Project(String),
    Sync(String),
    Group(String),
    Pin(String),
    Credential(String),
    Harness(String),
    Flsh,
    /// Whether coding agent skills sync across the mesh: `skills/sync`.
    SkillSync,
    /// One saved version of the user's instructions: `instructions/<ms>.<id>`.
    InstructionVersion(String),
    /// A terminal kept running on a machine: `terminal/<id>`.
    Terminal(String),
    /// A coding agent session moving to another machine: `session-move/<id>`.
    SessionMove(String),
    /// A value an account move could not bring from the server, as the record of what to enter
    /// again: `unmoved/credential/<id>` or `unmoved/harness/<agent>/<VAR>`. Nodes keep the
    /// server's values of that family while any record of it remains.
    Unmoved(String),
    /// A device removed after a day offline, such as a container: `temporary/<id>`,
    /// written by the desktop that spent its ticket. The value is empty.
    Temporary(String),
    /// A phone that controls the mesh but cannot host code: `observer/<id>`.
    Observer(String),
    /// A kind this build does not know, as its text. A member holds it and passes it on and
    /// nothing reads it, so a newer desktop's new kind never stops an older member's sync.
    Other(String),
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Project(n) => write!(f, "project/{n}"),
            Key::Sync(n) => write!(f, "sync/{n}"),
            Key::Group(n) => write!(f, "group/{n}"),
            Key::Pin(n) => write!(f, "pin/{n}"),
            Key::Credential(n) => write!(f, "credential/{n}"),
            Key::Harness(n) => write!(f, "harness/{n}"),
            Key::Flsh => f.write_str("flsh"),
            Key::SkillSync => f.write_str("skills/sync"),
            Key::InstructionVersion(n) => write!(f, "instructions/{n}"),
            Key::Terminal(n) => write!(f, "terminal/{n}"),
            Key::SessionMove(n) => write!(f, "session-move/{n}"),
            Key::Unmoved(n) => write!(f, "unmoved/{n}"),
            Key::Temporary(n) => write!(f, "temporary/{n}"),
            Key::Observer(n) => write!(f, "observer/{n}"),
            Key::Other(text) => f.write_str(text),
        }
    }
}

impl FromStr for Key {
    type Err = MeshError;

    fn from_str(s: &str) -> Result<Self, MeshError> {
        let bad = || MeshError::BadKey(s.to_owned());
        if s == "flsh" {
            return Ok(Key::Flsh);
        }
        if s == "skills/sync" {
            return Ok(Key::SkillSync);
        }
        let (kind, name) = s.split_once('/').ok_or_else(bad)?;
        if name.is_empty() {
            return Err(bad());
        }
        let name = name.to_owned();
        match kind {
            "project" => Ok(Key::Project(name)),
            "sync" => Ok(Key::Sync(name)),
            "group" => Ok(Key::Group(name)),
            "pin" => Ok(Key::Pin(name)),
            "credential" => Ok(Key::Credential(name)),
            "harness" => Ok(Key::Harness(name)),
            "instructions" => Ok(Key::InstructionVersion(name)),
            "terminal" => Ok(Key::Terminal(name)),
            "session-move" => Ok(Key::SessionMove(name)),
            "unmoved" => Ok(Key::Unmoved(name)),
            "temporary" => Ok(Key::Temporary(name)),
            "observer" => Ok(Key::Observer(name)),
            // Not a kind this build knows, but well formed: kept as it is.
            other if kind_is_well_formed(other) && other != "flsh" => Ok(Key::Other(s.to_owned())),
            _ => Err(bad()),
        }
    }
}

/// A kind is lower case letters, digits, `-` and `_`.
fn kind_is_well_formed(kind: &str) -> bool {
    !kind.is_empty()
        && kind
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Any text decodes, so one origin's refused key never fails the frame and strands its
        // later entries; `apply` refuses a malformed one and steps past it.
        let text = String::deserialize(d)?;
        Ok(text.parse().unwrap_or(Key::Other(text)))
    }
}

impl Key {
    /// Text that is none of the kinds, and not well formed as an unknown one either.
    pub(super) fn is_malformed(&self) -> bool {
        matches!(self, Key::Other(text) if text.parse::<Key>().is_err())
    }
}
