//! `project_plan`: turns a plan into tasks, after the user approves it.

use super::{
    super::{
        commands::HostCommand,
        fleet::{self, Machine},
        issues,
        model::{StartRequest, WorkerAgent},
    },
    MAX_PLAN, PLAN, PLAN_SLUG, ProjectTools, card, field, gated, leaked, machine_list, plan_asks,
};
use crate::{
    protocol::node::Capability,
    store::{After, NewTask, ProjectRow},
    tools::machines::command_name,
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, str::FromStr, sync::Arc};
use strum::VariantNames;

pub(super) struct PlanTool {
    pub(super) tools: Arc<ProjectTools>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanArgs {
    tasks: Vec<PlannedTask>,
    #[serde(default)]
    start: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlannedTask {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    needs: Vec<String>,
    #[serde(default)]
    after: Option<AfterArg>,
    #[serde(default)]
    machine: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    persona: Option<String>,
}

/// A task of the same plan by its number, or one the project has by slug.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum AfterArg {
    Number(usize),
    Slug(String),
}

/// One task of a plan, checked.
pub(super) struct Checked {
    task: NewTask,
    start: StartRequest,
    /// What the card says it waits for.
    after: Option<String>,
}

#[async_trait]
impl Tool for PlanTool {
    fn name(&self) -> &str {
        PLAN
    }

    fn description(&self) -> &str {
        "Make the project's tasks, each one unit of work one worker can finish on one branch, \
         and file an issue for each on the project's issues host. `after` is the number of an \
         earlier task in this plan, or the slug of one the project has. `needs` lists what the \
         machine must offer (gpu, cuda, blender, unreal, xcode, android, docker). The issue \
         text is public: no machine names, user names, home folders, IP addresses or ports. \
         A plan of 3 or more tasks, and any issue on a public repository, shows the user a \
         card first. `start` marks each task to start on a machine; Beep cannot start workers \
         yet, so they wait until it can."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_PLAN,
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "body": {"type": "string", "description": "What to do and how to tell it is done."},
                            "needs": {"type": "array", "items": {"type": "string", "enum": ["gpu", "cuda", "blender", "unreal", "xcode", "android", "docker"]}},
                            "after": {"type": ["integer", "string"], "description": "An earlier task's number in this plan, or a task's slug."},
                            "machine": {"type": "string", "description": "The machine to run it on, when the user named one."},
                            "agent": {"type": "string", "enum": WorkerAgent::VARIANTS},
                            "persona": {"type": "string", "description": "A persona from the project's .agents folder."}
                        },
                        "required": ["title", "body"],
                        "additionalProperties": false
                    }
                },
                "start": {"type": "boolean", "description": "Start each task once made."}
            },
            "required": ["tasks"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: PlanArgs = match ctx.parse_args(PLAN) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        Ok(self.plan(id, args).await?.into_result(id))
    }

    fn humanize(&self, args: &Value) -> String {
        match args.get("tasks").and_then(Value::as_array).map(Vec::len) {
            Some(1) => "Planning a task".to_owned(),
            Some(n) => format!("Planning {n} tasks"),
            None => "Planning tasks".to_owned(),
        }
    }
}

pub(super) enum Planned {
    Done(String),
    Refused(String),
    /// Refused or parked by the gate.
    Gated(ToolResult),
}

impl Planned {
    pub(super) fn into_result(self, id: &str) -> ToolResult {
        match self {
            Planned::Done(text) => ToolResult::success(id, text),
            Planned::Refused(text) => ToolResult::failure(id, text),
            Planned::Gated(result) => result,
        }
    }
}

