//! The tags Beep wraps its own words in when it writes into an agent's
//! prompt, as Claude Code wraps its `<system-reminder>`s: what Beep tells the
//! agent (its brief and the user's standing rules) in `<beep-context>`, and
//! what another session sends in `<from-session id=".." name="..">`. A log
//! keeps them in a user entry's text and the apps draw each part apart from
//! the user's own words.
//!
//! Prompts from before the tags hold the brief bare; `bare_block` and
//! `untagged` find it by the strings Beep wrote then.

use super::{checkout::WORKTREE_BRIEF, cut, mcp};

const CONTEXT: &str = "beep-context";
const FROM_SESSION: &str = "from-session";
/// The most of a context a log entry keeps: the user's words come first.
const MAX_CONTEXT: usize = 1024;

/// How every bare brief Beep wrote began.
const BARE_STARTS: [&str; 5] = [
    "Beep: the `beep` MCP server's tools",
    "Beep: the user's standing rules",
    "Beep: if you will change files and this folder",
    "This session moved from another machine.",
    "This session continues from a copy of another machine's",
];

/// One part of a user entry's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Part<'a> {
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
}

/// `text` as Beep's word to the agent.
pub(crate) fn context(text: &str) -> String {
    format!("<{CONTEXT}>\n{text}\n</{CONTEXT}>")
}

/// `text` as session `id`, called `name`, sends it.
pub(crate) fn from_session(id: &str, name: &str, text: &str) -> String {
    format!(
        "<{FROM_SESSION} id=\"{}\" name=\"{}\">\n{text}\n</{FROM_SESSION}>",
        escape(id),
        escape(name)
    )
}

/// The parts of `text`, in order; empty user parts are left out.
pub(crate) fn parts(text: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = text;
    loop {
        let next = [CONTEXT, FROM_SESSION]
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
        parts.push(if tag == CONTEXT {
            Part::Context(inner)
        } else {
            Part::Session {
                id: attribute(head, "id"),
                name: attribute(head, "name"),
                text: inner,
            }
        });
        rest = next_rest;
    }
}

/// `parts` written back as text, each context cut to `MAX_CONTEXT`.
pub(crate) fn join(parts: &[Part<'_>]) -> String {
    let written: Vec<String> = parts
        .iter()
        .map(|part| match part {
            Part::User(text) => (*text).to_owned(),
            Part::Context(text) => context(&cut(text, MAX_CONTEXT)),
            Part::Session { id, name, text } => from_session(id, name, text),
        })
        .collect();
    written.join("\n")
}

/// What was said to the agent, without Beep's context: the user's words and
/// what other sessions sent, for a title or a last prompt.
pub(crate) fn said(text: &str) -> String {
    let text = untagged(text);
    let said: Vec<&str> = parts(&text)
        .into_iter()
        .filter_map(|part| match part {
            Part::User(text) | Part::Session { text, .. } => Some(text),
            Part::Context(_) => None,
        })
        .map(|text| text.trim())
        .collect();
    said.join("\n")
}

/// One text block of a prompt, its whole text tagged as context when it is a
/// bare brief: before the tags the brief went as a block of its own.
pub(crate) fn bare_block(text: &str) -> String {
    if is_bare(text) {
        context(text.trim())
    } else {
        text.to_owned()
    }
}

/// `text` with a bare brief at its start tagged as context, for a harness
/// that keeps a prompt's blocks as one string. The brief is known by the
/// exact text it ends with.
pub(crate) fn untagged(text: &str) -> String {
    let trimmed = text.trim_start();
    if !is_bare(trimmed) {
        return text.to_owned();
    }
    for end in [mcp::BRIEF, WORKTREE_BRIEF] {
        if let Some(at) = trimmed.rfind(end) {
            let (brief, rest) = trimmed.split_at(at + end.len());
            return join(&[Part::Context(brief), Part::User(rest.trim_start())]);
        }
    }
    text.to_owned()
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

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_read_back_as_written() {
        let text = format!(
            "{}{}\nok, let's keep going",
            context("Beep: the tools.\n\nBeep: the user's standing rules:\n- Be brief."),
            from_session("a1", "Fix \"the\" build", "Run the tests <now>."),
        );
        assert_eq!(
            parts(&text),
            [
                Part::Context("Beep: the tools.\n\nBeep: the user's standing rules:\n- Be brief."),
                Part::Session {
                    id: "a1".into(),
                    name: "Fix \"the\" build".into(),
                    text: "Run the tests <now>.",
                },
                Part::User("ok, let's keep going"),
            ]
        );
        let again = join(&parts(&text));
        assert_eq!(parts(&again), parts(&text));
    }

    #[test]
    fn untagged_text_is_the_users() {
        assert_eq!(parts("a <b> c"), [Part::User("a <b> c")]);
        assert_eq!(said("<beep-contextual> hi"), "<beep-contextual> hi");
    }

    #[test]
    fn a_bare_brief_becomes_context() {
        let brief = mcp::BRIEF;
        assert_eq!(bare_block(brief), context(brief));
        assert_eq!(bare_block("ok"), "ok");
        let glued = format!("{brief}ok, let's keep going");
        assert_eq!(said(&glued), "ok, let's keep going");
        assert_eq!(
            parts(&untagged(&glued)),
            [Part::Context(brief), Part::User("ok, let's keep going")]
        );
        let worktree = format!("{WORKTREE_BRIEF}\n\n{brief}fix it");
        assert_eq!(said(&worktree), "fix it");
    }
}
