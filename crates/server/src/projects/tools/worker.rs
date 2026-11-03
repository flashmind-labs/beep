//! `project_worker`: start a task's worker, steer it and stop it.

use super::{Planned, ProjectTools, card, field, gated};
use crate::{
    app::App,
    projects::{
        fleet,
        gate::Asks,
        model::{InputFile, StartRequest, WorkerAgent, WorkerState},
        worker::{
            self, MAX_INPUTS, Placement, Prepared, StartError, Started, copy_inputs, launch,
            prepare,
        },
    },
    protocol::Approval,
    store::{TaskRow, WorkerRow},
    tools::machines::agents::{self, Asker, DEFAULT_LAST, DEFAULT_WAIT_SECS, Order},
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use strum::VariantNames;

pub(crate) const WORKER: &str = "project_worker";
/// The gate's name for a start card; every start asks, so "Always" saves nothing.
pub(super) const START_SLUG: &str = "PROJECT_WORKER_START";

pub(super) struct WorkerTool {
    pub(super) tools: Arc<ProjectTools>,
    pub(super) app: Arc<App>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Start,
    Status,
    Prompt,
    Answer,
    Stop,
    Transcript,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default, rename = "in")]
    machine: Option<String>,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerArgs {
    action: Action,
    task: String,
    #[serde(default, rename = "in")]
    machine: Option<String>,
    #[serde(default)]
    agent: Option<WorkerAgent>,
    #[serde(default)]
    persona: Option<String>,
    #[serde(default)]
    inputs: Vec<Input>,
    #[serde(default)]
    prompt: Option<String>,
}

/// The start card for `task` at `placement`. Always asks: agents run with permission prompts off.
pub(crate) fn start_card(
    task: &TaskRow,
    machine: &str,
    placement: &Placement,
    inputs: &[InputFile],
) -> Approval {
    let mut fields = vec![
        field("Task", format!("{}: {}", task.slug, task.title)),
        field("Machine", machine),
        field("Agent", placement.agent.to_string()),
    ];
    if let Some(persona) = &placement.persona {
        fields.push(field("Persona", persona.name.clone()));
    }
    fields.push(field(
        "Branch",
        format!("{} from {}", task.slug.branch(), placement.base),
    ));
    if !inputs.is_empty() {
        let paths: Vec<String> = inputs
            .iter()
            .map(|input| match &input.machine {
                Some(machine) => format!("{machine}:{}", input.path),
                None => input.path.clone(),
            })
            .collect();
        fields.push(field("Inputs", paths.join(", ")));
    }
    fields.push(field(
        "Permissions",
        format!(
            "{} runs commands without asking: nothing it does in the worktree is checked first.",
            placement.agent
        ),
    ));
    card(format!("Start {}", task.slug), fields)
}

#[async_trait]
impl Tool for WorkerTool {
    fn name(&self) -> &str {
        WORKER
    }

    fn description(&self) -> &str {
        "A task's worker: a coding agent in its own git worktree on branch beep/<task>. \
         `start` places it on a machine with a checkout (or the one `in` names), with `agent` \
         and a `persona` from the project's .agents folder, and copies `inputs` into the \
         worktree's .beep/incoming/<task>/. A start that has to wait for another task or a \
         free slot is kept and starts later. `status` shows what it is doing, `prompt` sends \
         it more to do (copying `inputs` first), `answer` answers its question, `transcript` \
         shows its last entries and `stop` ends it, leaving the task blocked."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["start", "status", "prompt", "answer", "stop", "transcript"]},
                "task": {"type": "string", "description": "The task's slug."},
                "in": {"type": "string", "description": "start: the machine to run it on, when the user named one."},
                "agent": {"type": "string", "enum": WorkerAgent::VARIANTS},
                "persona": {"type": "string"},
                "inputs": {
                    "type": "array",
                    "maxItems": MAX_INPUTS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "in": {"type": "string", "description": "The machine the file is on; the user's own environment when left out."},
                            "path": {"type": "string"}
                        },
                        "required": ["path"],
                        "additionalProperties": false
                    }
                },
                "prompt": {"type": "string", "description": "prompt and answer: the text to send."}
            },
            "required": ["action", "task"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: WorkerArgs = match ctx.parse_args(WORKER) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        Ok(match self.run(id, args).await? {
            Planned::Done(text) => ToolResult::success(id, text),
            Planned::Refused(text) => ToolResult::failure(id, text),
            Planned::Gated(result) => result,
        })
    }

    fn humanize(&self, args: &Value) -> String {
        let task = args.get("task").and_then(Value::as_str).unwrap_or("a task");
        match args.get("action").and_then(Value::as_str) {
            Some("start") => format!("Starting {task}"),
            Some("stop") => format!("Stopping {task}"),
            Some("prompt" | "answer") => format!("Steering {task}"),
            _ => format!("Checking {task}"),
        }
    }
}

