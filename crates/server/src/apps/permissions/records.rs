//! What an approval card shows: the call's fields and the records its IDs name.

use super::MAX_FIELDS;
use super::MAX_RECORDS;
use super::MAX_VALUE_CHARS;
use super::ToolSlug;
use super::capitalize;
use crate::{account::user::User, protocol::ApprovalField};
use anyhow::Result;
use flashmind::{
    core::{Conversation, EntryKind},
    types::ToolCall,
};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// What the records `call_id` names are, from earlier tool results in `conversation`.
pub(crate) fn describe_records(call_id: &str, conversation: &Conversation) -> Vec<ApprovalField> {
    find_call(call_id, conversation)
        .map(|call| records(call, conversation))
        .unwrap_or_default()
}

/// The call `call_id` in `conversation`.
pub(super) fn find_call<'a>(call_id: &str, conversation: &'a Conversation) -> Option<&'a ToolCall> {
    conversation
        .entries()
        .iter()
        .rev()
        .find_map(|entry| match &entry.kind {
            EntryKind::Assistant {
                tool_calls: Some(calls),
                ..
            } => calls.iter().find(|call| call.id == call_id),
            _ => None,
        })
}

/// The fields of a record that tell the user which one it is.
pub(super) const RECORD_KEYS: &[(&str, &str)] = &[
    ("subject", "Subject"),
    ("title", "Title"),
    ("summary", "Title"),
    ("content", "Task"),
    ("name", "Name"),
    ("from", "From"),
    ("sender", "From"),
];

pub(super) fn records(call: &ToolCall, conversation: &Conversation) -> Vec<ApprovalField> {
    let ids = record_ids(&call.arguments);
    if ids.is_empty() {
        return Vec::new();
    }
    let results: Vec<Value> = conversation
        .entries()
        .iter()
        .rev()
        .filter_map(|entry| match &entry.kind {
            EntryKind::Tool { output, .. } => parse_output(output),
            _ => None,
        })
        .collect();
    ids.into_iter()
        .filter_map(|id| results.iter().find_map(|result| find_record(result, id)))
        .take(MAX_RECORDS)
        .flat_map(|record| record_fields(record, call))
        .collect()
}

/// Most records a card reads for one call.
pub(super) const MAX_FETCHED: usize = 3;

/// How to read what an acting call names, for a call that only gives IDs.
pub(super) struct Lookup {
    /// The acting tools, as rules name them.
    pub(super) tools: &'static [&'static str],
    /// The tool that reads one record, Fastmail's by their MCP name.
    read: &'static str,
    /// Arguments carried from the call to the read, as (call's, read's); lists are read per item.
    args: &'static [(&'static str, &'static str)],
}

/// Every acting tool that names a record a tool can read back.
pub(super) const LOOKUPS: &[Lookup] = &[
    Lookup {
        tools: &[
            "FASTMAIL_DELETE_EMAIL",
            "FASTMAIL_ARCHIVE_EMAIL",
            "FASTMAIL_UPDATE_EMAIL",
        ],
        read: "read_email",
        args: &[("ids", "id")],
    },
    Lookup {
        tools: &["FASTMAIL_SEND_EMAIL"],
        read: "read_email",
        args: &[("emailId", "id")],
    },
    Lookup {
        tools: &["GMAIL_MOVE_TO_TRASH"],
        read: "GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID",
        args: &[("message_id", "message_id")],
    },
    Lookup {
        tools: &["GMAIL_BATCH_MODIFY_MESSAGES"],
        read: "GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID",
        args: &[("messageIds", "message_id")],
    },
    Lookup {
        tools: &["GMAIL_REPLY_TO_THREAD"],
        read: "GMAIL_FETCH_MESSAGE_BY_THREAD_ID",
        args: &[("thread_id", "thread_id")],
    },
    Lookup {
        tools: &["GOOGLECALENDAR_PATCH_EVENT", "GOOGLECALENDAR_DELETE_EVENT"],
        read: "GOOGLECALENDAR_EVENTS_GET",
        args: &[("event_id", "event_id"), ("calendar_id", "calendar_id")],
    },
    Lookup {
        tools: &["OUTLOOK_REPLY_EMAIL", "OUTLOOK_MOVE_MESSAGE"],
        read: "OUTLOOK_GET_MESSAGE",
        args: &[("message_id", "message_id")],
    },
    Lookup {
        tools: &[
            "OUTLOOK_ACCEPT_EVENT",
            "OUTLOOK_DECLINE_EVENT",
            "OUTLOOK_UPDATE_CALENDAR_EVENT",
            "OUTLOOK_DELETE_CALENDAR_EVENT",
        ],
        read: "OUTLOOK_GET_EVENT",
        args: &[("event_id", "event_id")],
    },
    Lookup {
        tools: &["NOTION_UPDATE_PAGE"],
        read: "NOTION_RETRIEVE_PAGE",
        args: &[("page_id", "page_id")],
    },
    Lookup {
        tools: &["NOTION_ADD_MULTIPLE_PAGE_CONTENT"],
        read: "NOTION_RETRIEVE_PAGE",
        args: &[("parent_block_id", "page_id")],
    },
    Lookup {
        tools: &["ONE_DRIVE_MOVE_ITEM"],
        read: "ONE_DRIVE_GET_ITEM",
        args: &[("itemId", "item_id"), ("driveId", "drive_id")],
    },
    Lookup {
        tools: &["ONE_DRIVE_CREATE_LINK"],
        read: "ONE_DRIVE_GET_ITEM",
        args: &[("item_id", "item_id"), ("drive_id", "drive_id")],
    },
];

