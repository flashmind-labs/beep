//! What a profile is read from.

use crate::{apps::feeds::mail::Mailbox, apps::mcp::FASTMAIL};
use std::fmt;

/// One thing the pass reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Mail(Mailbox),
    Calendar(Calendar),
}

/// A calendar the user can connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Calendar {
    Google,
    Outlook,
    Fastmail,
}

impl Source {
    pub(super) const ALL: [Source; 6] = [
        Source::Mail(Mailbox::Gmail),
        Source::Mail(Mailbox::Outlook),
        Source::Mail(Mailbox::Fastmail),
        Source::Calendar(Calendar::Google),
        Source::Calendar(Calendar::Outlook),
        Source::Calendar(Calendar::Fastmail),
    ];

    /// How the user knows it.
    pub fn name(self) -> &'static str {
        match self {
            Source::Mail(mailbox) => mailbox.name(),
            Source::Calendar(Calendar::Google) => "Google Calendar",
            Source::Calendar(Calendar::Outlook) => "Outlook calendar",
            Source::Calendar(Calendar::Fastmail) => "Fastmail calendar",
        }
    }

    /// The key of its read in the store.
    pub fn key(self) -> &'static str {
        match self {
            Source::Mail(mailbox) => mailbox.slug(),
            Source::Calendar(Calendar::Google) => "googlecalendar",
            Source::Calendar(Calendar::Outlook) => "outlook-calendar",
            Source::Calendar(Calendar::Fastmail) => "fastmail-calendar",
        }
    }

    /// What connecting the app `slug` lets the pass read.
    pub fn from_slug(slug: &str) -> &'static [Source] {
        match slug {
            "gmail" => &[Source::Mail(Mailbox::Gmail)],
            "googlecalendar" => &[Source::Calendar(Calendar::Google)],
            "outlook" => &[
                Source::Mail(Mailbox::Outlook),
                Source::Calendar(Calendar::Outlook),
            ],
            FASTMAIL => &[
                Source::Mail(Mailbox::Fastmail),
                Source::Calendar(Calendar::Fastmail),
            ],
            _ => &[],
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// The sources behind the connected apps `slugs`.
pub(super) fn sources(slugs: &[String]) -> Vec<Source> {
    slugs
        .iter()
        .flat_map(|slug| Source::from_slug(slug))
        .copied()
        .collect()
}
