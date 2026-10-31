//! Grants: narrow, standing permissions the user approves once, ahead of
//! time, for one routine. "Whenever Ana replies, reply back" becomes a
//! routine and a grant that lets each of its runs send one reply in that
//! thread, to Ana only, without asking.
//!
//! The agent asks for a grant with [`RequestGrantTool`] while it sets the
//! routine up; the user answers the same approval card any acting call
//! shows. What the card shows is what is saved: the server builds both
//! from the call's arguments, and nothing changes a grant afterwards.
//!
//! When a routine's run makes an acting call that would ask, the
//! permission path first tries [`RoutineGrants::claim`]. A call passes only
//! when every argument it gives is held by the grant: recipients must be
//! among the approved addresses, pinned arguments such as a thread id must
//! be exactly the approved value, and only the text may be written freely.
//! Anything else asks as before. A routine's run reads mail anyone can
//! send, so these checks, not the model, are what keeps a grant narrow.

use crate::{
    app::UserNotice,
    digest,
    permissions::{self, ApprovalPayload, ToolSlug},
    protocol::{Approval, ApprovalField, Grant, Notice},
    store::{GrantRow, NewGrant},
    user::User,
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

/// How long a grant lasts unless the agent asks for another span. Every
/// grant expires: the longest is a week.
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

/// How the arguments of a tool that can be granted are held to a grant.
/// An argument in none of the lists, such as an attachment or a sender,
/// makes a call fall outside every grant.
struct Grantable {
    tool: &'static str,
    /// Who receives. Every address in them must be one the grant allows.
    recipients: &'static [&'static str],
    /// At least one of these must name a recipient, so the tool never falls
    /// back to one the call does not name.
    primary: &'static [&'static str],
    /// May be pinned to one exact value. A call must give every pinned
    /// value, and one the grant leaves unpinned only as `fixed` allows.
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

/// Every tool a grant can name: the ones that send mail and say in their
/// arguments who to. A reply that goes to whoever wrote the original, such
/// as `OUTLOOK_REPLY_EMAIL`, cannot be held to a recipient, so it is left
/// out. Checked against `just tool-schema`.
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

/// What a grant holds a call to. Saved as JSON with the grant and never
/// changed after the user approved it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Constraints {
    /// Every address a call may send to, lowercased.
    recipients: BTreeSet<String>,
    /// Arguments a call must give with exactly these values.
    pinned: BTreeMap<String, String>,
}

impl Constraints {
    /// The constraints the agent asked for, or why `spec`'s tool cannot be
    /// granted with them.
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

/// The addresses `value` names, lowercased: one address or several
/// separated by commas, a list of them, or Fastmail's `{"name", "email"}`
/// objects. `None` when any part is not a plain address, so a call that
/// hides one in a display name or a second separator matches nothing.
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

/// Asks the user for a grant. The first call stops the turn on an approval
/// card; once the user allows it, the same call runs again and saves the
/// grant. Only turns that can set up routines get it, and a routine's own
/// runs do not, so text a routine reads cannot ask for one.
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

/// The card that asks for a grant: the routine, what each run may do, to
/// whom, in which thread, and when it ends. Pinned ids are read back, so
/// the user sees the thread's subject rather than its id.
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

/// A grant's lifetime as the card says it: "8 hours", "1 day", "2 days and
/// 6 hours".
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

/// The grants of one routine's run: which calls they let through, and what
/// those did, for the chat once the run ends.
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

    /// The grant that lets call `call_id` of `tool` with `args` through,
    /// with the use recorded against this run; `None` when no grant in
    /// force matches or the run has used its share, and the call asks. A
    /// grant that cannot be read counts as none.
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

    /// Tells the user what a call `grant` let through did, sent text
    /// included: a notification now, and a message in the routine's chat
    /// once the run ends.
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
mod tests {
    use super::*;
    use crate::{
        cron::BeepCronStore,
        permissions::Approved,
        protocol::PermissionLevel,
        store::{AppPermissionRow, Store, TestDb},
    };
    use flashmind::{
        core::CancellationToken,
        cron::{CronRegistry, JobSchedule},
    };
    use std::{
        collections::HashSet,
        sync::atomic::{AtomicUsize, Ordering},
    };

    const REPLY: &str = "GMAIL_REPLY_TO_THREAD";
    const THREAD: &str = "18c2a9f0d4e5b6a7";

    fn reply_spec() -> &'static Grantable {
        grantable(&ToolSlug::new(REPLY)).expect("Gmail replies can be granted")
    }

