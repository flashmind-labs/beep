//! Request handlers, one function per request type.

mod feeds;
mod routes;
mod runs;
use feeds::{
    get_updates, health, hello, stock_chart, stream_notifications, stream_updates, suggest_prompts,
};
pub use routes::router;
use runs::{
    active_run, answer_approval, cancel_run, chat, check_images, device_result, get_events,
    get_run, list_active_runs, speak_notice,
};

pub mod router;
pub mod transport;

use crate::{api::router::Cx, protocol::*};
use chrono::{DateTime, Utc};
use chrono_tz::{Tz, UTC};
use flashmind::cron::{CronSchedule, JobSchedule};
use std::time::Duration;
use uuid::Uuid;

mod agent_accounts;
mod agent_control;
mod agent_found;
mod agent_sessions;
pub(crate) use agent_sessions::machine_sessions;
mod branch_work;
mod credentials;
mod crons;
mod custom_agents;
mod devices;
mod findings;
mod git_items;
mod grants;
mod groups;
mod harnesses;
mod instructions;
mod live;
mod location;
mod machine_search;
mod machine_tools;
mod machine_traffic;
mod machines;
mod memories;
mod mesh;
mod own_nodes;
mod project_groups;
mod project_import;
pub(crate) mod project_sync;
mod project_watch;
mod projects;
mod research;
mod reviews;
mod sessions;
mod settings;
mod spend_report;
mod terminals;
mod threads;
mod transcribe;
mod updates;
mod usage;
mod work;
mod work_diff;
mod work_files;

const SEND_TIMEOUT: Duration = Duration::from_secs(10);

type Reply<T> = Result<T, ApiError>;

/// A chat id from a request; one that is not a UUID names no chat.
fn chat_id(id: &str) -> Reply<Uuid> {
    Uuid::parse_str(id).map_err(|_| ApiError::NotFound {})
}

/// The run, if it belongs to the caller.
async fn owned_run(cx: &Cx, id: &str) -> Reply<Run> {
    cx.user.run(id).await?.ok_or(ApiError::NotFound {})
}

fn found(found: bool) -> Reply<()> {
    if found {
        Ok(())
    } else {
        Err(ApiError::NotFound {})
    }
}

/// When `schedule` next fires, reading a cron expression in `tz_name`.
fn next_run(schedule: &JobSchedule, tz_name: &str) -> Option<DateTime<Utc>> {
    let expression = match schedule {
        JobSchedule::Once(at) => return Some(*at),
        JobSchedule::Cron(expression) => expression,
        JobSchedule::OnWake { .. } => return None,
    };
    let tz: Tz = tz_name.parse().unwrap_or(UTC);
    CronSchedule::parse(expression)
        .ok()?
        .next_after_in_tz(&Utc::now(), tz)
        .map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_op_is_routed_once() {
        super::router().check().unwrap();
    }
}
