//! The project chat's own tools: `project_setup` for checkouts and roles,
//! `project_plan` to make tasks and their issues, and `project_tasks` to
//! list, change, drop, reopen and merge them.

use super::{
    commands::{
        ChecksState, HostCommand, exclude_project_dirs, parse_pr, parse_setup, parse_visibility,
        read_setup,
    },
    fleet::{self, Fleet, Machine, NodeFleet},
    gate::{self, Asks, Gate, GatedExec, GatedProcess, ProjectGate},
    issues::{self, HOST_LIMIT, run_host},
    model::{
        IssueHost, MachinePath, ProjectId, StartRequest, TaskId, TaskState, Visibility, WorkerAgent,
    },
};
use crate::{
    app::App,
    leak::PublicText,
    nodes::SHELL,
    permissions::PROJECT_APP,
    protocol::{Approval, ApprovalField, node::Capability},
    store::{After, CheckoutRow, NewTask, ProjectRow, TaskRow},
    tools::machines::{Machines, command_name, exec::ExecTool, process::ProcessTool},
    user::User,
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, str::FromStr, sync::Arc};
use strum::VariantNames;
use uuid::Uuid;

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
    /// The project's chat.
    pub(crate) chat: Uuid,
    pub(crate) gate: Arc<ProjectGate>,
}

impl ProjectTools {
    /// The project as it is now: a setup call earlier in the turn may have
    /// changed it.
    pub(crate) async fn project(&self) -> Result<ProjectRow> {
        self.user
            .project(self.project)
            .await?
            .ok_or_else(|| anyhow::anyhow!("the project is gone"))
    }

    /// What text bound for the code host must not name: the machines, their
    /// homes, the project's checkouts and the folders the machines share.
    pub(crate) async fn public_text(&self) -> Result<PublicText> {
        let project = self.project().await?;
        let machines = self.fleet.machines().await?;
        fleet::project_public_text(self.fleet.as_ref(), &self.user, &project, &machines).await
    }
}

/// What "Always" on the card for this call saves, or `None` when the card
/// always asks (a merge, a big plan, a host command), so nothing is saved.
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
        "exec" => gate::exec_slug(text("command").unwrap_or_default(), &project.default_branch),
        "process" => (text("action") == Some("stop")).then_some(gate::PROCESS_STOP_SLUG),
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