impl PlanTool {
    pub(super) async fn plan(&self, id: &str, args: PlanArgs) -> Result<Planned> {
        let tools = &self.tools;
        if args.tasks.is_empty() || args.tasks.len() > MAX_PLAN {
            return Ok(Planned::Refused(format!(
                "A plan has 1 to {MAX_PLAN} tasks."
            )));
        }
        let project = tools.project().await?;
        let machines = tools.fleet.machines().await?;
        let mut checked = Vec::with_capacity(args.tasks.len());
        for (at, planned) in args.tasks.iter().enumerate() {
            match self.check(&project, &machines, at, planned).await? {
                Ok(task) => checked.push(task),
                Err(why) => return Ok(Planned::Refused(why)),
            }
        }
        let public_text =
            fleet::project_public_text(tools.fleet.as_ref(), &tools.user, &project, &machines)
                .await?;
        let mut hits: Vec<String> = Vec::new();
        for task in &checked {
            let command = HostCommand::CreateIssue {
                title: &task.task.title,
                body: &task.task.body,
            };
            for hit in command.leaks(&public_text) {
                if !hits.contains(&hit) {
                    hits.push(hit);
                }
            }
        }
        if !hits.is_empty() {
            return Ok(Planned::Refused(leaked(&hits)));
        }

        let asks = plan_asks(checked.len(), &project);
        let plan_card = || {
            let n = checked.len();
            let fields = checked
                .iter()
                .enumerate()
                .map(|(at, task)| {
                    let mut value = task.start.machine.clone().unwrap_or_else(|| {
                        project.pin.as_ref().map_or_else(
                            || "machine picked at start".to_owned(),
                            |pin| fleet::name_of(&machines, pin),
                        )
                    });
                    if let Some(after) = &task.after {
                        let _ = write!(value, ", after {after}");
                    }
                    field(format!("{}. {}", at + 1, task.task.title), value)
                })
                .collect();
            card(
                format!("Create {n} task{}", if n == 1 { "" } else { "s" }),
                fields,
            )
        };
        if let Some(stop) = gated(tools.gate.check(id, PLAN_SLUG, asks, plan_card)) {
            return Ok(stop);
        }

        // The start request is saved with the rows, so a failure makes none.
        let new: Vec<NewTask> = checked
            .iter()
            .map(|task| NewTask {
                start: args.start.then(|| task.start.clone()),
                ..task.task.clone()
            })
            .collect();
        let made = tools.user.create_tasks(project.id, &new).await?;
        let mut text = String::new();
        let mut failed = Vec::new();
        let mut filed = Vec::new();
        if let Some(host) = &project.roles.issues {
            match fleet::home_shell(tools.fleet.as_ref(), &tools.user, &project).await {
                Ok(home) => {
                    for task in &made {
                        match issues::file_issue(&tools.user, home.as_ref(), host, &project, task)
                            .await
                        {
                            Ok(issue) => filed.push(format!("{} (#{})", task.slug, issue.number)),
                            Err(error) => failed.push(format!("{} ({error:#})", task.slug)),
                        }
                    }
                }
                Err(why) => {
                    for task in &made {
                        failed.push(format!("{} ({why})", task.slug));
                    }
                }
            }
        }
        let slugs: Vec<String> = made.iter().map(|task| task.slug.to_string()).collect();
        let _ = write!(text, "Made {}: {}.", plural(made.len()), slugs.join(", "));
        if !filed.is_empty() {
            let _ = write!(text, " Issues: {}.", filed.join(", "));
        }
        if !failed.is_empty() {
            let _ = write!(
                text,
                " No issue yet for {}; Beep tries again on the next turn.",
                failed.join(", ")
            );
        }
        if args.start {
            text.push_str(
                " Each is marked to start, but Beep cannot start workers yet: they wait until \
                 it can.",
            );
        }
        Ok(Planned::Done(text))
    }

    /// The task at `at` of the plan, or why it cannot be made.
    pub(super) async fn check(
        &self,
        project: &ProjectRow,
        machines: &[Machine],
        at: usize,
        planned: &PlannedTask,
    ) -> Result<Result<Checked, String>> {
        let number = at + 1;
        let title = planned.title.trim();
        if title.is_empty() {
            return Ok(Err(format!("Task {number} has no title.")));
        }
        let mut needs = Vec::new();
        for need in &planned.needs {
            match Capability::from_str(need.trim()) {
                Ok(need) => needs.push(need),
                Err(_) => return Ok(Err(format!("Task {number} needs an unknown {need:?}."))),
            }
        }
        let agent = match planned.agent.as_deref().map(WorkerAgent::from_str) {
            None => None,
            Some(Ok(agent)) => Some(agent),
            Some(Err(_)) => {
                return Ok(Err(format!(
                    "Task {number}: the agent is one of {}.",
                    WorkerAgent::VARIANTS.join(", ")
                )));
            }
        };
        let machine = match planned.machine.as_deref() {
            None => None,
            Some(name) => match fleet::find(machines, name) {
                Some(machine) => Some(command_name(&machine.name)),
                None => {
                    return Ok(Err(format!(
                        "Task {number}: no machine called {name}. {}",
                        machine_list(machines)
                    )));
                }
            },
        };
        let (depends_on, after) = match &planned.after {
            None => (None, None),
            Some(AfterArg::Number(n)) if (1..number).contains(n) => {
                (Some(After::Earlier(n - 1)), Some(n.to_string()))
            }
            Some(AfterArg::Number(n)) => {
                return Ok(Err(format!(
                    "Task {number} is after {n}, which is not an earlier task of the plan."
                )));
            }
            Some(AfterArg::Slug(slug)) => {
                match self.tools.user.task_by_slug(project.id, slug).await? {
                    Some(task) => (Some(After::Task(task.id)), Some(task.slug.to_string())),
                    None => return Ok(Err(format!("Task {number}: there is no task {slug}."))),
                }
            }
        };
        Ok(Ok(Checked {
            task: NewTask {
                title: title.to_owned(),
                body: planned.body.trim().to_owned(),
                depends_on,
                needs,
                start: None,
            },
            start: StartRequest {
                base: None,
                machine,
                agent,
                persona: planned.persona.clone(),
                inputs: Vec::new(),
            },
            after,
        }))
    }
}

pub(super) fn plural(n: usize) -> String {
    if n == 1 {
        "1 task".to_owned()
    } else {
        format!("{n} tasks")
    }
}