impl WorkerTool {
    async fn run(&self, id: &str, args: WorkerArgs) -> Result<Planned> {
        let tools = &self.tools;
        let project = tools.project().await?;
        let Some(task) = tools.user.task_by_slug(project.id, &args.task).await? else {
            return Ok(Planned::Refused(format!(
                "There is no task {}; project_tasks list shows them.",
                args.task
            )));
        };
        if args.inputs.len() > MAX_INPUTS {
            return Ok(Planned::Refused(format!(
                "At most {MAX_INPUTS} input files go with a task."
            )));
        }
        let inputs: Vec<InputFile> = args
            .inputs
            .iter()
            .map(|input| InputFile {
                machine: input.machine.clone(),
                path: input.path.clone(),
            })
            .collect();
        if let Action::Start = args.action {
            let request = StartRequest {
                base: None,
                machine: args.machine,
                agent: args.agent,
                persona: args.persona,
                inputs,
            };
            return self.start(id, &project, &task, &request).await;
        }
        let Some(worker) = tools
            .user
            .live_workers()
            .await?
            .into_iter()
            .find(|worker| worker.task == task.id)
        else {
            return Ok(Planned::Refused(format!(
                "{} has no running worker; start it first.",
                task.slug
            )));
        };
        if let Action::Stop = args.action {
            worker::stop_worker_on(tools.fleet.as_ref(), &tools.user, &worker).await?;
            return Ok(Planned::Done(format!(
                "Stopped the worker of {}; the task is blocked until it starts again.",
                task.slug
            )));
        }
        let Some(session) = worker.session.clone() else {
            return Ok(Planned::Refused(format!(
                "The worker of {} is still starting.",
                task.slug
            )));
        };
        let machines = tools.fleet.machines().await?;
        let machine = fleet::name_of(&machines, &worker.machine);
        let wait = Duration::from_secs(DEFAULT_WAIT_SECS);
        let text = || args.prompt.clone().unwrap_or_default();
        let mut copied = Vec::new();
        let order = match args.action {
            Action::Status => Order::Status {
                id: Some(session),
                wait: None,
            },
            Action::Transcript => Order::Transcript {
                id: session,
                last: DEFAULT_LAST,
            },
            Action::Answer => Order::Answer {
                id: session,
                text: text(),
                wait,
            },
            Action::Prompt => {
                if args
                    .prompt
                    .as_deref()
                    .is_none_or(|prompt| prompt.trim().is_empty())
                {
                    return Ok(Planned::Refused("Give the prompt to send.".to_owned()));
                }
                let incoming = task.slug.incoming(&worker.folder);
                match copy_inputs(&self.app, &tools.user, &machine, &incoming, &inputs).await {
                    Ok(landed) => copied = landed,
                    Err(why) => return Ok(Planned::Refused(why)),
                }
                Order::Prompt {
                    id: session,
                    text: text(),
                    wait,
                }
            }
            Action::Start | Action::Stop => {
                return Ok(Planned::Refused(
                    "That action does not steer a worker.".to_owned(),
                ));
            }
        };
        let Some(online) = self.online(&worker) else {
            return Ok(Planned::Refused(format!("{machine} is offline.")));
        };
        // Given more to do, it is working again, not idle.
        if matches!(order, Order::Prompt { .. } | Order::Answer { .. }) {
            tools
                .user
                .set_worker_state(worker.id, WorkerState::Running)
                .await?;
        }
        let endpoint = worker.machine.to_string();
        let asker = Asker {
            app: &self.app,
            user: &tools.user,
            chat: tools.chat,
            endpoint: &endpoint,
        };
        let agent: &str = worker.agent.into();
        Ok(
            match agents::run(&online, &machine, agent, order, &asker).await {
                Ok(text) if copied.is_empty() => Planned::Done(text),
                Ok(text) => Planned::Done(format!("Copied {}.\n{text}", copied.join(", "))),
                Err(text) => Planned::Refused(text),
            },
        )
    }

