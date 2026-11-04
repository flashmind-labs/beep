//! Grants: narrow standing permissions the user approves once for one routine.
//! A call passes only when every argument it gives is held by the grant; these checks, not the model, keep it narrow.

use crate::{
    account::user::User,
    app::{Reach, UserNotice},
    apps::permissions::{self, ApprovalPayload, ToolSlug},
    protocol::{Approval, ApprovalField, Grant, Notice},
    routines::digest,
    store::{GrantRow, NewGrant},
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use flashmind::types::{
    ToolCall,
    tool::{Tool, ToolContext, ToolResult},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};
use uuid::Uuid;

/// The tool the agent asks for a grant with.
pub(crate) const REQUEST_GRANT: &str = "request_grant";

/// Default grant span; every grant expires within a week.
const DEFAULT_HOURS: u32 = 8;
const MAX_HOURS: u32 = 7 * 24;
/// Most calls one run may make under one grant.
const MAX_PER_RUN: u32 = 5;
const MAX_RECIPIENTS: usize = 10;
/// Longest value a grant may pin, such as a thread id.
const MAX_PIN_CHARS: usize = 300;
/// Longest sent text the notice after a use repeats.
const MAX_SENT_CHARS: usize = 2000;

/// A value an argument may take without the grant pinning it.
#[derive(Clone, Copy)]
enum Literal {
    Text(&'static str),
    Flag(bool),
}

impl Literal {
    fn is(self, value: &Value) -> bool {
        match self {
            Literal::Text(text) => value.as_str() == Some(text),
            Literal::Flag(flag) => value.as_bool() == Some(flag),
        }
    }
}

/// How a grantable tool's arguments are held; an unlisted argument puts a call outside every grant.
struct Grantable {
    tool: &'static str,
    /// Who receives. Every address in them must be one the grant allows.
    recipients: &'static [&'static str],
    /// At least one must name a recipient.
    primary: &'static [&'static str],
    /// May be pinned to one exact value; unpinned ones only as `fixed` allows.
    pinnable: &'static [&'static str],
    /// Pins every grant of the tool must have.
    required: &'static [&'static str],
    /// The values an unpinned argument may take, such as `user_id: "me"`.
    fixed: &'static [(&'static str, Literal)],
    /// What the agent writes: the text and how it is formatted.
    free: &'static [&'static str],
    /// The text a call sends, subject first, for the notice after a use.
    text: &'static [&'static str],
    /// One call, as the card counts them.
    noun: &'static str,
}

/// Every tool a grant can name: mail senders whose arguments name the recipients.
const GRANTABLE: &[Grantable] = &[
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

fn grantable(tool: &ToolSlug) -> Option<&'static Grantable> {
    GRANTABLE.iter().find(|spec| spec.tool == tool.as_str())
}

/// What a grant holds a call to, never changed after approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Constraints {
    /// Every address a call may send to, lowercased.
    recipients: BTreeSet<String>,
    /// Arguments a call must give with exactly these values.
    pinned: BTreeMap<String, String>,
}