/// What the records of `call_id` are, from earlier results or read back with the app's tool.
pub(crate) async fn describe_call(
    user: &User,
    call_id: &str,
    conversation: &Conversation,
) -> Vec<ApprovalField> {
    let shown = describe_records(call_id, conversation);
    if !shown.is_empty() {
        return shown;
    }
    match find_call(call_id, conversation) {
        Some(call) => read_back(user, call).await,
        None => Vec::new(),
    }
}

/// What the records `call` names are, read back with the app's own tool.
pub(crate) async fn read_back(user: &User, call: &ToolCall) -> Vec<ApprovalField> {
    let tool = ToolSlug::new(&call.name);
    let Some(lookup) = LOOKUPS
        .iter()
        .find(|lookup| lookup.tools.contains(&tool.as_str()))
    else {
        return Vec::new();
    };
    // A Fastmail call reads back from the account it acts in.
    let account = call.arguments.get("account").and_then(Value::as_str);
    let mut shown = Vec::new();
    for args in read_args(lookup, &call.arguments) {
        match read_record(user, lookup.read, account, args.clone()).await {
            Ok(found) => {
                // The record is the object holding the ID it was read by;
                // a read that answers with the record alone is that object.
                let id = args.values().find_map(Value::as_str).unwrap_or_default();
                shown.extend(record_fields(
                    find_record(&found, id).unwrap_or(&found),
                    call,
                ));
            }
            Err(error) => log::debug!("approval: unable to read {}: {error:#}", lookup.read),
        }
    }
    shown
}

/// The arguments for each read `lookup` makes, one per item for a list; none when something is missing.
pub(super) fn read_args(lookup: &Lookup, args: &Value) -> Vec<Map<String, Value>> {
    let mut shared = Map::new();
    let mut listed: Option<(&str, &Vec<Value>)> = None;
    for &(from, to) in lookup.args {
        match args.get(from) {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => listed = Some((to, items)),
            Some(value) => {
                shared.insert(to.to_owned(), value.clone());
            }
        }
    }
    let Some((to, items)) = listed else {
        return if shared.is_empty() {
            Vec::new()
        } else {
            vec![shared]
        };
    };
    items
        .iter()
        .filter(|item| item.as_str().is_some_and(|id| !id.trim().is_empty()))
        .take(MAX_FETCHED)
        .map(|item| {
            let mut args = shared.clone();
            args.insert(to.to_owned(), item.clone());
            args
        })
        .collect()
}

/// Calls `read` on Fastmail's MCP server for a lowercase name, else on Composio.
pub(super) async fn read_record(
    user: &User,
    read: &str,
    account: Option<&str>,
    args: Map<String, Value>,
) -> Result<Value> {
    if read.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        return user.call_fastmail(account, read, Value::Object(args)).await;
    }
    let client = user
        .composio_client()
        .ok_or_else(|| anyhow::anyhow!("Composio is not configured"))?;
    let response = client.execute_tool(read, Value::Object(args)).await?;
    if response.successful == Some(false) {
        anyhow::bail!("{read} failed");
    }
    Ok(response.data)
}

/// The IDs `args` names, such as a message ID or a list of them.
pub(super) fn record_ids(args: &Value) -> Vec<&str> {
    let Value::Object(args) = args else {
        return Vec::new();
    };
    args.iter()
        .filter(|(key, _)| is_id(key))
        .flat_map(|(_, value)| match value {
            Value::String(id) => vec![id.as_str()],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        })
        .filter(|id| !id.trim().is_empty())
        .collect()
}

/// What the sender of the email `call` names is to the user.
pub(super) fn sender_label(call: &ToolCall) -> &'static str {
    match ToolSlug::new(&call.name).as_str() {
        "GMAIL_REPLY_TO_THREAD" | "OUTLOOK_REPLY_EMAIL" => "Replying to",
        "FASTMAIL_SEND_EMAIL" => match call.arguments.get("mode").and_then(Value::as_str) {
            Some("forward") => "Forwarding from",
            _ => "Replying to",
        },
        _ => "From",
    }
}