    fn online(&self, worker: &WorkerRow) -> Option<crate::nodes::Online> {
        self.app
            .nodes
            .online(&worker.machine.to_string())
            .filter(|online| online.user == self.tools.user.id())
    }

    async fn start(
        &self,
        id: &str,
        project: &crate::store::ProjectRow,
        task: &TaskRow,
        request: &StartRequest,
    ) -> Result<Planned> {
        let tools = &self.tools;
        if !task.state.is_open() {
            return Ok(Planned::Refused(format!("{} is closed.", task.slug)));
        }
        let prepared = match prepare(&self.app, &tools.user, project, task, request).await {
            Ok(prepared) => prepared,
            Err(error) => return Ok(Planned::Refused(error.to_string())),
        };
        if let Prepared::Ready(placement) = &prepared {
            let approval = start_card(task, &placement.machine_name, placement, &request.inputs);
            if let Some(stop) = gated(tools.gate.check(id, START_SLUG, Asks::Always, || approval)) {
                return Ok(stop);
            }
        }
        Ok(
            match launch(&self.app, &tools.user, project, task, request, prepared).await {
                Ok(Started::Running(worker)) => Planned::Done(format!(
                    "Started {} with {} in {} on branch {}.",
                    task.slug,
                    worker.agent,
                    worker.folder,
                    task.slug.branch()
                )),
                Ok(Started::Queued(why)) => Planned::Done(format!(
                    "{} did not start yet: {why}. It is kept and starts later.",
                    task.slug
                )),
                Err(StartError::Store(error)) => return Err(error),
                Err(error) => Planned::Refused(error.to_string()),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{
        model::{MachinePath, ProjectId, TaskId, TaskSlug, TaskState},
        testing::machine_id,
        worker::ChosenPersona,
    };
    use chrono::Utc;
    use uuid::Uuid;

    fn placement(agent: WorkerAgent) -> Placement {
        Placement {
            machine: machine_id(1),
            machine_name: "build-box".to_owned(),
            checkout: MachinePath::parse("/srv/project/widget").expect("a path"),
            agent,
            persona: Some(ChosenPersona {
                name: "reviewer".to_owned(),
                prompt: "Review.".to_owned(),
                commit: None,
            }),
            base: "main".to_owned(),
        }
    }

    fn task() -> TaskRow {
        TaskRow {
            id: TaskId(Uuid::new_v4()),
            project: ProjectId(Uuid::new_v4()),
            slug: TaskSlug::parse("fix-it").expect("a slug"),
            title: "Fix it".to_owned(),
            body: "Make it pass.".to_owned(),
            state: TaskState::Planned,
            depends_on: None,
            needs: Vec::new(),
            issue_host: None,
            issue: None,
            branch: None,
            pr_url: None,
            start: None,
            pr_seen: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            issue_foreign: false,
            issue_commented: false,
        }
    }

    #[test]
    fn a_start_card_names_where_and_how() {
        let inputs = [InputFile {
            machine: None,
            path: "/srv/in/model.glb".to_owned(),
        }];
        let card = start_card(
            &task(),
            "build-box",
            &placement(WorkerAgent::Claude),
            &inputs,
        );
        assert_eq!(card.action, "Start fix-it");
        let shown: Vec<String> = card
            .fields
            .iter()
            .map(|field| format!("{}: {}", field.label, field.value))
            .collect();
        assert_eq!(
            shown,
            [
                "Task: fix-it: Fix it",
                "Machine: build-box",
                "Agent: claude",
                "Persona: reviewer",
                "Branch: beep/fix-it from main",
                "Inputs: /srv/in/model.glb",
                "Permissions: claude runs commands without asking: nothing it does in the \
                 worktree is checked first.",
            ]
        );
    }

    #[test]
    fn every_start_says_its_agent_runs_without_asking() {
        for name in WorkerAgent::VARIANTS {
            let agent: WorkerAgent = name.parse().expect("an agent");
            let card = start_card(&task(), "build-box", &placement(agent), &[]);
            assert!(
                card.fields.iter().any(|field| field.label == "Permissions"
                    && field
                        .value
                        .starts_with(&format!("{name} runs commands without asking"))),
                "{:?}",
                card.fields
            );
        }
    }
}