impl Constraints {
    /// The constraints the agent asked for, or why they cannot be granted.
    fn new(
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
    fn allow(&self, spec: &Grantable, args: &Value) -> bool {
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

fn list_or_none(keys: &[&str]) -> String {
    if keys.is_empty() {
        "nothing".to_owned()
    } else {
        keys.join(", ")
    }
}

/// An argument left empty, which the tool reads as not given.
fn absent(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

/// The lowercased plain addresses `value` names; `None` if any part is not a plain address.
fn addresses(value: &Value) -> Option<Vec<String>> {
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
fn address(text: &str) -> Option<String> {
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
fn object_address(item: &Value) -> Option<String> {
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
fn plain_address(text: &str) -> Option<String> {
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

/// What the agent asks for.
#[derive(Deserialize)]
struct Request {
    routine: String,
    tool: String,
    recipients: Vec<String>,
    #[serde(default)]
    exact: Map<String, Value>,
    #[serde(default)]
    per_run: Option<u32>,
    #[serde(default)]
    hours: Option<u32>,
}

/// A request that can be granted, with the card that asks.
struct Asked {
    routine: Uuid,
    tool: ToolSlug,
    constraints: Constraints,
    per_run: u32,
    hours: u32,
    card: Approval,
}

/// Asks the user for a grant on an approval card. Never given to a routine's own runs, so text it reads cannot ask.
pub(crate) struct RequestGrantTool {
    pub(crate) user: Arc<User>,
    /// The call the user just allowed, which saves its grant.
    pub(crate) approved: Option<String>,
}

#[async_trait]
impl Tool for RequestGrantTool {
    fn name(&self) -> &str {
        REQUEST_GRANT
    }

    fn description(&self) -> &str {
        "Ask the user to allow one routine to act without asking each time, such as \
         replying to Ana in one thread whenever she writes. Call it right after setting up a \
         routine that will send, with the routine's id. The user sees a card with exactly what \
         is allowed and answers once. Each run of the routine may then make `per_run` calls \
         of `tool` without asking, only to `recipients` and only with the `exact` values; any \
         other call asks as usual. It ends when the routine ends, after `hours`, or when the \
         user revokes it. Tools: GMAIL_REPLY_TO_THREAD (exact must pin thread_id), \
         GMAIL_SEND_EMAIL, OUTLOOK_SEND_EMAIL, FASTMAIL_SEND_EMAIL (may pin mode, emailId, \
         account)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "routine": {
                    "type": "string",
                    "description": "The id of the routine the grant is for, as cron_create or cron_list gave it."
                },
                "tool": {
                    "type": "string",
                    "enum": GRANTABLE.iter().map(|spec| spec.tool).collect::<Vec<_>>(),
                    "description": "The one tool the routine may call."
                },
                "recipients": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Every email address the routine may send to, such as [\"ana@example.com\"]."
                },
                "exact": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Arguments the call must give with exactly these values, such as {\"thread_id\": \"18c2...\"}."
                },
                "per_run": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_PER_RUN,
                    "description": "Calls each run may make. Default 1."
                },
                "hours": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_HOURS,
                    "description": "Hours until it expires. Default 8, at most a week."
                }
            },
            "required": ["routine", "tool", "recipients"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let request: Request = match ctx.parse_args(REQUEST_GRANT) {
            Ok(request) => request,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        let asked = match prepare(&self.user, &request).await? {
            Ok(asked) => asked,
            Err(reason) => return Ok(ToolResult::failure(id, format!("Not asked: {reason}"))),
        };
        if self.approved.as_deref() != Some(id) {
            return Ok(ToolResult::interrupt(
                id,
                Arc::new(ApprovalPayload {
                    approval: asked.card,
                }),
            ));
        }
        self.user
            .create_grant(NewGrant {
                routine: asked.routine,
                tool: asked.tool.as_str(),
                constraints: serde_json::to_value(&asked.constraints)?,
                per_run: asked.per_run,
                shown: serde_json::to_value(&asked.card.fields)?,
                call_id: id,
                expires_at: Utc::now() + Duration::hours(i64::from(asked.hours)),
            })
            .await?;
        Ok(ToolResult::success(
            id,
            "The user allowed it. Runs of the routine may now make these calls without asking; \
             anything else still asks. Tell them in a few words.",
        ))
    }

    fn humanize(&self, _args: &Value) -> String {
        "Asking to allow a routine".to_owned()
    }
}

/// Checks a request and builds its card, or says why it cannot be granted.
async fn prepare(user: &User, request: &Request) -> Result<Result<Asked, String>> {
    let tool = ToolSlug::new(&request.tool);
    let Some(spec) = grantable(&tool) else {
        return Ok(Err(format!(
            "{} cannot be granted. Leave it to ask the user each time.",
            request.tool
        )));
    };
    let Some(routine) = Uuid::parse_str(request.routine.trim()).ok() else {
        return Ok(Err("routine is not a routine id.".to_owned()));
    };
    let Some(job) = user.cron_job(routine).await? else {
        return Ok(Err("The user has no routine with that id.".to_owned()));
    };
    if digest::is_digest(&job) {
        return Ok(Err("The digest cannot be given a grant.".to_owned()));
    }
    let per_run = request.per_run.unwrap_or(1);
    if !(1..=MAX_PER_RUN).contains(&per_run) {
        return Ok(Err(format!("per_run must be 1 to {MAX_PER_RUN}.")));
    }
    let hours = request.hours.unwrap_or(DEFAULT_HOURS);
    if !(1..=MAX_HOURS).contains(&hours) {
        return Ok(Err(format!("hours must be 1 to {MAX_HOURS}.")));
    }
    let constraints = match Constraints::new(spec, &request.recipients, &request.exact) {
        Ok(constraints) => constraints,
        Err(reason) => return Ok(Err(reason)),
    };
    let label = job
        .metadata
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .unwrap_or(&job.task);
    let card = card(
        user,
        spec,
        &constraints,
        Card {
            routine: label,
            per_run,
            hours,
        },
    )
    .await;
    Ok(Ok(Asked {
        routine,
        tool,
        constraints,
        per_run,
        hours,
        card,
    }))
}

/// What a grant's card says besides its constraints.
struct Card<'a> {
    routine: &'a str,
    per_run: u32,
    hours: u32,
}

/// The card asking for a grant, with pinned ids read back so the user sees the thread's subject.
async fn card(
    user: &User,
    spec: &Grantable,
    constraints: &Constraints,
    about: Card<'_>,
) -> Approval {
    let tool = ToolSlug::new(spec.tool);
    let (app, action) = match permissions::lookup(tool.as_str()) {
        Some((app, _)) => (app.name(), permissions::action_name(&app, tool.as_str())),
        None => (spec.tool.to_owned(), spec.tool.to_lowercase()),
    };
    let routine: String = about
        .routine
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let calls = if about.per_run == 1 {
        format!("1 {}", spec.noun)
    } else {
        format!("Up to {} {}s", about.per_run, spec.noun)
    };
    let mut fields = vec![
        ApprovalField {
            label: "Routine".to_owned(),
            value: permissions::shorten(&routine, 200),
        },
        ApprovalField {
            label: "Allows".to_owned(),
            value: format!("{calls} each time it runs, without asking"),
        },
        ApprovalField {
            label: "To".to_owned(),
            value: format!(
                "{} only",
                constraints
                    .recipients
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    ];
    for (key, value) in &constraints.pinned {
        let label = permissions::field_label(key);
        fields.push(if permissions::is_id(key) {
            ApprovalField {
                label: "Only in".to_owned(),
                value: format!(
                    "this {}",
                    label
                        .trim_end_matches(" id")
                        .trim_end_matches("Id")
                        .to_lowercase()
                ),
            }
        } else {
            ApprovalField {
                label,
                value: value.clone(),
            }
        });
    }
    if !constraints.pinned.is_empty() {
        let named = ToolCall {
            id: String::new(),
            name: spec.tool.to_owned(),
            arguments: Value::Object(
                constraints
                    .pinned
                    .iter()
                    .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                    .collect(),
            ),
        };
        fields.extend(permissions::read_back(user, &named).await);
    }
    fields.push(ApprovalField {
        label: "Ends".to_owned(),
        value: format!("When the routine ends, or in {}", span(about.hours)),
    });
    Approval {
        app,
        action: format!("{action} for a routine"),
        fields,
    }
}

/// A grant's lifetime as the card says it, such as "2 days and 6 hours".
fn span(hours: u32) -> String {
    let unit = |n: u32, one: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    match (hours / 24, hours % 24) {
        (0, h) => unit(h, "hour"),
        (d, 0) => unit(d, "day"),
        (d, h) => format!("{} and {}", unit(d, "day"), unit(h, "hour")),
    }
}

/// A call a grant let through, for the routine's chat.
pub(crate) struct Used {
    pub(crate) at: DateTime<Utc>,
    pub(crate) text: String,
}

/// The grants of one routine's run: which calls they let through, and what those did.
pub(crate) struct RoutineGrants {
    user: Arc<User>,
    routine: Uuid,
    run: String,
    chat: Uuid,
    used: Mutex<Vec<Used>>,
}

impl RoutineGrants {
    pub(crate) fn new(user: Arc<User>, routine: Uuid, run: &str, chat: Uuid) -> Self {
        Self {
            user,
            routine,
            run: run.to_owned(),
            chat,
            used: Mutex::default(),
        }
    }

    /// The grant in force that lets this call through, recording its use; `None` when the call must ask.
    pub(crate) async fn claim(&self, tool: &str, args: &Value, call_id: &str) -> Option<GrantRow> {
        let slug = ToolSlug::new(tool);
        let spec = grantable(&slug)?;
        let grants = match self.user.routine_grants(self.routine, slug.as_str()).await {
            Ok(grants) => grants,
            Err(error) => {
                log::warn!("grants: unable to load, asking instead: {error:#}");
                return None;
            }
        };
        for grant in grants {
            let constraints: Constraints = match serde_json::from_value(grant.constraints.clone()) {
                Ok(constraints) => constraints,
                Err(error) => {
                    log::warn!(
                        "grants: unreadable constraints; grant={} error={error}",
                        grant.id
                    );
                    continue;
                }
            };
            if !constraints.allow(spec, args) {
                continue;
            }
            match self.user.use_grant(grant.id, &self.run, call_id).await {
                Ok(true) => return Some(grant),
                Ok(false) => {}
                Err(error) => {
                    log::warn!(
                        "grants: unable to record a use; grant={} error={error:#}",
                        grant.id
                    )
                }
            }
        }
        None
    }

    /// Tells the user what a granted call did: a notification now, a chat message when the run ends.
    pub(crate) fn report(&self, grant: &GrantRow, tool: &str, args: &Value) {
        let slug = ToolSlug::new(tool);
        let (app, action) = match permissions::lookup(slug.as_str()) {
            Some((app, _)) => {
                let action = permissions::action_name(&app, slug.as_str());
                (app.name(), action)
            }
            None => (tool.to_owned(), tool.to_lowercase()),
        };
        let text = sent(&slug, args);
        let title = format!("{app}: {action}");
        let body = format!(
            "Done without asking, as you allowed for \"{}\".\n{text}",
            permissions::shorten(&grant.routine_name, 120)
        );
        self.used
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Used {
                at: Utc::now(),
                text: format!("{title}\n{body}"),
            });
        // Nobody listening only means no device is connected; the chat
        // message still says what was done.
        let _ = self.user.notifications.send(UserNotice {
            user: self.user.id().to_owned(),
            notice: Notice {
                title,
                body,
                session_id: self.chat.to_string(),
                app: None,
                approval: None,
                changed: false,
                group: false,
            },
            desktop: None,
            to: Reach::Everyone,
        });
    }

    /// The calls let through so far, taken.
    pub(crate) fn take_used(&self) -> Vec<Used> {
        std::mem::take(&mut *self.used.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Who a call sent to and what it said.
fn sent(tool: &ToolSlug, args: &Value) -> String {
    let Some(spec) = grantable(tool) else {
        return String::new();
    };
    let mut to: Vec<String> = Vec::new();
    for key in spec.recipients {
        for address in args.get(*key).and_then(addresses).unwrap_or_default() {
            if !to.contains(&address) {
                to.push(address);
            }
        }
    }
    let mut lines = vec![format!("To: {}", to.join(", "))];
    for key in spec.text {
        if let Some(text) = args.get(*key).and_then(Value::as_str).map(str::trim)
            && !text.is_empty()
        {
            lines.push(format!(
                "{}: {}",
                permissions::field_label(key),
                permissions::shorten(text, MAX_SENT_CHARS)
            ));
        }
    }
    lines.join("\n")
}

/// A grant as the grant list shows it.
pub(crate) fn listed(row: GrantRow) -> Grant {
    let slug = ToolSlug::new(&row.tool);
    let (app, action) = match permissions::lookup(slug.as_str()) {
        Some((app, _)) => {
            let action = permissions::action_name(&app, slug.as_str());
            (app.name(), action)
        }
        None => (row.tool.clone(), row.tool.to_lowercase()),
    };
    let fields = serde_json::from_value(row.shown).unwrap_or_else(|error| {
        log::warn!("grants: unreadable card; grant={} error={error}", row.id);
        Vec::new()
    });
    Grant {
        id: row.id.to_string(),
        routine_id: row.routine,
        routine: row.routine_name,
        app,
        action,
        fields,
        per_run: row.per_run,
        expires_at: SystemTime::from(row.expires_at),
        created_at: SystemTime::from(row.created_at),
        last_used_at: row.last_used_at.map(SystemTime::from),
    }
}

#[cfg(test)]
mod tests;
