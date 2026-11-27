//! The tags Beep wraps its own words in when it writes into an agent's
//! prompt, as Claude Code wraps its `<system-reminder>`s: what Beep tells the
//! agent (its brief and the user's standing rules) in `<beep-context>`, and
//! what another session sends in `<from-session id=".." name="..">`, and what
//! a child said or asks in `<beep-child id=".." name=".." state="..">`. A log
//! keeps them in a user entry's text and the apps draw each part apart from
//! the user's own words.
//!
//! Prompts from before the tags hold the brief bare; `bare_block` and
//! `untagged` find it by the strings Beep wrote then.

#[cfg(feature = "host")]
use crate::text::cut;
#[cfg(feature = "host")]
use regex::Regex;
#[cfg(feature = "host")]
use std::sync::LazyLock;

/// Claude Code stores interruption notices as synthetic user messages.
#[cfg(feature = "host")]
pub(crate) fn after_interruption(text: &str) -> Option<&str> {
    let text = text.trim_start();
    [
        "[Request interrupted by user]",
        "[Request interrupted by user for tool use]",
    ]
    .iter()
    .find_map(|notice| text.strip_prefix(notice))
}

const CONTEXT: &str = "beep-context";
const FROM_SESSION: &str = "from-session";
const CHILD: &str = "beep-child";
/// What another Claude Code session sends with its own `SendMessage`, and
/// what one of its agents sends, as Claude Code tags them.
const CLAUDE_SESSION: &str = "cross-session-message";
const CLAUDE_AGENT: &str = "agent-message";
/// The most of a context a log entry keeps: the user's words come first.
#[cfg(feature = "host")]
const MAX_CONTEXT: usize = 4096;

/// How every bare brief Beep wrote began.
const BARE_STARTS: [&str; 5] = [
    "Beep: the `beep` MCP server's tools",
    "Beep: the user's standing rules",
    "Beep: if you will change files and this folder",
    "This session moved from another machine.",
    "This session continues from a copy of another machine's",
];

/// How every bare brief Beep wrote ended, latest first.
const BARE_ENDS: [&str; 3] = [
    "change the user's global configuration unless asked.",
    "ask_user asks the user a question and waits for the answer.",
    BARE_WORKTREE,
];

/// The worktree brief as bare prompts hold it.
const BARE_WORKTREE: &str = "Beep: if you will change files and this folder is a git repository's main checkout, first create a new branch and a git worktree for it under the repository's .worktrees/ folder, both named for the work, and work there. Questions that change nothing need neither.";

