//! Using grants during a routine's run.

use super::{Constraints, MAX_SENT_CHARS, addresses, grantable};
use crate::{
    account::user::User,
    app::{Reach, UserNotice},
    apps::permissions::{self, ToolSlug},
    protocol::Notice,
    store::GrantRow,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::{Arc, Mutex, PoisonError};
use uuid::Uuid;

/// A call a grant let through, for the routine's chat.
pub(crate) struct Used {
    pub(crate) at: DateTime<Utc>,
    pub(crate) text: String,
}

/// The grants of one routine's run: which calls they let through, and what those did.
pub(crate) struct RoutineGrants {
    pub(super) user: Arc<User>,
    pub(super) routine: Uuid,
    pub(super) run: String,
    pub(super) chat: Uuid,
    pub(super) used: Mutex<Vec<Used>>,
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
pub(super) fn sent(tool: &ToolSlug, args: &Value) -> String {
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
