//! The Beep MCP server's tools (`tools/1`), called by node agents one stream per call.
//! `run_on` and `send_files` act, so under ask first they answer `Reply::Ask`.

use super::{
    Machines, Need,
    agents::{self, Asker, Order},
    command_name, copy,
    exec::ExecTool,
    process::ProcessTool,
    transfer,
};
use crate::{
    apps::permissions::{self, Check, RUN_ON, SEND_FILES, ToolApp},
    nodes::{StartOn, agent_name, describe},
    protocol::node::{
        LinkRx, LinkTx, Open, Opened, agent,
        process::{Reply as TaskReply, Request as TaskRequest},
        sync::{ProjectStatus, State as SyncState},
        tools::{Approved, Field, PROTOCOL, Reply, Request, Tool as Call},
    },
};
use anyhow::{Context, Result};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter, Write},
    time::Duration,
};
use tokio::time::{Instant, sleep, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// How long a node has to say what a stream is for, and to send its call.
const READ_LIMIT: Duration = Duration::from_secs(10);
/// The most output lines a command run returns.
const RUN_TAIL: u64 = 200;
/// The longest a command may run; the default when the call sets none.
const MAX_RUN_SECS: u64 = 1800;
const DEFAULT_RUN_SECS: u64 = 600;
/// How long `sync_project` waits for the machines to settle.
const SYNC_WAIT: Duration = Duration::from_secs(90);
const SYNC_POLL: Duration = Duration::from_secs(1);
/// The most paths one `send_files` takes.
const MAX_SEND: usize = 20;

/// Serves one `tools/1` stream the node `caller` opened.
pub(crate) async fn serve(calls: Calls, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let open: Open = timeout(READ_LIMIT, rx.recv())
        .await
        .context("the stream did not say its protocol")??
        .context("the stream closed before its protocol")?;
    if open.protocol != PROTOCOL {
        let reason = format!("the server does not take {} streams", open.protocol);
        return tx.send(&Opened::Refused { reason }).await;
    }
    tx.send(&Opened::Ready {}).await?;
    let request: Request = timeout(READ_LIMIT, rx.recv())
        .await
        .context("the node sent no call")??
        .context("the stream closed before its call")?;
    let reply = calls.answer(&request).await;
    tx.send(&reply).await
}

/// The calls one node's agents make.
pub(crate) struct Calls {
    pub(crate) machines: Machines,
    /// The node the calls come from, by its key as the store keeps it.
    pub(crate) caller: String,
}

/// A result for the agent, or why the call did not work.
type Outcome = Result<String, String>;

impl Calls {
    /// What the user's rules say, then the call's result.
    pub(crate) async fn answer(&self, request: &Request) -> Reply {
        let args: Value = match serde_json::from_str(&request.args) {
            Ok(args) => args,
            Err(e) => return done(Err(format!("The arguments are not JSON: {e}"))),
        };
        if let Some(reply) = self.ask_first(request, &args).await {
            return reply;
        }
        log::info!(
            "agent tool call; user={} session={} tool={}",
            self.machines.user.id(),
            request.session,
            request.tool
        );
        done(self.run(request.tool, args).await)
    }

    /// The refusal or ask card that stops an acting call; `None` lets it run.
    async fn ask_first(&self, request: &Request, args: &Value) -> Option<Reply> {
        let slug = match request.tool {
            Call::RunOn => RUN_ON,
            Call::SendFiles => SEND_FILES,
            _ => return None,
        };
        let user = &self.machines.user;
        let rules = match user.app_rules().await {
            Ok(rules) => rules,
            Err(e) => return Some(done(Err(format!("Could not read the user's rules: {e:#}")))),
        };
        match (
            permissions::check(&rules, &ToolApp::Machines, slug, args),
            request.approved,
        ) {
            (Check::Run, _) => None,
            (Check::Refuse(text), _) => Some(done(Err(text))),
            (Check::Ask(approval), Approved::No) => Some(Reply::Ask {
                app: approval.app,
                action: approval.action,
                fields: approval
                    .fields
                    .into_iter()
                    .map(|field| Field {
                        label: field.label,
                        value: field.value,
                    })
                    .collect(),
            }),
            (Check::Ask(_), Approved::Once) => None,
            (Check::Ask(_), Approved::Always) => {
                if let Err(e) = permissions::allow_always(user, slug).await {
                    log::warn!("allow always not kept; tool={slug} error={e:#}");
                }
                None
            }
        }
    }

    async fn run(&self, tool: Call, args: Value) -> Outcome {
        match tool {
            Call::ListMachines => self.list_machines().await,
            Call::RunOn => self.run_on(parse(args)?).await,
            Call::SyncProject => self.sync_project(parse(args)?).await,
            Call::SendFiles => self.send_files(parse(args)?).await,
            Call::JobStatus => self.job(parse::<JobArgs>(args)?.id, false).await,
            Call::Cancel => self.job(parse::<JobArgs>(args)?.id, true).await,
            Call::Unknown => Err("This server does not know that tool; it needs an update.".into()),
        }
    }

    async fn list_machines(&self) -> Outcome {
        let rows = self
            .machines
            .user
            .nodes()
            .await
            .map_err(|e| format!("Could not list the user's machines: {e:#}"))?;
        if rows.is_empty() {
            return Ok("The user has no machines running beep-node.".into());
        }
        let mut out = String::from(
            "The user's machines, by the name the other tools take. Pick one whose OS and \
             `Can` list fit the work:\n",
        );
        for row in &rows {
            let online = self.machines.online(row);
            let mut facts = vec![
                if online.is_some() {
                    "online"
                } else {
                    "offline"
                }
                .to_owned(),
            ];
            if row.endpoint == self.caller {
                facts.push("this machine, where you run".into());
            }
            if online.as_ref().is_some_and(|node| node.workspace.is_some()) {
                facts.push("the user's environment".into());
            }
            let agents: Vec<String> = online
                .iter()
                .flat_map(|node| node.offers.iter())
                .filter_map(|offer| agent::agent_of(offer))
                .map(|agent| format!("{} ({agent})", agent_name(agent)))
                .collect();
            if !agents.is_empty() {
                facts.push(format!("coding agents: {}", agents.join(", ")));
            }
            let _ = writeln!(out, "\n- {}: {}", command_name(&row.name), facts.join("; "));
            if let Some(env) = &row.env {
                for line in describe(env).lines() {
                    let _ = writeln!(out, "  {line}");
                }
            }
        }
        Ok(out.trim_end().to_owned())
    }

    async fn run_on(&self, args: RunOnArgs) -> Outcome {
        match (&args.command, &args.prompt) {
            (Some(command), None) if !command.trim().is_empty() => {
                if args.background {
                    self.start_task(&args, command).await
                } else {
                    self.run_command(&args, command).await
                }
            }
            (None, Some(prompt)) if !prompt.trim().is_empty() => {
                self.start_agent(&args, prompt).await
            }
            _ => Err("Give either `command` or `prompt`, not both.".into()),
        }
    }

    async fn run_command(&self, args: &RunOnArgs, command: &str) -> Outcome {
        let command = in_cwd(args.cwd.as_deref(), command)?;
        let secs = args
            .timeout_secs
            .unwrap_or(DEFAULT_RUN_SECS)
            .clamp(1, MAX_RUN_SECS);
        let exec = ExecTool {
            machines: self.machines.clone(),
        };
        let args = json!({
            "command": command,
            "timeout_secs": secs,
            "tail": RUN_TAIL,
            "in": args.machine,
        });
        outcome(call_tool(&exec, args).await)
    }

    async fn start_task(&self, args: &RunOnArgs, command: &str) -> Outcome {
        let picked = self.machines.pick(Some(&args.machine), Need::Shell).await?;
        let cwd = match args.cwd.as_deref() {
            Some(cwd) => Some(
                copy::on_machine(cwd, picked.cwd(), picked.home.as_deref())
                    .map_err(|e| format!("{e:#}"))?,
            ),
            None => picked.cwd().map(str::to_owned),
        };
        let request = TaskRequest::Start {
            command: command.to_owned(),
            cwd,
        };
        match picked.online.tasks(&request).await {
            Ok(TaskReply::Started { id }) => {
                let job = Job::Task {
                    machine: picked.shown(),
                    id: id.0,
                };
                Ok(format!(
                    "Started it in the background on {}. Job id: {job}. Check it with \
                     job_status, stop it with cancel.",
                    picked.shown()
                ))
            }
            Ok(TaskReply::Error { message }) => Err(message),
            Ok(other) => Err(format!("{} answered {other:?}", picked.shown())),
            Err(e) => Err(format!("{e:#}")),
        }
    }

    async fn start_agent(&self, args: &RunOnArgs, prompt: &str) -> Outcome {
        let need = Need::Agent(args.agent.as_deref());
        let picked = self.machines.pick(Some(&args.machine), need).await?;
        let offered: Vec<&str> = picked
            .online
            .offers
            .iter()
            .filter_map(|offer| agent::agent_of(offer))
            .collect();
        let agent = match (args.agent.as_deref(), &offered[..]) {
            (Some(agent), _) => agent,
            (None, [only]) => only,
            (None, []) => return Err(format!("{} offers no coding agent.", picked.shown())),
            (None, many) => {
                return Err(format!(
                    "{} has several coding agents; name one in `agent`: {}.",
                    picked.shown(),
                    many.join(", ")
                ));
            }
        };
        let link = picked
            .online
            .start_agent(
                agent,
                args.cwd.clone(),
                prompt.to_owned(),
                StartOn::default(),
            )
            .await
            .map_err(|e| format!("Could not start {}: {e:#}", agent_name(agent)))?;
        let session = link.now().info.id;
        let job = Job::Agent {
            machine: picked.shown(),
            agent: agent.to_owned(),
            session,
        };
        Ok(format!(
            "Started {} on {} with the prompt. Job id: {job}. It runs on its own; check it \
             with job_status, stop its turn with cancel. The user sees it in Beep.",
            agent_name(agent),
            picked.shown()
        ))
    }

    async fn job(&self, id: String, cancel: bool) -> Outcome {
        match Job::parse(&id)? {
            Job::Task { machine, id } => {
                let process = ProcessTool {
                    machines: self.machines.clone(),
                };
                let action = if cancel { "stop" } else { "status" };
                let args = json!({ "action": action, "id": id, "in": machine });
                outcome(call_tool(&process, args).await)
            }
            Job::Agent {
                machine,
                agent,
                session,
            } => {
                let picked = self
                    .machines
                    .pick(Some(&machine), Need::Agent(Some(&agent)))
                    .await?;
                let order = if cancel {
                    Order::Cancel { id: session }
                } else {
                    Order::Status {
                        id: Some(session),
                        wait: None,
                    }
                };
                // Status and cancel follow nothing, so no chat is needed.
                let asker = Asker {
                    app: &self.machines.app,
                    user: &self.machines.user,
                    chat: Uuid::nil(),
                    endpoint: &picked.endpoint,
                };
                agents::run(&picked.online, &picked.shown(), &agent, order, &asker).await
            }
        }
    }

    async fn sync_project(&self, args: SyncArgs) -> Outcome {
        let user = &self.machines.user;
        let all = user
            .project_syncs()
            .await
            .map_err(|e| format!("Could not read the synced projects: {e:#}"))?;
        let rows: Vec<_> = all
            .iter()
            .filter(|row| row.project.eq_ignore_ascii_case(args.project.trim()))
            .collect();
        let Some(project) = rows.first().map(|row| row.project.clone()) else {
            let mut names: Vec<&str> = all.iter().map(|row| row.project.as_str()).collect();
            names.dedup();
            return Err(if names.is_empty() {
                "The user syncs no projects between machines.".to_owned()
            } else {
                format!(
                    "No synced project called {}. Synced: {}.",
                    args.project,
                    names.join(", ")
                )
            });
        };
        let nodes = user
            .nodes()
            .await
            .map_err(|e| format!("Could not list the user's machines: {e:#}"))?;
        let names: HashMap<&str, String> = nodes
            .iter()
            .map(|row| (row.endpoint.as_str(), command_name(&row.name)))
            .collect();
        let named = |endpoint: &str| {
            names
                .get(endpoint)
                .cloned()
                .unwrap_or_else(|| endpoint.to_owned())
        };
        let wanted: Vec<&str> = match args.machine.as_deref().map(command_name) {
            Some(machine) => {
                let found: Vec<&str> = rows
                    .iter()
                    .filter(|row| named(&row.endpoint) == machine)
                    .map(|row| row.endpoint.as_str())
                    .collect();
                if found.is_empty() {
                    return Err(format!("{machine} does not sync {project}."));
                }
                found
            }
            None => rows.iter().map(|row| row.endpoint.as_str()).collect(),
        };
        // Scanned first where the agent changed files, then on the others.
        let mut order: Vec<&str> = rows.iter().map(|row| row.endpoint.as_str()).collect();
        order.sort_by_key(|endpoint| *endpoint != self.caller);
        let mut notes = Vec::new();
        for endpoint in &order {
            let Some(online) = self.machines.app.nodes.online(endpoint) else {
                continue;
            };
            if online.user != user.id() || !online.sync_rescans() {
                continue;
            }
            if let Err(e) = online.sync_rescan(&project).await {
                notes.push(format!("{}: {e:#}", named(endpoint)));
            }
        }
        let deadline = Instant::now() + SYNC_WAIT;
        loop {
            let mut lines = Vec::new();
            let mut settled = true;
            for endpoint in &wanted {
                let name = named(endpoint);
                let status = match self.machines.app.nodes.online(endpoint) {
                    Some(online) if online.user == user.id() => online
                        .sync_status()
                        .await
                        .map_err(|e| format!("{e:#}"))
                        .map(|all| all.into_iter().find(|status| status.name == project)),
                    _ => {
                        lines.push(format!("- {name}: offline; it syncs once it is back"));
                        continue;
                    }
                };
                match status {
                    Ok(Some(status)) => {
                        settled &= is_settled(&status);
                        lines.push(format!("- {name}: {}", sync_line(&status)));
                    }
                    Ok(None) => lines.push(format!("- {name}: not syncing {project} yet")),
                    Err(e) => lines.push(format!("- {name}: {e}")),
                }
            }
            let waited_out = Instant::now() >= deadline;
            if settled || waited_out {
                let head = if settled {
                    format!("{project} is in sync:")
                } else {
                    format!(
                        "{project} is still syncing after {}s; check again later:",
                        SYNC_WAIT.as_secs()
                    )
                };
                let mut out = format!("{head}\n{}", lines.join("\n"));
                for note in notes {
                    let _ = write!(out, "\nNot scanned on {note}");
                }
                return Ok(out);
            }
            sleep(SYNC_POLL).await;
        }
    }

    async fn send_files(&self, args: SendArgs) -> Outcome {
        if args.paths.is_empty() {
            return Err("`paths` is empty.".into());
        }
        if args.paths.len() > MAX_SEND {
            return Err(format!("Send at most {MAX_SEND} paths at a time."));
        }
        let rows = self
            .machines
            .user
            .nodes()
            .await
            .map_err(|e| format!("Could not list the user's machines: {e:#}"))?;
        let here = rows
            .iter()
            .find(|row| row.endpoint == self.caller)
            .map(|row| row.name.clone())
            .ok_or("This machine is not among the user's machines.")?;
        let source = self.machines.pick(Some(&here), Need::Files).await?;
        let target = self.machines.pick(Some(&args.machine), Need::Files).await?;
        let folder = transfer::folder_on_machine(
            args.dest.as_deref().unwrap_or("~"),
            target.cwd(),
            target.home.as_deref(),
        )
        .map_err(|e| format!("{e:#}"))?;
        let cancel = CancellationToken::new();
        let mut lines = Vec::new();
        let mut failed = false;
        for path in &args.paths {
            let sent = match copy::picked_path(&source, path) {
                Ok(path) => {
                    transfer::send(&source, &path, &target, &folder, &mut |_| {}, &cancel).await
                }
                Err(e) => Err(e),
            };
            match sent {
                Ok(sent) => lines.push(format!(
                    "- {path}: sent to {}:{} ({})",
                    target.shown(),
                    sent.path,
                    transfer::size(sent.size)
                )),
                Err(e) => {
                    failed = true;
                    lines.push(format!("- {path}: not sent: {e:#}"));
                }
            }
        }
        let text = lines.join("\n");
        if failed { Err(text) } else { Ok(text) }
    }
}

/// A run `run_on` started: `task:<n>@<machine>` or `<agent>:<session>@<machine>`.
#[derive(Debug, PartialEq)]
enum Job {
    Task {
        machine: String,
        id: u32,
    },
    Agent {
        machine: String,
        agent: String,
        session: String,
    },
}

impl Job {
    fn parse(id: &str) -> Result<Self, String> {
        let bad = || format!("{id} is not a job id from run_on.");
        let (what, machine) = id.trim().split_once('@').ok_or_else(bad)?;
        let (kind, rest) = what.split_once(':').ok_or_else(bad)?;
        if machine.is_empty() || rest.is_empty() {
            return Err(bad());
        }
        let machine = machine.to_owned();
        if kind == "task" {
            let id = rest.parse().map_err(|_| bad())?;
            return Ok(Self::Task { machine, id });
        }
        Ok(Self::Agent {
            machine,
            agent: kind.to_owned(),
            session: rest.to_owned(),
        })
    }
}

impl Display for Job {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Task { machine, id } => write!(f, "task:{id}@{machine}"),
            Self::Agent {
                machine,
                agent,
                session,
            } => write!(f, "{agent}:{session}@{machine}"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunOnArgs {
    machine: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    background: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncArgs {
    project: String,
    #[serde(default)]
    machine: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    machine: String,
    paths: Vec<String>,
    #[serde(default)]
    dest: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobArgs {
    id: String,
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("Bad arguments: {e}"))
}

fn done(outcome: Outcome) -> Reply {
    match outcome {
        Ok(text) => Reply::Done {
            text,
            failed: false,
        },
        Err(text) => Reply::Done { text, failed: true },
    }
}

/// `command` run in `cwd`, when given.
fn in_cwd(cwd: Option<&str>, command: &str) -> Result<String, String> {
    match cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) {
        None => Ok(command.to_owned()),
        Some(cwd) => {
            let quoted = shlex::try_quote(cwd).map_err(|_| format!("{cwd} cannot be quoted"))?;
            Ok(format!("cd -- {quoted} && {command}"))
        }
    }
}

/// Runs one of Beep's own machine tools with `args`.
async fn call_tool(tool: &dyn Tool, args: Value) -> anyhow::Result<ToolResult> {
    let cancel = CancellationToken::new();
    tool.execute(ToolContext::new("mcp", args, None, &cancel))
        .await
}

fn outcome(result: anyhow::Result<ToolResult>) -> Outcome {
    match result {
        Ok(result @ ToolResult::Success { .. }) => Ok(result.output()),
        Ok(result) => Err(result.output()),
        Err(e) => Err(format!("{e:#}")),
    }
}

/// Whether a machine has nothing left to bring in line.
fn is_settled(status: &ProjectStatus) -> bool {
    match status.state {
        SyncState::Idle => status.pending == 0 && status.progress.is_none(),
        SyncState::Failed | SyncState::Paused => true,
        _ => false,
    }
}

fn sync_line(status: &ProjectStatus) -> String {
    let mut line = match status.state {
        SyncState::Idle if status.pending == 0 => format!("in sync, {} files", status.files),
        SyncState::Failed => format!(
            "failed: {}",
            status.error.as_deref().unwrap_or("no reason given")
        ),
        SyncState::Paused => "paused by the user".to_owned(),
        _ => match status.progress {
            Some(progress) => format!(
                "syncing, {} of {} files",
                progress.files_done, progress.files_total
            ),
            None => format!("syncing, {} files pending", status.pending),
        },
    };
    if !status.conflicts.is_empty() {
        let _ = write!(line, "; {} conflict copies", status.conflicts.len());
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::watchers::testing::{Harness, Scripted};

    fn calls(harness: &Harness) -> Calls {
        Calls {
            machines: Machines {
                app: harness.app.clone(),
                user: harness.alice.clone(),
            },
            caller: "unknown".into(),
        }
    }

    fn request(tool: Call, args: Value, approved: Approved) -> Request {
        Request {
            session: "a3f9c1".into(),
            tool,
            args: args.to_string(),
            approved,
        }
    }

    #[tokio::test]
    async fn an_acting_call_asks_first_then_runs_once_allowed() {
        let harness = Harness::new(Scripted::default()).await;
        let calls = calls(&harness);
        let args = json!({ "machine": "build-box", "command": "uname" });
        let asked = calls
            .answer(&request(Call::RunOn, args.clone(), Approved::No))
            .await;
        let Reply::Ask {
            app,
            action,
            fields,
        } = asked
        else {
            panic!("run_on ran without asking: {asked:?}");
        };
        assert_eq!((app.as_str(), action.as_str()), ("Your machines", "run on"));
        assert!(
            fields.iter().any(|field| field.value == "uname"),
            "{fields:?}"
        );
        // Allowed, it runs: there is no such machine, which it says.
        let ran = calls
            .answer(&request(Call::RunOn, args, Approved::Once))
            .await;
        assert!(
            matches!(&ran, Reply::Done { failed: true, text } if text.contains("No machine called build-box")),
            "{ran:?}"
        );
    }
}
