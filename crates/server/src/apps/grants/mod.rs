//! Grants: narrow standing permissions the user approves once for one routine.
//! A call passes only when every argument it gives is held by the grant; these checks, not the model, keep it narrow.

mod constraints;
mod request;
mod routine;
pub(crate) use constraints::Constraints;
use constraints::{GRANTABLE, Grantable, addresses, grantable};
pub(crate) use request::RequestGrantTool;
pub(crate) use routine::RoutineGrants;

use crate::{
    apps::permissions::{self, ToolSlug},
    protocol::Grant,
    store::GrantRow,
};
use std::time::SystemTime;

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