/// The fields of `record` that tell the user which one it is.
pub(super) fn record_fields(record: &Value, call: &ToolCall) -> Vec<ApprovalField> {
    let mut labels = HashSet::new();
    let mut shown: Vec<ApprovalField> = RECORD_KEYS
        .iter()
        .filter_map(|(key, label)| {
            let value = record_value(record, key)?;
            let label = if *label == "From" {
                sender_label(call)
            } else {
                label
            };
            labels.insert(label).then(|| ApprovalField {
                label: label.to_owned(),
                value: value.chars().take(MAX_VALUE_CHARS).collect(),
            })
        })
        .collect();
    if !labels.contains("Title")
        && let Some(title) = notion_title(record)
    {
        shown.insert(
            0,
            ApprovalField {
                label: "Title".to_owned(),
                value: title.chars().take(MAX_VALUE_CHARS).collect(),
            },
        );
    }
    shown
}

/// A Notion page's title, kept in whichever property has the `title` type.
pub(super) fn notion_title(page: &Value) -> Option<String> {
    let title = page
        .get("properties")?
        .as_object()?
        .values()
        .find(|property| property.get("type").and_then(Value::as_str) == Some("title"))?
        .get("title")?
        .as_array()?
        .iter()
        .filter_map(|part| part.get("plain_text").and_then(Value::as_str))
        .collect::<String>();
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// A tool result as JSON, skipping any text before it.
pub(super) fn parse_output(output: &str) -> Option<Value> {
    let start = output.find(['{', '['])?;
    serde_json::from_str(&output[start..]).ok()
}

/// The object in `value` that has `id` as one of its own values.
pub(super) fn find_record<'a>(value: &'a Value, id: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if map.values().any(|v| v.as_str() == Some(id)) {
                return Some(value);
            }
            map.values().find_map(|v| find_record(v, id))
        }
        Value::Array(items) => items.iter().find_map(|v| find_record(v, id)),
        _ => None,
    }
}

/// `key` of `record` as text: a string, an address, or a list of addresses.
pub(super) fn record_value(record: &Value, key: &str) -> Option<String> {
    let value = record
        .as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))?
        .1;
    let text = match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| address(item).or_else(|| address(item.get("emailAddress")?)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => address(value).or_else(|| address(value.get("emailAddress")?))?,
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// The call's arguments as the user reads them.
pub(super) fn fields(args: &Value) -> Vec<ApprovalField> {
    let Value::Object(args) = args else {
        return Vec::new();
    };
    args.iter()
        .filter_map(|(key, value)| {
            let value = match value {
                // A list of IDs means nothing to the user, but how many it
                // names does, as when a batch trashes hundreds of emails.
                Value::Array(items) if is_id(key) && items.len() > 1 => items.len().to_string(),
                _ if is_id(key) => return None,
                Value::Null => return None,
                Value::String(text) => text.trim().to_owned(),
                Value::Array(items) if items.iter().all(|item| address(item).is_some()) => items
                    .iter()
                    .filter_map(address)
                    .collect::<Vec<_>>()
                    .join(", "),
                Value::Array(items) if items.iter().all(Value::is_string) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            if value.is_empty() || value == "[]" || value == "{}" {
                return None;
            }
            Some(ApprovalField {
                label: field_label(key),
                value: shorten(&value, MAX_VALUE_CHARS),
            })
        })
        .take(MAX_FIELDS)
        .collect()
}

/// `text` cut to `max` characters, with "..." when something was cut.
pub(crate) fn shorten(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

/// A Fastmail `{"name", "email"}` recipient as "Jo <jo@example.com>".
pub(super) fn address(item: &Value) -> Option<String> {
    let email = item
        .get("email")
        .or_else(|| item.get("address"))?
        .as_str()?;
    Some(match item.get("name").and_then(Value::as_str) {
        Some(name) if !name.trim().is_empty() => format!("{} <{email}>", name.trim()),
        _ => email.to_owned(),
    })
}

/// An argument's name as the user reads it: `addLabelIds` becomes "Add labels".
pub(crate) fn field_label(key: &str) -> String {
    let mut words = String::new();
    for c in key.chars() {
        if c == '_' {
            words.push(' ');
        } else if c.is_uppercase() {
            words.push(' ');
            words.extend(c.to_lowercase());
        } else {
            words.push(c);
        }
    }
    let words = words.trim();
    let words = match words.strip_suffix(" ids") {
        Some(listed) => format!("{listed}s"),
        None if words == "ids" => "items".to_owned(),
        None => words.to_owned(),
    };
    capitalize(&words)
}

/// An argument that only names a record, such as `message_id`; label IDs are shown.
pub(crate) fn is_id(key: &str) -> bool {
    if key.to_ascii_lowercase().contains("label") {
        return false;
    }
    let camel = key.ends_with("Id") || key.ends_with("Ids");
    let key = key.to_ascii_lowercase();
    camel || ["id", "ids"].contains(&key.as_str()) || key.ends_with("_id") || key.ends_with("_ids")
}