    fn ana_in_thread() -> Constraints {
        let exact = json!({"thread_id": THREAD});
        Constraints::new(
            reply_spec(),
            &["Ana@Example.com".to_owned()],
            exact.as_object().expect("an object"),
        )
        .expect("a grant Gmail replies can have")
    }

    fn reply(to: &str) -> Value {
        json!({"thread_id": THREAD, "recipient_email": to, "message_body": "Thanks, see you then."})
    }

    #[test]
    fn a_call_within_the_grant_matches() {
        let spec = reply_spec();
        let grant = ana_in_thread();
        assert!(grant.allow(spec, &reply("ana@example.com")));
        // Addresses compare without case, and a display name is fine.
        assert!(grant.allow(spec, &reply("ANA@example.COM")));
        assert!(grant.allow(spec, &reply("Ana <ana@example.com>")));
        // Formatting and the user's own mailbox are not widening.
        let mut args = reply("ana@example.com");
        args["is_html"] = json!(false);
        args["user_id"] = json!("me");
        args["cc"] = json!([]);
        assert!(grant.allow(spec, &args));
    }

    #[test]
    fn a_call_outside_the_grant_does_not_match() {
        let spec = reply_spec();
        let grant = ana_in_thread();
        let outside = |change: &dyn Fn(&mut Value)| {
            let mut args = reply("ana@example.com");
            change(&mut args);
            !grant.allow(spec, &args)
        };
        assert!(outside(
            &|args| args["recipient_email"] = json!("sam@example.com")
        ));
        assert!(outside(&|args| args["thread_id"] = json!("other-thread")));
        assert!(outside(&|args| {
            args.as_object_mut().expect("an object").remove("thread_id");
        }));
        assert!(outside(&|args| {
            args.as_object_mut()
                .expect("an object")
                .remove("recipient_email");
        }));
        assert!(outside(&|args| args["user_id"] = json!("boss@example.com")));
        assert!(!grant.allow(spec, &json!("not an object")));
    }

    #[test]
    fn arguments_cannot_widen_a_grant() {
        let spec = reply_spec();
        let grant = ana_in_thread();
        let widened = |key: &str, value: Value| {
            let mut args = reply("ana@example.com");
            args[key] = value;
            !grant.allow(spec, &args)
        };
        // More recipients, in any field or form.
        assert!(widened("bcc", json!(["sam@example.com"])));
        assert!(widened("cc", json!(["ana@example.com", "sam@example.com"])));
        assert!(widened("extra_recipients", json!(["sam@example.com"])));
        assert!(widened(
            "recipient_email",
            json!("ana@example.com, sam@example.com")
        ));
        assert!(widened(
            "recipient_email",
            json!("ana@example.com;sam@example.com")
        ));
        assert!(widened(
            "recipient_email",
            json!("ana@example.com <sam@example.com>")
        ));
        assert!(widened(
            "recipient_email",
            json!("Ana <ana@example.com> sam@example.com")
        ));
        assert!(widened("recipient_email", json!(["sam@example.com"])));
        // Arguments the grant says nothing about.
        assert!(widened(
            "attachment",
            json!({"name": "notes.pdf", "s3key": "x"})
        ));
        assert!(widened("from_email", json!("other@example.com")));
        assert!(widened("Thread_Id", json!(THREAD)));
        // The same recipient again is still only Ana.
        let mut args = reply("ana@example.com");
        args["extra_recipients"] = json!(["ANA@example.com"]);
        assert!(grant.allow(spec, &args));
    }

    #[test]
    fn a_grant_can_only_hold_what_its_tool_allows() {
        let spec = reply_spec();
        let ana = ["ana@example.com".to_owned()];
        let pin = |exact: Value| {
            Constraints::new(spec, &ana, exact.as_object().expect("an object")).is_err()
        };
        // A reply grant names its thread.
        assert!(pin(json!({})));
        // Recipients and text are not pinned, and only text pins.
        assert!(pin(json!({"thread_id": THREAD, "bcc": "sam@example.com"})));
        assert!(pin(json!({"thread_id": THREAD, "message_body": "hi"})));
        assert!(pin(json!({"thread_id": ""})));
        assert!(pin(json!({"thread_id": 7})));
        let thread = json!({"thread_id": THREAD});
        let thread = thread.as_object().expect("an object");
        assert!(Constraints::new(spec, &[], thread).is_err());
        assert!(Constraints::new(spec, &["Ana <ana@example.com>".to_owned()], thread).is_err());
        assert!(Constraints::new(spec, &["ana@example".to_owned()], thread).is_err());
        let saved = serde_json::to_value(ana_in_thread()).expect("constraints save");
        assert_eq!(
            saved,
            json!({"recipients": ["ana@example.com"], "pinned": {"thread_id": THREAD}})
        );
    }

