//! Window labels: which project or assistant chat a window apart from the
//! main one shows, written into its label and read back from it.

use serde::{Deserialize, Serialize};

/// Labels of the windows that each show one project, or one chat.
const PROJECT_PREFIX: &str = "project-";
const CHAT_PREFIX: &str = "chat-";

/// What a window apart from the main one shows alone: a project, or an
/// assistant chat with its threads. Its label says which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Alone {
    Project { project: String },
    Chat { machine_id: String, session: String },
}

impl Alone {
    /// The window's label: a prefix, then the name in hex, since a label
    /// takes only letters, digits and a few signs.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Project { project } => format!("{PROJECT_PREFIX}{}", hex(project)),
            Self::Chat {
                machine_id,
                session,
            } => format!("{CHAT_PREFIX}{}", hex(&format!("{machine_id}\n{session}"))),
        }
    }

    /// What window `label` shows, `None` for the main one and any other.
    pub(crate) fn of(label: &str) -> Option<Self> {
        if let Some(name) = label.strip_prefix(PROJECT_PREFIX) {
            return Some(Self::Project {
                project: unhex(name)?,
            });
        }

        let chat = unhex(label.strip_prefix(CHAT_PREFIX)?)?;
        let (machine_id, session) = chat.split_once('\n')?;
        Some(Self::Chat {
            machine_id: machine_id.to_owned(),
            session: session.to_owned(),
        })
    }

    /// The window's title until its page names it.
    pub(crate) fn title(&self) -> &str {
        match self {
            Self::Project { project } => project,
            Self::Chat { .. } => "Assistant",
        }
    }
}

/// Whether `label` is a window showing a project or a chat alone.
pub(crate) fn alone(label: &str) -> bool {
    label.starts_with(PROJECT_PREFIX) || label.starts_with(CHAT_PREFIX)
}

pub(crate) fn hex(text: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(text.len() * 2);

    for byte in text.bytes() {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }

    hex
}

fn unhex(hex: &str) -> Option<String> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }

    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}