/// One part of a user entry's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part<'a> {
    /// What the user wrote.
    User(&'a str),
    /// What Beep told the agent.
    Context(&'a str),
    /// What another session sent.
    Session {
        id: String,
        name: String,
        text: &'a str,
    },
    /// What a child the session started said: it finished a turn or waits
    /// on the user (`state`).
    Child {
        id: String,
        name: String,
        state: String,
        text: &'a str,
    },
}

/// `text` as Beep's word to the agent.
#[cfg(feature = "host")]
pub(crate) fn context(text: &str) -> String {
    format!("<{CONTEXT}>\n{text}\n</{CONTEXT}>")
}

/// `text` as session `id`, called `name`, sends it.
#[cfg(feature = "host")]
pub(crate) fn from_session(id: &str, name: &str, text: &str) -> String {
    format!(
        "<{FROM_SESSION} id=\"{}\" name=\"{}\">\n{text}\n</{FROM_SESSION}>",
        escape(id),
        escape(name)
    )
}

/// `text` from a teammate's agent `id`, called `name`, as `from_session`
/// gives it, with every tag of Beep's in the text neutralised, opening or
/// closing, in any case and spacing: it is not this user's, and must not
/// close the tag and go on as if it were. The rest arrives as written.
#[cfg(feature = "host")]
pub(crate) fn from_teammate(id: &str, name: &str, text: &str) -> String {
    static TAG: LazyLock<Regex> = LazyLock::new(|| {
        let names = [CONTEXT, FROM_SESSION, CHILD, CLAUDE_SESSION, CLAUDE_AGENT].join("|");
        Regex::new(&format!(r"(?i)<(\s*/?\s*(?:{names}))")).expect("a valid pattern")
    });

    from_session(id, name, &TAG.replace_all(text, "&lt;$1"))
}

/// `text` about the child `id`, called `name`, which is in `state`.
#[cfg(feature = "host")]
pub(crate) fn from_child(id: &str, name: &str, state: &str, text: &str) -> String {
    format!(
        "<{CHILD} id=\"{}\" name=\"{}\" state=\"{}\">\n{text}\n</{CHILD}>",
        escape(id),
        escape(name),
        escape(state)
    )
}

/// The parts of `text`, in order; empty user parts are left out.
pub fn parts(text: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = text;

    loop {
        let next = [CONTEXT, FROM_SESSION, CHILD, CLAUDE_SESSION, CLAUDE_AGENT]
            .into_iter()
            .filter_map(|tag| open_at(rest, tag).map(|at| (at, tag)))
            .min_by_key(|(at, _)| *at);
        let Some((at, tag)) = next else {
            push_user(&mut parts, rest);
            return parts;
        };
        push_user(&mut parts, &rest[..at]);
        let after = &rest[at + 1 + tag.len()..];
        let Some(head_end) = after.find('>') else {
            push_user(&mut parts, &rest[at..]);
            return parts;
        };
        let head = &after[..head_end];
        let body = &after[head_end + 1..];
        let close = format!("</{tag}>");
        let (inner, next_rest) = match body.find(&close) {
            Some(end) => (&body[..end], &body[end + close.len()..]),
            None => (body, ""),
        };
        let inner = inner.trim();
        parts.push(match tag {
            CONTEXT => Part::Context(inner),
            CHILD => Part::Child {
                id: attribute(head, "id"),
                name: attribute(head, "name"),
                state: attribute(head, "state"),
                text: inner,
            },
            FROM_SESSION => Part::Session {
                id: attribute(head, "id"),
                name: attribute(head, "name"),
                text: inner,
            },
            _ => Part::Session {
                id: attribute(head, "from"),
                name: attribute(head, "from-name"),
                text: inner,
            },
        });
        rest = next_rest;
    }
}

/// `parts` written back as text, each context cut to `MAX_CONTEXT`.
#[cfg(feature = "host")]
pub(crate) fn join(parts: &[Part<'_>]) -> String {
    let written: Vec<String> = parts
        .iter()
        .map(|part| match part {
            Part::User(text) => (*text).to_owned(),
            Part::Context(text) => context(&cut(text, MAX_CONTEXT)),
            Part::Session { id, name, text } => from_session(id, name, text),
            Part::Child {
                id,
                name,
                state,
                text,
            } => from_child(id, name, state, text),
        })
        .collect();
    written.join("\n")
}

/// What was said to the agent, without Beep's context: the user's words and
/// what other sessions sent, without image markers, for a title or last prompt.
pub(crate) fn said(text: &str) -> String {
    // A bare brief is context: only what follows it was said.
    let text = bare_split(text).map_or(text, |(_, rest)| rest);
    let said: Vec<&str> = parts(text)
        .into_iter()
        .filter_map(|part| match part {
            Part::User(text) | Part::Session { text, .. } | Part::Child { text, .. } => Some(text),
            Part::Context(_) => None,
        })
        .map(|text| text.trim())
        .collect();
    said.join("\n")
        .lines()
        .filter(|line| !line.trim().starts_with("[image: "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A readable automatic title, including old titles copied from tagged prompts.
pub fn title(text: &str) -> Option<String> {
    const MAX_CHARS: usize = 80;
    let said = said(text);
    let line = said.lines().map(str::trim).find(|line| !line.is_empty())?;

    // A streamed or previously truncated tag header is not a title. A saved
    // header may still hold a complete sender name we can recover.
    let recovered;
    let line = if line.starts_with('<') {
        let tag = [FROM_SESSION, CHILD]
            .into_iter()
            .find(|tag| open_at(line, tag) == Some(0))?;

        if line.contains('>') {
            return None;
        }

        let head = &line[1 + tag.len()..];
        let start = head.find(" name=\"")? + " name=\"".len();
        head[start..].find('"')?;
        recovered = attribute(head, "name");
        let name = recovered.trim();

        if name.is_empty() || name.starts_with('<') {
            return None;
        }

        name
    } else {
        line
    };

    if line.chars().count() <= MAX_CHARS {
        return Some(line.to_owned());
    }

    let end = line.char_indices().nth(MAX_CHARS - 3)?.0;
    let prefix = &line[..end];
    let boundary = if line[end..].starts_with(char::is_whitespace) {
        end
    } else {
        prefix
            .rfind(char::is_whitespace)
            .filter(|at| *at > 0)
            .unwrap_or(end)
    };

    Some(format!("{}...", line[..boundary].trim_end()))
}

/// The user's own words in a user entry's text, without what Beep or
/// another session put in it, as the apps draw them in the user's row.
pub fn own_words(text: &str) -> String {
    let own: Vec<&str> = parts(text)
        .into_iter()
        .filter_map(|part| match part {
            Part::User(text) => Some(text),
            _ => None,
        })
        .collect();
    own.join("\n")
}

/// One text block of a prompt, its whole text tagged as context when it is a
/// bare brief: before the tags the brief went as a block of its own.
#[cfg(feature = "host")]
pub(crate) fn bare_block(text: &str) -> String {
    if is_bare(text) {
        context(text.trim())
    } else {
        text.to_owned()
    }
}

/// `text` with a bare brief at its start tagged as context, for a harness
/// that keeps a prompt's blocks as one string.
#[cfg(feature = "host")]
pub(crate) fn untagged(text: &str) -> String {
    match bare_split(text) {
        Some((brief, rest)) => join(&[Part::Context(brief), Part::User(rest)]),
        None => text.to_owned(),
    }
}

/// A bare brief at the start of `text` and what follows it. The brief is
/// known by the exact text it ends with.
fn bare_split(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.trim_start();

    if !is_bare(trimmed) {
        return None;
    }

    BARE_ENDS.into_iter().find_map(|end| {
        let at = trimmed.rfind(end)?;
        let (brief, rest) = trimmed.split_at(at + end.len());
        Some((brief, rest.trim_start()))
    })
}

fn is_bare(text: &str) -> bool {
    let text = text.trim_start();
    BARE_STARTS.iter().any(|start| text.starts_with(start))
}

/// Where `<tag>` or `<tag ...>` opens in `text`.
fn open_at(text: &str, tag: &str) -> Option<usize> {
    let open = format!("<{tag}");
    let mut from = 0;

    while let Some(found) = text[from..].find(&open) {
        let at = from + found;

        match text[at + open.len()..].chars().next() {
            Some('>' | ' ') => return Some(at),
            _ => from = at + open.len(),
        }
    }

    None
}

fn push_user<'a>(parts: &mut Vec<Part<'a>>, text: &'a str) {
    let text = text.trim();
    if !text.is_empty() {
        parts.push(Part::User(text));
    }
}

/// The value of `name="..."` in a tag's head; empty when missing.
fn attribute(head: &str, name: &str) -> String {
    let key = format!(" {name}=\"");
    let Some(start) = head.find(&key) else {
        return String::new();
    };
    let value = &head[start + key.len()..];
    let value = value.split('"').next().unwrap_or_default();
    value
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(feature = "host")]
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
#[path = "tests/tags.rs"]
pub(crate) mod tests;