    #[test]
    fn fastmail_sends_new_mail_unless_a_reply_is_pinned() {
        let spec = grantable(&ToolSlug::new("FASTMAIL_SEND_EMAIL")).expect("grantable");
        let ana = ["ana@example.com".to_owned()];
        let new_mail = Constraints::new(spec, &ana, &Map::new()).expect("a grant");
        let to_ana = json!([{"name": "Ana", "email": "ana@example.com"}]);
        let send = json!({"to": to_ana, "subject": "Hi", "body": "Hello"});
        assert!(new_mail.allow(spec, &send));
        let mut forward = send.clone();
        forward["mode"] = json!("forward");
        forward["emailId"] = json!("M1");
        assert!(!new_mail.allow(spec, &forward));
        let mut reply_all = send.clone();
        reply_all["replyAll"] = json!(true);
        assert!(!new_mail.allow(spec, &reply_all));
        let tricky =
            json!({"to": [{"name": "sam@example.com", "email": "ana@example.com"}], "body": "x"});
        assert!(!new_mail.allow(spec, &tricky));
        // Without `to`, a reply goes to whoever wrote, which no grant names.
        assert!(!new_mail.allow(spec, &json!({"body": "Hello"})));
    }

    #[test]
    fn a_span_reads_in_days_and_hours() {
        assert_eq!(span(1), "1 hour");
        assert_eq!(span(8), "8 hours");
        assert_eq!(span(24), "1 day");
        assert_eq!(span(54), "2 days and 6 hours");
    }

    #[test]
    fn every_grantable_tool_acts() {
        for spec in GRANTABLE {
            let (app, acts) = permissions::lookup(spec.tool).expect("a known tool");
            assert_ne!(acts, permissions::Acts::Never, "{} does not act", spec.tool);
            assert!(!app.name().is_empty());
            for key in spec.required {
                assert!(spec.pinnable.contains(key), "{} requires {key}", spec.tool);
            }
            for key in spec.primary {
                assert!(spec.recipients.contains(key), "{} names {key}", spec.tool);
            }
        }
    }

    /// Stands in for Gmail's reply tool, counting what it sends.
    struct Reply(Arc<AtomicUsize>);

