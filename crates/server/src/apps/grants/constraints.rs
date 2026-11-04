//! What a grant pins: which tools can be granted and the arguments they may take.

use super::{MAX_PIN_CHARS, MAX_RECIPIENTS};
use crate::apps::permissions::ToolSlug;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// A value an argument may take without the grant pinning it.
#[derive(Clone, Copy)]
pub(super) enum Literal {
    Text(&'static str),
    Flag(bool),
}

impl Literal {
    pub(super) fn is(self, value: &Value) -> bool {
        match self {
            Literal::Text(text) => value.as_str() == Some(text),
            Literal::Flag(flag) => value.as_bool() == Some(flag),
        }
    }
}

/// How a grantable tool's arguments are held; an unlisted argument puts a call outside every grant.
pub(super) struct Grantable {
    pub(super) tool: &'static str,
    /// Who receives. Every address in them must be one the grant allows.
    pub(super) recipients: &'static [&'static str],
    /// At least one must name a recipient.
    pub(super) primary: &'static [&'static str],
    /// May be pinned to one exact value; unpinned ones only as `fixed` allows.
    pub(super) pinnable: &'static [&'static str],
    /// Pins every grant of the tool must have.
    pub(super) required: &'static [&'static str],
    /// The values an unpinned argument may take, such as `user_id: "me"`.
    pub(super) fixed: &'static [(&'static str, Literal)],
    /// What the agent writes: the text and how it is formatted.
    pub(super) free: &'static [&'static str],
    /// The text a call sends, subject first, for the notice after a use.
    pub(super) text: &'static [&'static str],
    /// One call, as the card counts them.
    pub(super) noun: &'static str,
}

/// Every tool a grant can name: mail senders whose arguments name the recipients.
pub(super) const GRANTABLE: &[Grantable] = &[
    Grantable {
        tool: "GMAIL_REPLY_TO_THREAD",
        recipients: &["recipient_email", "extra_recipients", "cc", "bcc"],
        primary: &["recipient_email"],
        pinnable: &["thread_id"],
        required: &["thread_id"],
        fixed: &[("user_id", Literal::Text("me"))],
        free: &["message_body", "is_html", "include_signature"],
        text: &["message_body"],
        noun: "reply",
    },
    Grantable {
        tool: "GMAIL_SEND_EMAIL",
        recipients: &["recipient_email", "to", "extra_recipients", "cc", "bcc"],
        primary: &["recipient_email", "to"],
        pinnable: &[],
        required: &[],
        fixed: &[("user_id", Literal::Text("me"))],
        free: &["subject", "body", "is_html", "include_signature"],
        text: &["subject", "body"],
        noun: "email",
    },
    Grantable {
        tool: "OUTLOOK_SEND_EMAIL",
        recipients: &["to", "cc_emails", "bcc_emails"],
        primary: &["to"],
        pinnable: &[],
        required: &[],
        fixed: &[("user_id", Literal::Text("me"))],
        free: &[
            "subject",
            "body",
            "is_html",
            "to_name",
            "save_to_sent_items",
        ],
        text: &["subject", "body"],
        noun: "email",
    },
    Grantable {
        tool: "FASTMAIL_SEND_EMAIL",
        recipients: &["to", "cc", "bcc"],
        primary: &["to"],
        pinnable: &["mode", "emailId", "account"],
        required: &[],
        fixed: &[
            ("mode", Literal::Text("new")),
            ("replyAll", Literal::Flag(false)),
        ],
        free: &["subject", "body", "format"],
        text: &["subject", "body"],
        noun: "email",
    },
];

pub(super) fn grantable(tool: &ToolSlug) -> Option<&'static Grantable> {
    GRANTABLE.iter().find(|spec| spec.tool == tool.as_str())
}

/// What a grant holds a call to, never changed after approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Constraints {
    /// Every address a call may send to, lowercased.
    pub(super) recipients: BTreeSet<String>,
    /// Arguments a call must give with exactly these values.
    pub(super) pinned: BTreeMap<String, String>,
}