/// Registers the project tools, plus `exec` behind the gate and `process`
/// when one of the user's machines offers a shell. There is no
/// `code_agent`: workers start through the project's own tools.
pub(crate) fn register(
    registry: &mut ToolRegistry,
    app: &Arc<App>,
    user: &Arc<User>,
    project: &ProjectRow,
    chat: Uuid,
    gate: Arc<ProjectGate>,
) {
    let tools = Arc::new(ProjectTools {
        fleet: Arc::new(NodeFleet {
            app: app.clone(),
            user: user.clone(),
        }),
        user: user.clone(),
        project: project.id,
        chat,
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
    let machines = Machines {
        app: app.clone(),
        user: user.clone(),
    };
    if app
        .nodes
        .of_user(user.id())
        .iter()
        .any(|node| node.offers(SHELL))
    {
        registry.register(Arc::new(GatedExec {
            exec: Arc::new(ExecTool {
                machines: machines.clone(),
            }),
            tools,
        }));
        registry.register(Arc::new(GatedProcess {
            process: Arc::new(ProcessTool { machines }),
            gate,
        }));
    }
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

fn host_label(host: &IssueHost) -> String {
    match host {
        IssueHost::Github => "GitHub".to_owned(),
        IssueHost::Forgejo(host) => format!("Forgejo at {}", host.as_str()),
    }
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

struct SetupTool {
    tools: Arc<ProjectTools>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SetupAction {
    Checkout,
    Home,
    Pin,
    Show,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupArgs {
    action: SetupAction,
    #[serde(default, rename = "in")]
    machine: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl Tool for SetupTool {
    fn name(&self) -> &str {
        SETUP
    }

    fn description(&self) -> &str {
        "Set up where the project lives. `checkout` adds the clone at `path` on the machine `in` \
         names: it must be in a folder that machine shares. It reads the remote, the beep.issues \
         and beep.prs git settings and the default branch, and the first checkout becomes the \
         home one, where outputs are copied. `home` makes another checkout the home one, `pin` \
         makes workers start on the machine `in` names first, and `show` lists the setup."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["checkout", "home", "pin", "show"]},
                "in": {"type": "string", "description": "The machine, by its name in the machines list."},
                "path": {"type": "string", "description": "The checkout's absolute path on that machine, for checkout and home."}
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: SetupArgs = match ctx.parse_args(SETUP) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        if let SetupAction::Checkout = args.action {
            let checkout_card = || {
                let text = |value: &Option<String>| value.clone().unwrap_or_default();
                card(
                    "Add a checkout".to_owned(),
                    vec![
                        field("Machine", text(&args.machine)),
                        field("Path", text(&args.path)),
                    ],
                )
            };
            let gate = self
                .tools
                .gate
                .check(id, CHECKOUT_SLUG, Asks::ByLevel, checkout_card);
            match gate {
                Gate::Run => {}
                Gate::Refuse(reason) => return Ok(ToolResult::success(id, reason)),
                Gate::Park(result) => return Ok(result),
            }
        }
        let done = match args.action {
            SetupAction::Show => self.show().await?,
            action => self.change(action, &args).await?,
        };
        Ok(match done {
            Ok(text) => ToolResult::success(id, text),
            Err(text) => ToolResult::failure(id, text),
        })
    }

    fn humanize(&self, _args: &Value) -> String {
        "Setting up the project".to_owned()
    }
}

impl SetupTool {
    async fn show(&self) -> Result<Result<String, String>> {
        let tools = &self.tools;
        let project = tools.project().await?;
        let machines = tools.fleet.machines().await?;
        let mut text = format!("{}\n", project.name);
        let role = |host: Option<&IssueHost>| host.map_or("kept in Beep".to_owned(), host_label);
        let _ = writeln!(text, "Issues: {}", role(project.roles.issues.as_ref()));
        let _ = writeln!(text, "Pull requests: {}", role(project.roles.prs.as_ref()));
        let visibility: &'static str = project.visibility.into();
        let _ = writeln!(text, "Visibility: {visibility}");
        let _ = writeln!(text, "Default branch: {}", project.default_branch);
        for checkout in tools.user.checkouts(project.id).await? {
            let home = if checkout.home { " (home)" } else { "" };
            let _ = writeln!(
                text,
                "Checkout: {} on {}{home}",
                checkout.path,
                fleet::name_of(&machines, &checkout.machine)
            );
        }
        if let Some(pin) = &project.pin {
            let _ = writeln!(text, "Pinned to: {}", fleet::name_of(&machines, pin));
        }
        Ok(Ok(text))
    }

    async fn change(
        &self,
        action: SetupAction,
        args: &SetupArgs,
    ) -> Result<Result<String, String>> {
        let tools = &self.tools;
        let project = tools.project().await?;
        let machines = tools.fleet.machines().await?;
        let Some(name) = args.machine.as_deref() else {
            return Ok(Err(format!(
                "Name the machine with `in`. {}",
                machine_list(&machines)
            )));
        };
        let Some(machine) = fleet::find(&machines, name) else {
            return Ok(Err(format!(
                "No machine called {name}. {}",
                machine_list(&machines)
            )));
        };
        if let SetupAction::Pin = action {
            tools.user.set_pin(project.id, Some(&machine.id)).await?;
            return Ok(Ok(format!(
                "Workers start on {} first.",
                command_name(&machine.name)
            )));
        }
        let path = match args.path.as_deref().map(MachinePath::parse) {
            Some(Ok(path)) => path,
            Some(Err(error)) => return Ok(Err(format!("Not a usable path: {error}."))),
            None => return Ok(Err("Give the checkout's `path`.".to_owned())),
        };
        if let SetupAction::Home = action {
            return Ok(
                match tools.user.set_home(project.id, &machine.id, &path).await {
                    Ok(()) => Ok(format!(
                        "{path} on {} is the home checkout.",
                        command_name(&machine.name)
                    )),
                    Err(error) => Err(format!("{error:#}.")),
                },
            );
        }
        self.checkout(&project, machine, &path).await
    }

    async fn checkout(
        &self,
        project: &ProjectRow,
        machine: &Machine,
        path: &MachinePath,
    ) -> Result<Result<String, String>> {
        let tools = &self.tools;
        let name = command_name(&machine.name);
        match fleet::shared(tools.fleet.as_ref(), &machine.id, path).await {
            Ok(true) => {}
            Ok(false) => {
                return Ok(Err(format!(
                    "{path} is not in a folder {name} shares, so workers could not copy files \
                     there. Its owner lists shared folders as `files = [\"/path\"]` in \
                     beep-node's node.toml and restarts it."
                )));
            }
            Err(error) => {
                return Ok(Err(format!(
                    "Could not read the folders {name} shares: {error:#}."
                )));
            }
        }
        let Some(shell) = tools.fleet.shell(&machine.id) else {
            return Ok(Err(format!("{name} is offline or offers no shell.")));
        };
        let out = shell.run(&read_setup(path), HOST_LIMIT).await?;
        let setup = match parse_setup(&out.out, &project.name) {
            Ok(setup) => setup,
            Err(error) => return Ok(Err(error.to_string())),
        };
        let visibility = match setup.roles.prs.as_ref().or(setup.roles.issues.as_ref()) {
            Some(host) => {
                let command = HostCommand::Visibility.render(host, &project.name);
                let out = shell.run(&command, HOST_LIMIT).await?;
                parse_visibility(host, &out.out)
            }
            None => project.visibility,
        };
        let excluded = shell.run(&exclude_project_dirs(path), HOST_LIMIT).await?;
        if excluded.code != Some(0) {
            return Ok(Err(format!(
                "Could not keep .worktrees/ and .beep/ out of git there: {}",
                excluded.out.trim()
            )));
        }
        if let Some(host) = &setup.roles.issues
            && let Err(error) =
                run_host(shell.as_ref(), host, project, HostCommand::EnsureLabels).await
        {
            return Ok(Err(format!(
                "Could not make the beep labels on {}: {error:#}. Check that the CLI is \
                 signed in there.",
                host_label(host)
            )));
        }
        let checkouts = tools.user.checkouts(project.id).await?;
        let home = checkouts.iter().all(|checkout| !checkout.home)
            || checkouts.iter().any(|checkout| {
                checkout.home && checkout.machine == machine.id && &checkout.path == path
            });
        tools
            .user
            .add_checkout(
                project.id,
                &CheckoutRow {
                    machine: machine.id,
                    path: path.clone(),
                    home,
                },
            )
            .await?;
        tools
            .user
            .set_project_setup(project.id, &setup.roles, visibility, &setup.default_branch)
            .await?;
        let role = |host: Option<&IssueHost>| host.map_or("kept in Beep".to_owned(), host_label);
        let visibility: &'static str = visibility.into();
        Ok(Ok(format!(
            "Added {path} on {name}{}. Issues: {}. Pull requests: {}. Default branch: {}. The \
             repository is {visibility}.",
            if home { " as the home checkout" } else { "" },
            role(setup.roles.issues.as_ref()),
            role(setup.roles.prs.as_ref()),
            setup.default_branch,
        )))
    }
}

// ---- project_plan ----

struct PlanTool {
    tools: Arc<ProjectTools>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanArgs {
    tasks: Vec<PlannedTask>,
    #[serde(default)]
    start: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlannedTask {
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
enum AfterArg {
    Number(usize),
    Slug(String),
}

/// One task of a plan, checked.
struct Checked {
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
        Ok(match self.plan(id, args).await? {
            Planned::Done(text) => ToolResult::success(id, text),
            Planned::Refused(text) => ToolResult::failure(id, text),
            Planned::Gated(result) => result,
        })
    }

    fn humanize(&self, args: &Value) -> String {
        match args.get("tasks").and_then(Value::as_array).map(Vec::len) {
            Some(1) => "Planning a task".to_owned(),
            Some(n) => format!("Planning {n} tasks"),
            None => "Planning tasks".to_owned(),
        }
    }
}

enum Planned {
    Done(String),
    Refused(String),
    /// Refused or parked by the gate.
    Gated(ToolResult),
}

impl PlanTool {
    async fn plan(&self, id: &str, args: PlanArgs) -> Result<Planned> {
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
                        match issues::file_issue(
                            &tools.user,
                            home.shell.as_ref(),
                            host,
                            &project,
                            task,
                        )
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
    async fn check(
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
                machine,
                agent,
                persona: planned.persona.clone(),
                inputs: Vec::new(),
            },
            after,
        }))
    }
}

fn plural(n: usize) -> String {
    if n == 1 {
        "1 task".to_owned()
    } else {
        format!("{n} tasks")
    }
}

// ---- project_tasks ----

struct TasksTool {
    tools: Arc<ProjectTools>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TasksAction {
    List,
    Update,
    Drop,
    Reopen,
    Merge,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TasksArgs {
    action: TasksAction,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    /// A task's slug, or empty to clear.
    #[serde(default)]
    after: Option<String>,
}

#[async_trait]
impl Tool for TasksTool {
    fn name(&self) -> &str {
        TASKS
    }

    fn description(&self) -> &str {
        "The project's tasks. `list` shows them all. `update` changes a task's title, body or \
         the task it waits for (`after`, a slug; empty clears it) and its issue. `drop` stops \
         its worker and closes its issue as not planned; `reopen` plans a dropped or done task \
         again. `merge` squash-merges the task's pull request after the user approves a card \
         showing the PR and its checks; never merge any other way."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "update", "drop", "reopen", "merge"]},
                "task": {"type": "string", "description": "The task's slug."},
                "title": {"type": "string"},
                "body": {"type": "string"},
                "after": {"type": "string", "description": "The slug of the task it waits for; empty for none."}
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: TasksArgs = match ctx.parse_args(TASKS) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        let project = self.tools.project().await?;
        let tasks = self.tools.user.project_tasks(project.id).await?;
        if let TasksAction::List = args.action {
            return Ok(ToolResult::success(id, list(&tasks)));
        }
        let Some(slug) = args.task.as_deref() else {
            return Ok(ToolResult::failure(id, "Name the task by its slug."));
        };
        let Some(task) = tasks.iter().find(|task| task.slug.as_str() == slug) else {
            let slugs: Vec<&str> = tasks.iter().map(|task| task.slug.as_str()).collect();
            return Ok(ToolResult::failure(
                id,
                format!("There is no task {slug}. Tasks: {}.", slugs.join(", ")),
            ));
        };
        let done = match args.action {
            TasksAction::List => unreachable!("listed above"),
            TasksAction::Update => self.update(id, &project, &tasks, task, &args).await?,
            TasksAction::Drop => self.drop_task(id, &project, task).await?,
            TasksAction::Reopen => self.reopen(id, &project, task).await?,
            TasksAction::Merge => self.merge(id, &project, task).await?,
        };
        Ok(match done {
            Planned::Done(text) => ToolResult::success(id, text),
            Planned::Refused(text) => ToolResult::failure(id, text),
            Planned::Gated(result) => result,
        })
    }

    fn humanize(&self, args: &Value) -> String {
        match args.get("action").and_then(Value::as_str) {
            Some("merge") => "Merging a pull request".to_owned(),
            Some("list") => "Reading the tasks".to_owned(),
            _ => "Updating a task".to_owned(),
        }
    }
}

fn list(tasks: &[TaskRow]) -> String {
    if tasks.is_empty() {
        return "The project has no tasks yet.".to_owned();
    }
    let mut text = String::new();
    for task in tasks {
        let state: &'static str = task.state.into();
        let _ = write!(text, "- {}: {state}, \"{}\"", task.slug, task.title);
        if let Some(after) = task
            .depends_on
            .and_then(|id| tasks.iter().find(|other| other.id == id))
        {
            let _ = write!(text, ", after {}", after.slug);
        }
        if let Some(issue) = &task.issue {
            let _ = write!(text, ", issue #{} {}", issue.number, issue.url);
        }
        if let Some(pr) = &task.pr_url {
            let _ = write!(text, ", PR {pr}");
        }
        text.push('\n');
    }
    text
}

/// What a gate check turned into, when it did not run.
fn gated(gate: Gate) -> Option<Planned> {
    match gate {
        Gate::Run => None,
        Gate::Refuse(reason) => Some(Planned::Refused(reason)),
        Gate::Park(result) => Some(Planned::Gated(result)),
    }
}

/// Whether `task` already waits for `target`, directly or through others.
fn waits_on(tasks: &[TaskRow], task: &TaskRow, target: TaskId) -> bool {
    let mut at = Some(task);
    // Each task is passed at most once, so a loop already saved ends too.
    for _ in 0..=tasks.len() {
        let Some(current) = at else {
            return false;
        };
        if current.id == target {
            return true;
        }
        at = current
            .depends_on
            .and_then(|next| tasks.iter().find(|other| other.id == next));
    }
    false
}

/// The number at the end of a pull request's URL.
fn pr_number(url: &str) -> Option<u64> {
    url.trim_end_matches('/').rsplit('/').next()?.parse().ok()
}

impl TasksTool {
    async fn update(
        &self,
        id: &str,
        project: &ProjectRow,
        tasks: &[TaskRow],
        task: &TaskRow,
        args: &TasksArgs,
    ) -> Result<Planned> {
        let tools = &self.tools;
        let title = args.title.as_deref().map(str::trim);
        let body = args.body.as_deref().map(str::trim);
        if title.is_none() && body.is_none() && args.after.is_none() {
            return Ok(Planned::Refused(
                "Give a new title, body or after.".to_owned(),
            ));
        }
        if title == Some("") {
            return Ok(Planned::Refused("A task needs a title.".to_owned()));
        }
        let after: Option<Option<TaskId>> = match args.after.as_deref().map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(slug) if slug == task.slug.as_str() => {
                return Ok(Planned::Refused(
                    "A task cannot wait for itself.".to_owned(),
                ));
            }
            Some(slug) => match tasks.iter().find(|other| other.slug.as_str() == slug) {
                Some(other) if waits_on(tasks, other, task.id) => {
                    return Ok(Planned::Refused(format!(
                        "{} cannot wait for {slug}: {slug} already waits for it, so neither \
                         would start.",
                        task.slug
                    )));
                }
                Some(other) => Some(Some(other.id)),
                None => return Ok(Planned::Refused(format!("There is no task {slug}."))),
            },
        };
        let new_title = title.unwrap_or(&task.title);
        let new_body = body.unwrap_or(&task.body);
        let hits = HostCommand::EditIssue {
            number: 0,
            title: new_title,
            body: new_body,
        }
        .leaks(&tools.public_text().await?);
        if !hits.is_empty() {
            return Ok(Planned::Refused(leaked(&hits)));
        }
        let update_card = || {
            let mut fields = vec![field("Task", task.slug.to_string())];
            if let Some(title) = title {
                fields.push(field("Title", title));
            }
            if let Some(body) = body {
                fields.push(field("Body", body));
            }
            if let Some(after) = &args.after {
                fields.push(field(
                    "After",
                    if after.is_empty() { "nothing" } else { after },
                ));
            }
            card("Update task".to_owned(), fields)
        };
        if let Some(stop) = gated(
            tools
                .gate
                .check(id, UPDATE_SLUG, Asks::ByLevel, update_card),
        ) {
            return Ok(stop);
        }
        tools.user.update_task_text(task.id, title, body).await?;
        if let Some(after) = after {
            tools.user.set_task_after(task.id, after).await?;
        }
        let mut text = format!("Updated {}.", task.slug);
        if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host)
            && (title.is_some() || body.is_some())
        {
            let command = HostCommand::EditIssue {
                number: issue.number,
                title: new_title,
                body: new_body,
            };
            if let Err(why) = self.on_home(project, host, command).await? {
                let _ = write!(text, " Its issue was not updated: {why}");
            }
        }
        Ok(Planned::Done(text))
    }

    async fn drop_task(&self, id: &str, project: &ProjectRow, task: &TaskRow) -> Result<Planned> {
        let tools = &self.tools;
        if !task.state.is_open() {
            return Ok(Planned::Refused(format!(
                "{} is already closed.",
                task.slug
            )));
        }
        let drop_card = || {
            card(
                "Drop task".to_owned(),
                vec![field("Task", format!("{}: {}", task.slug, task.title))],
            )
        };
        if let Some(stop) = gated(tools.gate.check(id, DROP_SLUG, Asks::ByLevel, drop_card)) {
            return Ok(stop);
        }
        tools
            .user
            .set_task_state(task.id, TaskState::Dropped)
            .await?;
        for worker in tools.user.live_workers().await? {
            if worker.task == task.id {
                super::stop_worker(&tools.user, &worker).await?;
            }
        }
        let mut text = format!("Dropped {}.", task.slug);
        if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) {
            let command = HostCommand::CloseIssue {
                number: issue.number,
                comment: "Dropped: no longer planned.",
                not_planned: true,
            };
            match self.on_home(project, host, command).await? {
                Ok(_) => {
                    let _ = write!(text, " Closed issue #{} as not planned.", issue.number);
                }
                Err(why) => {
                    let _ = write!(text, " Its issue is still open: {why}");
                }
            }
        }
        Ok(Planned::Done(text))
    }

    async fn reopen(&self, id: &str, project: &ProjectRow, task: &TaskRow) -> Result<Planned> {
        let tools = &self.tools;
        if task.state.is_open() {
            return Ok(Planned::Refused(format!("{} is still open.", task.slug)));
        }
        let reopen_card = || {
            card(
                "Reopen task".to_owned(),
                vec![field("Task", format!("{}: {}", task.slug, task.title))],
            )
        };
        if let Some(stop) = gated(
            tools
                .gate
                .check(id, REOPEN_SLUG, Asks::ByLevel, reopen_card),
        ) {
            return Ok(stop);
        }
        tools
            .user
            .set_task_state(task.id, TaskState::Planned)
            .await?;
        let mut text = format!("Reopened {}; it is planned again.", task.slug);
        if let (Some(issue), Some(host)) = (&task.issue, &task.issue_host) {
            let command = HostCommand::ReopenIssue {
                number: issue.number,
            };
            if let Err(why) = self.on_home(project, host, command).await? {
                let _ = write!(text, " Its issue is still closed: {why}");
            }
        }
        Ok(Planned::Done(text))
    }

    async fn merge(&self, id: &str, project: &ProjectRow, task: &TaskRow) -> Result<Planned> {
        let tools = &self.tools;
        let Some(host) = &project.roles.prs else {
            return Ok(Planned::Refused(
                "The project opens no pull requests.".to_owned(),
            ));
        };
        let Some((url, number)) = task
            .pr_url
            .as_deref()
            .and_then(|url| Some((url, pr_number(url)?)))
        else {
            return Ok(Planned::Refused(format!(
                "{} has no pull request yet.",
                task.slug
            )));
        };
        let status = self
            .on_home(project, host, HostCommand::PrStatus { number })
            .await?
            .ok()
            .and_then(|out| parse_pr(host, &out).ok().flatten());
        let merge_card = || {
            let checks = match &status {
                None => "unknown".to_owned(),
                Some(pr) => {
                    let checks: &'static str = match pr.checks {
                        ChecksState::None => "none reported",
                        other => other.into(),
                    };
                    checks.to_owned()
                }
            };
            let mut fields = vec![
                field("Task", format!("{}: {}", task.slug, task.title)),
                field("Pull request", url),
                field("Checks", checks),
            ];
            if let Some(review) = status.as_ref().and_then(|pr| pr.review.clone()) {
                fields.push(field("Review", review.to_lowercase().replace('_', " ")));
            }
            card("Merge pull request".to_owned(), fields)
        };
        if let Some(stop) = gated(tools.gate.check(id, MERGE_SLUG, Asks::Always, merge_card)) {
            return Ok(stop);
        }
        if let Err(why) = self
            .on_home(project, host, HostCommand::MergePr { number })
            .await?
        {
            return Ok(Planned::Refused(format!("The merge failed: {why}")));
        }
        super::finish_task(&tools.user, task).await?;
        Ok(Planned::Done(format!(
            "Merged {url}; {} is done.",
            task.slug
        )))
    }

    /// Runs `command` on the home machine; the error is for the model.
    async fn on_home(
        &self,
        project: &ProjectRow,
        host: &IssueHost,
        command: HostCommand<'_>,
    ) -> Result<Result<String, String>> {
        let tools = &self.tools;
        let home = match fleet::home_shell(tools.fleet.as_ref(), &tools.user, project).await {
            Ok(home) => home,
            Err(why) => return Ok(Err(why)),
        };
        Ok(run_host(home.shell.as_ref(), host, project, command)
            .await
            .map_err(|error| format!("{error:#}.")))
    }
}

#[cfg(test)]
mod tests;
