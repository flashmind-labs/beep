//! `projects`: the user's projects, checkouts, open items and agents. Read only.

mod load;
mod render;
use load::{Live, Place, Project, in_project, items_of, live, load, sessions_of, waits_on_user};
use render::{render_list, render_show};

use crate::{account::user::User, app::App};
use async_trait::async_trait;
use beep_projects::model::RepoName;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, sync::Arc, time::SystemTime};

pub(crate) const NAME: &str = "projects";

/// The projects section's first line, also how a chat finds the last one it got.
pub(crate) const HEADER: &str = "The user's projects (`projects` gives their machines, open PRs, \
    issues, tasks and coding sessions):";

/// Projects named in the context and listed by `list`.
const MAX_PROJECTS: usize = 30;

pub(crate) struct ProjectsTool {
    pub(crate) app: Arc<App>,
    pub(crate) user: Arc<User>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    action: Action,
    #[serde(default)]
    project: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Action {
    List,
    Show,
}

#[async_trait]
impl Tool for ProjectsTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "The user's projects (git repositories they imported into Beep). `list` gives each \
         project with its URL, the machines and folders it is checked out in, how many open \
         pull requests and issues it has, the PRs waiting on the user, open tasks and the \
         coding-agent sessions running in it. `show` with `project` gives that project's open \
         PRs with review state and checks, issues, tasks and sessions. PRs and issues are the \
         ones that involve the user: opened by them, assigned to them or asking their review. \
         Use it first for questions about the user's projects, priorities, PRs or issues."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "show"]},
                "project": {
                    "type": "string",
                    "description": "For show: the project as org/repo, or just repo when only one has that name."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, args: &Value) -> String {
        match args.get("project").and_then(Value::as_str) {
            Some(project) => format!("Checking {}", project.trim()),
            None => "Checking your projects".to_owned(),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call = ctx.tool_call_id;
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(call, format!("Bad arguments: {e}"))),
        };
        let projects = load(&self.user).await?;
        let now = SystemTime::now();
        let text = match args.action {
            Action::List => {
                if projects.is_empty() {
                    return Ok(ToolResult::success(
                        call,
                        "The user has no projects. They import one from Projects, or you can \
                         with import_project.",
                    ));
                }
                let live = live(&self.app, &self.user).await;
                render_list(&projects, &live, now)
            }
            Action::Show => {
                let Some(wanted) = args.project.as_deref().map(str::trim) else {
                    return Ok(ToolResult::failure(call, "show needs `project`."));
                };
                let project = match find(&projects, wanted) {
                    Ok(project) => project,
                    Err(why) => return Ok(ToolResult::failure(call, why)),
                };
                let live = live(&self.app, &self.user).await;
                render_show(project, &live, now)
            }
        };
        Ok(ToolResult::success(call, text))
    }
}

/// The context's projects section, names only; `None` without projects.
pub(crate) fn context_block(names: &[RepoName]) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let shown: Vec<String> = names
        .iter()
        .take(MAX_PROJECTS)
        .map(ToString::to_string)
        .collect();
    let mut block = format!("{HEADER} {}", shown.join(", "));
    if names.len() > MAX_PROJECTS {
        let _ = write!(block, ", and {} more", names.len() - MAX_PROJECTS);
    }
    block.push('.');
    Some(block)
}

/// The project `wanted` names: `org/repo`, or a repo name only one has.
fn find<'a>(projects: &'a [Project], wanted: &str) -> Result<&'a Project, String> {
    let names = || {
        let names: Vec<String> = projects
            .iter()
            .take(MAX_PROJECTS)
            .map(|project| project.name.to_string())
            .collect();
        if names.is_empty() {
            "The user has no projects.".to_owned()
        } else {
            format!("Projects: {}.", names.join(", "))
        }
    };
    if let Some(project) = projects
        .iter()
        .find(|project| project.name.to_string().eq_ignore_ascii_case(wanted))
    {
        return Ok(project);
    }
    let by_repo: Vec<&Project> = projects
        .iter()
        .filter(|project| {
            project
                .name
                .to_string()
                .rsplit('/')
                .next()
                .is_some_and(|repo| repo.eq_ignore_ascii_case(wanted))
        })
        .collect();
    match by_repo[..] {
        [only] => Ok(only),
        [] => Err(format!("No project called {wanted}. {}", names())),
        _ => Err(format!(
            "Several projects are called {wanted}. Give org/repo. {}",
            names()
        )),
    }
}

#[cfg(test)]
mod tests;