impl Constraints {
    /// The constraints the agent asked for, or why they cannot be granted.
    pub(super) fn new(
        spec: &Grantable,
        recipients: &[String],
        exact: &Map<String, Value>,
    ) -> Result<Self, String> {
        if recipients.is_empty() {
            return Err("Name at least one recipient.".to_owned());
        }
        if recipients.len() > MAX_RECIPIENTS {
            return Err(format!("Name at most {MAX_RECIPIENTS} recipients."));
        }
        let recipients = recipients
            .iter()
            .map(|text| {
                plain_address(text.trim()).ok_or_else(|| format!("{text} is not an email address."))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let mut pinned = BTreeMap::new();
        for (key, value) in exact {
            if !spec.pinnable.contains(&key.as_str()) {
                return Err(format!(
                    "{} cannot be pinned for {}; it can pin: {}.",
                    key,
                    spec.tool,
                    list_or_none(spec.pinnable)
                ));
            }
            let value = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.chars().count() <= MAX_PIN_CHARS)
                .ok_or_else(|| format!("Give {key} as the exact text the call will use."))?;
            pinned.insert(key.clone(), value.to_owned());
        }
        if let Some(missing) = spec.required.iter().find(|key| !pinned.contains_key(**key)) {
            return Err(format!("A grant for {} must pin {missing}.", spec.tool));
        }
        Ok(Self { recipients, pinned })
    }

    /// Whether a call of `spec`'s tool with `args` stays within these.
    pub(super) fn allow(&self, spec: &Grantable, args: &Value) -> bool {
        let Value::Object(args) = args else {
            return false;
        };
        let each_held = args
            .iter()
            .filter(|(_, value)| !absent(value))
            .all(|(key, value)| {
                let key = key.as_str();
                if spec.recipients.contains(&key) {
                    addresses(value)
                        .is_some_and(|found| found.iter().all(|to| self.recipients.contains(to)))
                } else if let Some(pin) = self.pinned.get(key) {
                    value.as_str() == Some(pin.as_str())
                } else if spec.free.contains(&key) {
                    true
                } else {
                    spec.fixed
                        .iter()
                        .any(|(name, literal)| *name == key && literal.is(value))
                }
            });
        let pins_given = self
            .pinned
            .iter()
            .all(|(key, pin)| args.get(key).and_then(Value::as_str) == Some(pin.as_str()));
        let addressed = spec
            .primary
            .iter()
            .any(|key| args.get(*key).is_some_and(|value| !absent(value)));
        each_held && pins_given && addressed
    }
}

pub(super) fn list_or_none(keys: &[&str]) -> String {
    if keys.is_empty() {
        "nothing".to_owned()
    } else {
        keys.join(", ")
    }
}

/// An argument left empty, which the tool reads as not given.
pub(super) fn absent(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

/// The lowercased plain addresses `value` names; `None` if any part is not a plain address.
pub(super) fn addresses(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(text) => text.split(',').map(address).collect(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => address(text),
                Value::Object(_) => object_address(item),
                _ => None,
            })
            .collect(),
        Value::Object(_) => Some(vec![object_address(value)?]),
        _ => None,
    }
}

/// `jo@example.com` or `Jo <jo@example.com>`, lowercased.
pub(super) fn address(text: &str) -> Option<String> {
    let text = text.trim();
    match text.split_once('<') {
        Some((name, rest)) => {
            let inner = rest.strip_suffix('>')?;
            if name.contains(['@', '<', '>', '"']) {
                return None;
            }
            plain_address(inner.trim())
        }
        None => plain_address(text),
    }
}

/// `{"email": "jo@example.com", "name": "Jo"}`, with no other keys.
pub(super) fn object_address(item: &Value) -> Option<String> {
    let map = item.as_object()?;
    let mut email = None;
    for (key, value) in map {
        match key.as_str() {
            "email" | "address" if email.is_none() => email = Some(value.as_str()?),
            "name" => {
                if value.as_str()?.contains(['@', '<', '>']) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    plain_address(email?.trim())
}

/// One bare address in plain ASCII, lowercased.
pub(super) fn plain_address(text: &str) -> Option<String> {
    let (local, domain) = text.split_once('@')?;
    let plain = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && text.chars().all(|c| {
            c.is_ascii_graphic()
                && !matches!(
                    c,
                    ',' | ';' | '<' | '>' | '"' | '(' | ')' | '[' | ']' | '\\' | ':'
                )
        });
    plain.then(|| text.to_ascii_lowercase())
}