    #[async_trait]
    impl Tool for Reply {
        fn name(&self) -> &str {
            REPLY
        }
        fn description(&self) -> &str {
            "Reply in a thread."
        }
        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }
        async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult::success(
                ctx.tool_call_id,
                r#"{"successful": true}"#,
            ))
        }
    }

    struct Fixture {
        _db: TestDb,
        store: Store,
        user: Arc<User>,
        routine: Uuid,
        chat: Uuid,
        sent: Arc<AtomicUsize>,
    }

    async fn fixture() -> Fixture {
        let db = TestDb::new().await.expect("a test database");
        let store = db.store().await.expect("a store");
        let user = User::for_test(store.clone(), "alice");
        user.save_app_permissions(&AppPermissionRow {
            app: "gmail".to_owned(),
            level: PermissionLevel::Ask {},
            allowed: Vec::new(),
        })
        .await
        .expect("saved rules");
        let routines = CronRegistry::new(Arc::new(BeepCronStore::for_user(store.clone(), "alice")));
        let routine = routines
            .create_with_metadata(
                JobSchedule::Cron("*/15 * * * *".to_owned()),
                "When Ana replies in the lunch thread, reply back".to_owned(),
                false,
                json!({"user_id": "alice", "label": "Reply to Ana"}),
            )
            .await
            .expect("routine saved");
        Fixture {
            _db: db,
            store,
            user,
            routine: routine.id,
            chat: Uuid::new_v4(),
            sent: Arc::new(AtomicUsize::new(0)),
        }
    }

    impl Fixture {
        /// Asks for the grant, checks the card, and allows it.
        async fn grant(&self) -> Uuid {
            let cancel = CancellationToken::new();
            let args = json!({
                "routine": self.routine.to_string(),
                "tool": REPLY,
                "recipients": ["ana@example.com"],
                "exact": {"thread_id": THREAD},
            });
            let before = self.user.live_grants().await.expect("grants").len();
            let asking = RequestGrantTool {
                user: self.user.clone(),
                approved: None,
            };
            let asked = asking
                .execute(ToolContext::new("ask", args.clone(), None, &cancel))
                .await
                .expect("the tool runs");
            let payload = asked.payload().expect("the turn stops on a card");
            let card = &payload
                .as_any()
                .downcast_ref::<ApprovalPayload>()
                .expect("an approval card")
                .approval;
            let shown: Vec<(&str, &str)> = card
                .fields
                .iter()
                .map(|field| (field.label.as_str(), field.value.as_str()))
                .collect();
            assert_eq!(card.app, "Gmail");
            assert!(shown.contains(&("Routine", "Reply to Ana")), "{shown:?}");
            assert!(shown.contains(&("To", "ana@example.com only")), "{shown:?}");
            assert!(shown.contains(&("Only in", "this thread")), "{shown:?}");
            assert!(
                shown.contains(&("Allows", "1 reply each time it runs, without asking")),
                "{shown:?}"
            );
            assert!(
                shown.contains(&("Ends", "When the routine ends, or in 8 hours")),
                "{shown:?}"
            );
            // Asking saves nothing.
            let after = self.user.live_grants().await.expect("grants").len();
            assert_eq!(after, before);

            let allowed = RequestGrantTool {
                user: self.user.clone(),
                approved: Some("ask".to_owned()),
            };
            let saved = allowed
                .execute(ToolContext::new("ask", args, None, &cancel))
                .await
                .expect("the tool runs");
            assert!(saved.is_success(), "{}", saved.output());
            let grants = self.user.live_grants().await.expect("grants");
            assert_eq!(grants.len(), 1);
            let lasts = grants[0].expires_at - Utc::now();
            assert!(
                lasts > Duration::hours(7) && lasts <= Duration::hours(8),
                "a grant lasts 8 hours by default: {lasts}"
            );
            grants[0].id
        }

        fn run(&self, run: &str) -> Arc<RoutineGrants> {
            Arc::new(RoutineGrants::new(
                self.user.clone(),
                self.routine,
                run,
                self.chat,
            ))
        }

        /// Calls the guarded reply tool in `grants`' run; true when it ran
        /// without asking.
        async fn call(&self, grants: &Arc<RoutineGrants>, call_id: &str, args: Value) -> bool {
            let rules = self.user.app_rules().await.expect("rules");
            let tool = permissions::guard(
                vec![Arc::new(Reply(self.sent.clone()))],
                rules,
                Approved(None),
                &HashSet::new(),
                Some(grants.clone()),
            )
            .pop()
            .expect("the reply tool");
            let before = self.sent.load(Ordering::SeqCst);
            let cancel = CancellationToken::new();
            let result = tool
                .execute(ToolContext::new(call_id, args, None, &cancel))
                .await
                .expect("the tool runs");
            let ran = self.sent.load(Ordering::SeqCst) > before;
            assert_eq!(ran, !result.is_interrupt(), "{}", result.output());
            ran
        }
    }

    #[tokio::test]
    async fn a_grant_lets_one_call_through_per_run() {
        let fixture = fixture().await;
        fixture.grant().await;
        let mut notices = fixture.user.notifications.subscribe();

        let first = fixture.run("run-1");
        assert!(fixture.call(&first, "c1", reply("ana@example.com")).await);
        let notice = notices.try_recv().expect("the user is told").notice;
        assert_eq!(notice.session_id, fixture.chat.to_string());
        assert!(
            notice.body.contains("Thanks, see you then."),
            "{}",
            notice.body
        );
        assert!(notice.body.contains("ana@example.com"), "{}", notice.body);
        let used = first.take_used();
        assert_eq!(used.len(), 1);
        assert!(used[0].text.contains("Thanks, see you then."));

        // A second call in the same run asks.
        assert!(!fixture.call(&first, "c2", reply("ana@example.com")).await);
        assert!(first.take_used().is_empty());

        // The next run gets its own call.
        let second = fixture.run("run-2");
        assert!(fixture.call(&second, "c3", reply("ana@example.com")).await);
        let listed = listed(fixture.user.live_grants().await.expect("grants").remove(0));
        assert!(listed.last_used_at.is_some());
        assert_eq!(listed.app, "Gmail");
    }

    #[tokio::test]
    async fn a_call_outside_the_grant_asks() {
        let fixture = fixture().await;
        fixture.grant().await;
        let run = fixture.run("run-1");
        assert!(!fixture.call(&run, "c1", reply("sam@example.com")).await);
        let mut other_thread = reply("ana@example.com");
        other_thread["thread_id"] = json!("other-thread");
        assert!(!fixture.call(&run, "c2", other_thread).await);
        let mut bcc = reply("ana@example.com");
        bcc["bcc"] = json!(["sam@example.com"]);
        assert!(!fixture.call(&run, "c3", bcc).await);
        // None of those used the run's call.
        assert!(fixture.call(&run, "c4", reply("ana@example.com")).await);
    }

    #[tokio::test]
    async fn another_routine_or_user_has_no_grant() {
        let fixture = fixture().await;
        fixture.grant().await;
        let other_routine = Arc::new(RoutineGrants::new(
            fixture.user.clone(),
            Uuid::new_v4(),
            "run-1",
            fixture.chat,
        ));
        assert!(
            !fixture
                .call(&other_routine, "c1", reply("ana@example.com"))
                .await
        );
        let bob = User::for_test(fixture.store.clone(), "bob");
        let bobs = RoutineGrants::new(bob, fixture.routine, "run-1", fixture.chat);
        assert!(
            bobs.claim(REPLY, &reply("ana@example.com"), "c2")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_expired_grant_asks() {
        let fixture = fixture().await;
        let id = fixture.grant().await;
        fixture.store.expire_grant(id).await.expect("expired");
        assert!(
            !fixture
                .call(&fixture.run("run-1"), "c1", reply("ana@example.com"))
                .await
        );
        assert!(fixture.user.live_grants().await.expect("grants").is_empty());
    }

    #[tokio::test]
    async fn a_revoked_grant_asks() {
        let fixture = fixture().await;
        let id = fixture.grant().await;
        assert!(fixture.user.revoke_grant(id).await.expect("revoked"));
        assert!(!fixture.user.revoke_grant(id).await.expect("runs"));
        assert!(
            !fixture
                .call(&fixture.run("run-1"), "c1", reply("ana@example.com"))
                .await
        );
        assert!(fixture.user.live_grants().await.expect("grants").is_empty());
    }

    #[tokio::test]
    async fn a_grant_ends_with_its_routine() {
        let fixture = fixture().await;
        fixture.grant().await;
        let ended =
            crate::cron::EndRoutineTool::new(fixture.store.clone(), "alice", fixture.routine);
        let cancel = CancellationToken::new();
        ended
            .execute(ToolContext::new("end", json!({}), None, &cancel))
            .await
            .expect("the routine ends");
        assert!(
            !fixture
                .call(&fixture.run("run-1"), "c1", reply("ana@example.com"))
                .await
        );
        assert!(fixture.user.live_grants().await.expect("grants").is_empty());
    }

    #[tokio::test]
    async fn answering_twice_saves_one_grant() {
        let fixture = fixture().await;
        fixture.grant().await;
        fixture.grant().await;
    }

    #[tokio::test]
    async fn a_request_outside_what_can_be_granted_is_refused() {
        let fixture = fixture().await;
        let cancel = CancellationToken::new();
        let tool = RequestGrantTool {
            user: fixture.user.clone(),
            approved: Some("ask".to_owned()),
        };
        let refused = |args: Value| {
            let tool = &tool;
            let cancel = &cancel;
            async move {
                let result = tool
                    .execute(ToolContext::new("ask", args, None, cancel))
                    .await
                    .expect("the tool runs");
                !result.is_success() && !result.is_interrupt()
            }
        };
        let routine = fixture.routine.to_string();
        assert!(refused(json!({"routine": routine, "tool": "GMAIL_MOVE_TO_TRASH", "recipients": ["ana@example.com"]})).await);
        assert!(
            refused(json!({"routine": Uuid::new_v4().to_string(), "tool": REPLY,
            "recipients": ["ana@example.com"], "exact": {"thread_id": THREAD}}))
            .await
        );
        assert!(
            refused(json!({"routine": routine, "tool": REPLY, "recipients": ["ana@example.com"]}))
                .await
        );
        assert!(
            refused(
                json!({"routine": routine, "tool": REPLY, "recipients": ["ana@example.com"],
            "exact": {"thread_id": THREAD}, "per_run": 50})
            )
            .await
        );
        assert!(
            refused(
                json!({"routine": routine, "tool": REPLY, "recipients": ["ana@example.com"],
            "exact": {"thread_id": THREAD}, "hours": MAX_HOURS + 1})
            )
            .await
        );
        assert!(fixture.user.live_grants().await.expect("grants").is_empty());
    }
}
