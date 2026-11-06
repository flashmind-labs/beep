//! The project chat's tools: `project_setup`, `project_plan` and `project_tasks`.

mod plan;
mod setup;
mod tasks;
use plan::{PlanTool, Planned};
use setup::SetupTool;
use tasks::{TasksTool, gated, on_home};
pub(crate) use tasks::{drop_task, reopen_task};

use super::{
    fleet::{self, Fleet, Machine, RecordedFleet},
    gate::{Asks, ProjectGate},
    model::ProjectId,
};
use crate::{
    account::user::User,
    app::App,
    apps::permissions::PROJECT_APP,
    protocol::{Approval, ApprovalField},
    store::ProjectRow,
    tools::machines::command_name,
};
use anyhow::Result;
use beep_projects::leak::PublicText;
use beep_projects::model::Visibility;
use flashmind::types::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;

const SETUP: &str = "project_setup";
const PLAN: &str = "project_plan";
const TASKS: &str = "project_tasks";

/// What each gated action is saved as when the user allows it always.
const PLAN_SLUG: &str = "PROJECT_PLAN";
const CHECKOUT_SLUG: &str = "PROJECT_SETUP_CHECKOUT";
const UPDATE_SLUG: &str = "PROJECT_TASKS_UPDATE";
const DROP_SLUG: &str = "PROJECT_TASKS_DROP";
const REOPEN_SLUG: &str = "PROJECT_TASKS_REOPEN";
/// Never saved: a merge always asks.
const MERGE_SLUG: &str = "PROJECT_TASKS_MERGE";
/// Never saved: a comment on an issue always asks.
const COMMENT_SLUG: &str = "PROJECT_TASKS_COMMENT";

/// Most tasks one plan makes.
const MAX_PLAN: usize = 20;
/// From this many tasks on, a plan always shows its card first.
const PLAN_CARD_FROM: usize = 3;

/// What every project tool works with.
#[derive(Clone)]
pub(crate) struct ProjectTools {
    pub(crate) fleet: Arc<dyn Fleet>,
    pub(crate) user: Arc<User>,
    pub(crate) project: ProjectId,
    pub(crate) gate: Arc<ProjectGate>,
}

impl ProjectTools {
    /// The project as it is now, since a setup call this turn may have changed it.
    pub(crate) async fn project(&self) -> Result<ProjectRow> {
        self.user
            .project(self.project)
            .await?
            .ok_or_else(|| anyhow::anyhow!("the project is gone"))
    }

    /// What text bound for the code host must not name.
    pub(crate) async fn public_text(&self) -> Result<PublicText> {
        let project = self.project().await?;
        let machines = self.fleet.machines().await?;
        fleet::project_public_text(self.fleet.as_ref(), &self.user, &project, &machines).await
    }
}

/// What "Always" on this call's card saves; `None` when the card always asks.
pub(crate) fn saved_as(tool: &str, args: &Value, project: &ProjectRow) -> Option<String> {
    let text = |key: &str| args.get(key).and_then(Value::as_str);
    let slug = match tool {
        PLAN => {
            let n = args
                .get("tasks")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            (plan_asks(n, project) == Asks::ByLevel).then_some(PLAN_SLUG)
        }
        TASKS => match text("action")? {
            "update" => Some(UPDATE_SLUG),
            "drop" => Some(DROP_SLUG),
            "reopen" => Some(REOPEN_SLUG),
            _ => None,
        },
        SETUP => (text("action") == Some("checkout")).then_some(CHECKOUT_SLUG),
        _ => None,
    };
    slug.map(str::to_owned)
}

/// Whether a plan of `n` tasks asks by the level or always.
fn plan_asks(n: usize, project: &ProjectRow) -> Asks {
    if n >= PLAN_CARD_FROM
        || (project.visibility == Visibility::Public && project.roles.issues.is_some())
    {
        Asks::Always
    } else {
        Asks::ByLevel
    }
}

/// Registers the project tools.
/// No `code_agent`: workers start through the project tools.
pub(crate) fn register(
    registry: &mut ToolRegistry,
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    gate: Arc<ProjectGate>,
) {
    let tools = Arc::new(ProjectTools {
        fleet: Arc::new(RecordedFleet::new(app, user)),
        user: user.clone(),
        project: project.id,
        gate: gate.clone(),
    });

    registry.register(Arc::new(SetupTool {
        tools: tools.clone(),
    }));
    registry.register(Arc::new(PlanTool {
        tools: tools.clone(),
    }));
    registry.register(Arc::new(TasksTool {
        tools: tools.clone(),
    }));
    registry.register(Arc::new(worker::WorkerTool {
        tools: tools.clone(),
        app: app.clone(),
    }));
    registry.register(Arc::new(reply::ReplyTool {
        tools: tools.clone(),
    }));
    registry.register(Arc::new(review::ReviewTool {
        tools: tools.clone(),
    }));
}

fn machine_list(machines: &[Machine]) -> String {
    if machines.is_empty() {
        return "The user has no machines running beep-node.".to_owned();
    }
    let names: Vec<String> = machines
        .iter()
        .map(|machine| command_name(&machine.name))
        .collect();
    format!("Machines: {}.", names.join(", "))
}

fn field(label: impl Into<String>, value: impl Into<String>) -> ApprovalField {
    ApprovalField {
        label: label.into(),
        value: value.into(),
    }
}

fn card(action: String, fields: Vec<ApprovalField>) -> Approval {
    Approval {
        app: PROJECT_APP.name.to_owned(),
        action,
        fields,
    }
}

/// The result for a call with private details in its text.
pub(crate) fn leaked(hits: &[String]) -> String {
    format!(
        "Not done: the text names private details: {}. Rewrite it without machine names, user \
         names, home folders, IP addresses or ports, then call again.",
        hits.join(", ")
    )
}

// ---- project_setup ----

// ---- project_plan ----

// ---- project_tasks ----

mod reply;
mod review;
mod worker;

#[cfg(test)]
mod tests;
