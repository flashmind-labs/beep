//! `start_agent` and its companions: an agent starts another agent, of any
//! harness, on any machine of the mesh, and follows it.
//!
//! The child is a normal session linked to the caller (see
//! `agents::family`). On this machine the node starts it itself; on another
//! it sends the order over `tools/1`, with the link, and the other node
//! starts it and checks the limits. An agent reaches only its own children.

use super::machines::{Machine, command_name, machines, over_the_mesh};
use crate::agents::{
    Agents, Command, Session,
    family::{ChildStart, RemoteKid, WAIT_LIMIT, WAIT_POLL, Who},
};
use crate::link::Offers;
use beep_mesh::{Key, Mesh};
use beep_proto::ModelId;
use beep_proto::node::{
    agent::{Entry, EntryKind, ParentLink, SessionInfo, Status},
    mesh_state::{StateValue, SyncSpec},
    tools::{Reply, Request, Tool},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::time::{Instant, sleep, timeout};

/// How long a call to a child on another machine may take.
const REMOTE_LIMIT: Duration = Duration::from_secs(20);
/// The longest title or message the agent reads back.
const MAX_TEXT: usize = 2000;

/// Whether `tool` is one of the agent tools this module answers.
pub(crate) fn handles(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::StartAgent
            | Tool::AgentStatus
            | Tool::SendToAgent
            | Tool::StopAgent
            | Tool::ListAgents
    )
}

/// What an agent reads of one child.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChildView {
    /// The id the other tools take: `<session>@<machine>`.
    pub(crate) id: String,
    /// The id its machine lists it under, which the agent's own id
    /// replaces after a restart: how an app finds its row.
    #[serde(default)]
    pub(crate) key: String,
    pub(crate) harness: String,
    pub(crate) machine: String,
    /// `working`, `waiting_on_user`, `idle` (its turn is done, its last
    /// message is the answer), `ended` or `unreachable`.
    pub(crate) state: String,
    pub(crate) title: Option<String>,
    pub(crate) last_message: String,
    pub(crate) files_changed: usize,
    /// What it waits for the user to allow or answer. Only the user can.
    pub(crate) question: Option<String>,
    pub(crate) turns: u32,
    pub(crate) tokens: u64,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) error: Option<String>,
    /// Said when `wait` ran out of time.
    #[serde(default)]
    pub(crate) note: Option<String>,
}

impl ChildView {
    /// The view of a session as its own node reports it; `machine` and the
    /// id's machine part are set by whoever names the machine.
    pub(crate) fn of(info: &SessionInfo, id: &str) -> Self {
        let state = match info.state.status {
            Status::Ended => "ended",
            Status::Asking => "waiting_on_user",
            Status::Idle if info.turns > 0 => "idle",
            _ => "working",
        };
        Self {
            id: id.to_owned(),
            key: info.id.clone(),
            harness: info.agent.clone(),
            machine: String::new(),
            state: state.to_owned(),
            title: info.title.clone(),
            last_message: crate::agents::cut(&info.last_message, MAX_TEXT),
            files_changed: info.state.files_touched.len(),
            question: info
                .question
                .as_ref()
                .map(|q| crate::agents::cut(&q.text, MAX_TEXT)),
            turns: info.turns,
            tokens: info.tokens,
            cost_usd: info.cost_usd,
            error: info.state.error.clone(),
            note: None,
        }
    }

    fn named(mut self, machine: &str) -> Self {
        self.machine = machine.to_owned();
        if !machine.is_empty() && !self.id.contains('@') {
            self.id = format!("{}@{machine}", self.id);
        }
        self
    }

    fn settled(&self) -> bool {
        self.state != "working"
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartArgs {
    harness: String,
    prompt: String,
    machine: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    #[serde(default)]
    wait: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    id: String,
    text: String,
}

/// What `start_agent` sends the machine that starts the child.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PeerStart {
    pub(crate) harness: String,
    pub(crate) prompt: String,
    pub(crate) cwd: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) parent: ParentLink,
}

/// What a call to a child on another machine carries besides its id.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PeerCall {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) text: Option<String>,
    /// Stop only the running turn, not the session.
    #[serde(default)]
    pub(crate) turn_only: bool,
}

fn fail(text: impl Into<String>) -> Reply {
    Reply::Done {
        text: text.into(),
        failed: true,
    }
}

fn ok(text: impl Into<String>) -> Reply {
    Reply::Done {
        text: text.into(),
        failed: false,
    }
}

fn json_of<T: Serialize>(value: &T) -> Reply {
    match serde_json::to_string(value) {
        Ok(text) => ok(text),
        Err(e) => fail(format!("Could not write the answer: {e}")),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(args: &str) -> Result<T, String> {
    serde_json::from_str(args).map_err(|e| format!("The arguments do not fit: {e}"))
}

/// Answers one agent tool for the session that called it.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    let agents = &offers.agents;
    let Some((session, who)) = agents.caller(&request.session) else {
        return fail("This session is not one this node runs.");
    };
    let reach = Reach {
        agents,
        mesh: agents.mesh(),
        who: &who,
    };
    match request.tool {
        Tool::StartAgent => match parse::<StartArgs>(&request.args) {
            Ok(args) => reach.start(&session, args).await,
            Err(e) => fail(e),
        },
        Tool::AgentStatus => match parse::<IdArgs>(&request.args) {
            Ok(args) => reach.status(&args.id).await,
            Err(e) => fail(e),
        },
        Tool::SendToAgent => match parse::<SendArgs>(&request.args) {
            Ok(args) => reach.send(&args.id, args.text).await,
            Err(e) => fail(e),
        },
        Tool::StopAgent => match parse::<IdArgs>(&request.args) {
            Ok(args) => reach.stop(&args.id).await,
            Err(e) => fail(e),
        },
        Tool::ListAgents => reach.list().await,
        other => fail(format!("{other} is not an agent tool.")),
    }
}

/// The caller, and the ways it has to its children.
struct Reach<'a> {
    agents: &'a Agents,
    mesh: Option<Mesh>,
    who: &'a Who,
}

/// A child's id split into its session and the machine it runs on.
struct ChildId<'a> {
    session: &'a str,
    machine: Option<&'a str>,
}

impl<'a> ChildId<'a> {
    fn parse(text: &'a str) -> Self {
        match text.trim().split_once('@') {
            Some((session, machine)) => Self {
                session,
                machine: Some(machine),
            },
            None => Self {
                session: text.trim(),
                machine: None,
            },
        }
    }
}

impl Reach<'_> {
    /// This machine's name in the mesh, as agents say it; empty without one.
    fn my_name(&self) -> String {
        let Some(mesh) = &self.mesh else {
            return String::new();
        };
        machines(mesh)
            .into_iter()
            .find(|machine| machine.id == mesh.me())
            .map(|machine| machine.name)
            .unwrap_or_default()
    }

    /// The machine `name` names: `None` is this one.
    fn target(&self, name: Option<&str>) -> Result<Option<Machine>, String> {
        let Some(name) = name.map(command_name).filter(|name| !name.is_empty()) else {
            return Ok(None);
        };
        let Some(mesh) = &self.mesh else {
            return if name == self.my_name() {
                Ok(None)
            } else {
                Err(format!(
                    "This machine is in no mesh, so it reaches no machine called {name}."
                ))
            };
        };
        let all = machines(mesh);
        match all.iter().find(|machine| machine.name == name) {
            Some(machine) if machine.id == mesh.me() => Ok(None),
            Some(machine) => Ok(Some(Machine {
                id: machine.id,
                name: machine.name.clone(),
            })),
            None => {
                let names: Vec<&str> = all.iter().map(|machine| machine.name.as_str()).collect();
                Err(format!(
                    "No machine called {name}. Machines: {}.",
                    names.join(", ")
                ))
            }
        }
    }

    async fn start(&self, parent: &Arc<Session>, args: StartArgs) -> Reply {
        let StartArgs {
            harness,
            prompt,
            machine,
            cwd,
            model,
            wait,
        } = args;
        if prompt.trim().is_empty() {
            return fail("Give a `prompt`.");
        }
        if let Some(model) = &model
            && !beep_proto::node::agent::valid_model(model)
        {
            return fail(format!("{model:?} is not a model name."));
        }
        let target = match self.target(machine.as_deref()) {
            Ok(target) => target,
            Err(e) => return fail(e),
        };
        let link = match self.agents.link_for(parent, self.who, target.is_some()) {
            Ok(link) => link,
            Err(e) => return fail(e),
        };
        let parts = StartParts {
            harness: harness.trim().to_owned(),
            prompt,
            cwd,
            model,
        };
        let started = match target {
            None => self.start_here(parent, link, parts).await,
            Some(machine) => self.start_there(&machine, parent, link, parts).await,
        };
        match started {
            Ok(view) if wait => json_of(&self.waited(view).await),
            Ok(view) => json_of(&view),
            Err(e) => fail(e),
        }
    }

    /// Starts the child here.
    async fn start_here(
        &self,
        parent: &Arc<Session>,
        link: ParentLink,
        start: StartParts,
    ) -> Result<ChildView, String> {
        let cwd = start.cwd.unwrap_or_else(|| parent.info().cwd);
        let child = self
            .agents
            .start_child(
                link,
                ChildStart {
                    harness: start.harness,
                    prompt: start.prompt,
                    cwd: Some(cwd),
                    model: start.model.map(ModelId::new),
                },
            )
            .await?;
        let id = Agents::name_of(&child);
        Ok(ChildView::of(&child.info(), &id).named(&self.my_name()))
    }

    /// Sends the start to `machine`, which starts the child and checks the
    /// limits again.
    async fn start_there(
        &self,
        machine: &Machine,
        parent: &Arc<Session>,
        link: ParentLink,
        start: StartParts,
    ) -> Result<ChildView, String> {
        let Some(mesh) = &self.mesh else {
            return Err("This machine is in no mesh.".to_owned());
        };
        let cwd = match start.cwd {
            Some(cwd) => cwd,
            None => project_folder(mesh, &parent.info().cwd, machine.id)?,
        };
        let order = PeerStart {
            harness: start.harness,
            prompt: start.prompt,
            cwd: Some(cwd),
            model: start.model,
            parent: link,
        };
        let reply = self
            .peer(mesh, machine, Tool::StartAgent, &json!(order))
            .await?;
        let view: ChildView = serde_json::from_str(&reply)
            .map_err(|e| format!("{} answered something unreadable: {e}", machine.name))?;
        let view = view.named(&machine.name);
        let id = ChildId::parse(&view.id);
        let kid = RemoteKid {
            parent: self.who.session.clone(),
            machine: machine.id,
            name: machine.name.clone(),
            id: id.session.to_owned(),
            ended: false,
        };
        if let Some(parent) = self.agents.get(&self.who.session) {
            self.agents.adopt_remote(&parent, kid);
        }
        Ok(view)
    }

    /// One call to a child on `machine`: the text it answers, or why not.
    async fn peer(
        &self,
        mesh: &Mesh,
        machine: &Machine,
        tool: Tool,
        args: &Value,
    ) -> Result<String, String> {
        let request = Request {
            session: self.who.session.clone(),
            tool,
            args: args.to_string(),
            approved: Default::default(),
        };
        let sent = timeout(REMOTE_LIMIT, over_the_mesh(mesh, machine.id, &request))
            .await
            .map_err(|_| format!("{} did not answer in time.", machine.name))?
            .map_err(|e| format!("Could not reach {}: {e:#}", machine.name))?;
        match sent {
            Reply::Done {
                text,
                failed: false,
            } => Ok(text),
            Reply::Done { text, .. } => Err(text),
            other => Err(format!("{} answered {other:?}.", machine.name)),
        }
    }

    /// Where a child named by `id` is, and the child itself on this machine.
    /// The same refusal whether it is not there or not the caller's.
    fn find(&self, id: &str) -> Result<Found, String> {
        let id = ChildId::parse(id);
        let none = || format!("You have no agent {}.", id.session);
        match self.target(id.machine).map_err(|_| none())? {
            None => Ok(Found::Here(self.agents.child_of(self.who, id.session)?)),
            Some(machine) => self
                .agents
                .remote_kids(&self.who.session)
                .into_iter()
                .find(|kid| kid.machine == machine.id && kid.id == id.session)
                .map(Found::There)
                .ok_or_else(none),
        }
    }

    async fn status(&self, id: &str) -> Reply {
        match self.view_of(id).await {
            Ok(view) => json_of(&view),
            Err(e) => fail(e),
        }
    }

    /// How a child stands now.
    async fn view_of(&self, id: &str) -> Result<ChildView, String> {
        match self.find(id)? {
            Found::Here(child) => {
                let name = Agents::name_of(&child);
                Ok(ChildView::of(&child.info(), &name).named(&self.my_name()))
            }
            Found::There(kid) => self.view_there(&kid).await,
        }
    }

    async fn view_there(&self, kid: &RemoteKid) -> Result<ChildView, String> {
        let Some(mesh) = &self.mesh else {
            return Err("This machine is in no mesh.".to_owned());
        };
        let machine = Machine {
            id: kid.machine,
            name: kid.name.clone(),
        };
        let args = json!(PeerCall {
            id: kid.id.clone(),
            text: None,
            turn_only: false
        });
        let text = self.peer(mesh, &machine, Tool::AgentStatus, &args).await?;
        let view: ChildView = serde_json::from_str(&text)
            .map_err(|e| format!("{} answered something unreadable: {e}", kid.name))?;
        if view.state == "ended" {
            self.agents.end_remote(kid);
        }
        Ok(view.named(&kid.name))
    }

    async fn send(&self, id: &str, text: String) -> Reply {
        if text.trim().is_empty() {
            return fail("Give the `text` to send.");
        }
        match self.find(id) {
            Err(e) => fail(e),
            Ok(Found::Here(child)) => match child.order(Command::Prompt(text.into())).await {
                Ok(()) => ok("Sent. It reads it after its current turn."),
                Err(e) => fail(e),
            },
            Ok(Found::There(kid)) => {
                let Some(mesh) = &self.mesh else {
                    return fail("This machine is in no mesh.");
                };
                let machine = Machine {
                    id: kid.machine,
                    name: kid.name.clone(),
                };
                let args = json!(PeerCall {
                    id: kid.id.clone(),
                    text: Some(text),
                    turn_only: false
                });
                match self.peer(mesh, &machine, Tool::SendToAgent, &args).await {
                    Ok(text) => ok(text),
                    Err(e) => fail(e),
                }
            }
        }
    }

    async fn stop(&self, id: &str) -> Reply {
        match self.find(id) {
            Err(e) => fail(e),
            Ok(Found::Here(child)) => {
                child.close().await;
                ok("Stopped. Its session ended and stays in the user's list.")
            }
            Ok(Found::There(kid)) => {
                let Some(mesh) = &self.mesh else {
                    return fail("This machine is in no mesh.");
                };
                let sent = stop_remote(mesh, &self.who.session, &kid, false).await;
                self.agents.end_remote(&kid);
                match sent {
                    Ok(()) => ok("Stopped. Its session ended and stays in the user's list."),
                    Err(e) => fail(e),
                }
            }
        }
    }

    /// Every child of the caller, here and elsewhere.
    async fn list(&self) -> Reply {
        let here = self.my_name();
        let mut views: Vec<ChildView> = self
            .agents
            .children_of(self.who)
            .iter()
            .map(|child| {
                let id = Agents::name_of(child);
                ChildView::of(&child.info(), &id).named(&here)
            })
            .collect();
        let kids = self.agents.remote_kids(&self.who.session);
        let there = futures::future::join_all(kids.iter().map(|kid| self.view_there(kid))).await;
        for (kid, view) in kids.iter().zip(there) {
            views.push(view.unwrap_or_else(|e| ChildView {
                id: format!("{}@{}", kid.id, kid.name),
                key: String::new(),
                harness: String::new(),
                machine: kid.name.clone(),
                state: "unreachable".to_owned(),
                title: None,
                last_message: String::new(),
                files_changed: 0,
                question: None,
                turns: 0,
                tokens: 0,
                cost_usd: None,
                error: Some(e),
                note: None,
            }));
        }
        json_of(&views)
    }

    /// Waits for `view`'s turn to end, at most `WAIT_LIMIT`: an agent's tool
    /// call must come back before its client gives up, so a long job is
    /// polled instead.
    async fn waited(&self, mut view: ChildView) -> ChildView {
        let deadline = Instant::now() + WAIT_LIMIT;
        while !view.settled() {
            if Instant::now() >= deadline {
                view.note = Some(format!(
                    "Still working after {} seconds. Call agent_status with this id to check \
                     again.",
                    WAIT_LIMIT.as_secs()
                ));
                return view;
            }
            sleep(WAIT_POLL).await;
            match self.view_of(&view.id).await {
                Ok(next) => view = next,
                Err(e) => {
                    view.note = Some(e);
                    return view;
                }
            }
        }
        view
    }
}

enum Found {
    Here(Arc<Session>),
    There(RemoteKid),
}

/// What `StartArgs` hands on without the `wait`.
struct StartParts {
    harness: String,
    prompt: String,
    cwd: Option<String>,
    model: Option<String>,
}

/// The call that started a child, as its parent's transcript shows it:
/// `Beep: Start <harness> agent ...`, then the JSON `start_agent` returned.
const STARTED: &str = "Beep: Start ";
/// How long a transcript read waits for one child on another machine.
const SHOW_LIMIT: Duration = Duration::from_secs(2);

/// Shows each `start_agent` call among `entries` as the subagent card the
/// apps draw: its title, harness and machine, how it stands now and what it
/// last said. The child's own log is its own session, which the card links
/// to with a `[child]` line. A call whose result cannot be read stays a
/// plain call.
pub(in crate::agents) async fn show_children(
    agents: &Agents,
    session: &Session,
    entries: &mut [Entry],
) {
    let calls: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.kind == EntryKind::Tool && entry.text.starts_with(STARTED))
        .map(|(at, _)| at)
        .collect();
    if calls.is_empty() {
        return;
    }
    let Some((_, who)) = agents.caller(&session.id) else {
        return;
    };
    let reach = Reach {
        agents,
        mesh: agents.mesh(),
        who: &who,
    };
    let shown = futures::future::join_all(calls.iter().map(|&at| {
        let text = entries[at].text.clone();
        let reach = &reach;
        async move {
            let first = started_view(&text)?;
            let now = timeout(SHOW_LIMIT, reach.view_of(&first.id))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_else(|| first.clone());
            Some(child_entry(&text, &now))
        }
    }))
    .await;
    for (at, text) in calls.into_iter().zip(shown) {
        if let Some(text) = text {
            entries[at].text = text;
        }
    }
}

/// The child a `start_agent` entry's text names, from the JSON line it holds.
fn started_view(text: &str) -> Option<ChildView> {
    text.lines()
        .find(|line| line.starts_with('{'))
        .and_then(|line| serde_json::from_str(line).ok())
}

/// A child as its parent's transcript entry writes it: the lines the apps
/// read as a subagent, and `[child]` with the id to open and its state.
fn child_entry(call: &str, view: &ChildView) -> String {
    let title = view.title.clone().unwrap_or_else(|| {
        let line = call.lines().next().unwrap_or_default();
        let line = line
            .strip_suffix(']')
            .and_then(|l| l.rsplit_once(" ["))
            .map_or(line, |(l, _)| l);
        let line = line.strip_prefix(STARTED).unwrap_or(line);
        let asked = line.split_once(": ").map_or(line, |(_, prompt)| prompt);
        asked.to_owned()
    });
    let status = match view.state.as_str() {
        "working" => "in_progress",
        "ended" if view.error.is_some() => "failed",
        _ => "completed",
    };
    let mut text = format!(
        "{} [{status}]\n[steps] {}\n[agent] {} on {}\n[child] {} {} {}",
        title.lines().next().unwrap_or_default(),
        view.turns,
        view.harness,
        view.machine,
        view.id,
        view.state,
        view.key
    );
    if let Some(question) = &view.question {
        text.push_str(&format!("\nWaiting for you: {question}"));
    } else if !view.last_message.is_empty() {
        text.push('\n');
        text.push_str(&view.last_message);
    }
    text
}

/// Stops a child on another machine: its session, or only its running turn.
pub(crate) async fn stop_remote(
    mesh: &Mesh,
    parent: &str,
    kid: &RemoteKid,
    turn_only: bool,
) -> Result<(), String> {
    let request = Request {
        session: parent.to_owned(),
        tool: Tool::StopAgent,
        args: json!(PeerCall {
            id: kid.id.clone(),
            text: None,
            turn_only
        })
        .to_string(),
        approved: Default::default(),
    };
    let sent = timeout(REMOTE_LIMIT, over_the_mesh(mesh, kid.machine, &request))
        .await
        .map_err(|_| format!("{} did not answer in time.", kid.name))?
        .map_err(|e| format!("Could not reach {}: {e:#}", kid.name))?;
    match sent {
        Reply::Done { failed: false, .. } => Ok(()),
        Reply::Done { text, .. } => Err(text),
        other => Err(format!("{} answered {other:?}.", kid.name)),
    }
}

/// Answers an agent tool another node's agent called on this machine. The
/// caller's own node checked its side; the limits are checked here, from the
/// link the call carries.
pub(crate) async fn answer_peer(agents: &Agents, peer: EndpointId, request: Request) -> Reply {
    let who = Who {
        machine: Some(peer),
        session: request.session.clone(),
    };
    match request.tool {
        Tool::StartAgent => match parse::<PeerStart>(&request.args) {
            Ok(order) if order.parent.machine != Some(peer) => {
                fail("The link names another machine than the one calling.")
            }
            Ok(order) if order.parent.session != who.session => {
                fail("The link names another session than the one calling.")
            }
            Ok(order) => {
                let start = ChildStart {
                    harness: order.harness,
                    prompt: order.prompt,
                    cwd: order.cwd,
                    model: order.model.map(ModelId::new),
                };
                match agents.start_child(order.parent, start).await {
                    Ok(child) => json_of(&ChildView::of(&child.info(), &Agents::name_of(&child))),
                    Err(e) => fail(e),
                }
            }
            Err(e) => fail(e),
        },
        Tool::AgentStatus | Tool::SendToAgent | Tool::StopAgent => {
            let call = match parse::<PeerCall>(&request.args) {
                Ok(call) => call,
                Err(e) => return fail(e),
            };
            let child = match agents.child_of(&who, &call.id) {
                Ok(child) => child,
                Err(e) => return fail(e),
            };
            match request.tool {
                Tool::AgentStatus => {
                    json_of(&ChildView::of(&child.info(), &Agents::name_of(&child)))
                }
                Tool::SendToAgent => match call.text.filter(|text| !text.trim().is_empty()) {
                    Some(text) => match child.order(Command::Prompt(text.into())).await {
                        Ok(()) => ok("Sent. It reads it after its current turn."),
                        Err(e) => fail(e),
                    },
                    None => fail("Give the `text` to send."),
                },
                _ if call.turn_only => {
                    agents.cancel_family(&child).await;
                    // Refused when no turn runs.
                    let _ = child.order(Command::Cancel).await;
                    ok("Its turn is stopped.")
                }
                _ => {
                    child.close().await;
                    ok("Stopped.")
                }
            }
        }
        other => fail(format!("A node does not serve {other} for another node.")),
    }
}

/// The folder on `target` that holds the same synced project as `cwd`
/// here, with the same path inside it.
fn project_folder(mesh: &Mesh, cwd: &str, target: EndpointId) -> Result<String, String> {
    let specs: Vec<SyncSpec> = mesh
        .scan("sync/")
        .into_iter()
        .filter_map(|(key, bytes)| match key {
            Key::Sync(_) => SyncSpec::from_bytes(&bytes).ok(),
            _ => None,
        })
        .collect();
    folder_on(&specs, mesh.me(), Path::new(cwd), target).ok_or_else(|| {
        format!(
            "{cwd} is in no project that machine has a checkout of. Give `cwd`, a folder on \
             that machine."
        )
    })
}

/// The `target`'s path for `cwd`, a path of `me`, from the projects that
/// both sync.
fn folder_on(specs: &[SyncSpec], me: EndpointId, cwd: &Path, target: EndpointId) -> Option<String> {
    specs.iter().find_map(|spec| {
        let mine = Path::new(spec.folders.get(&me)?);
        let theirs = Path::new(spec.folders.get(&target)?);
        let inside = cwd.strip_prefix(mine).ok()?;
        Some(theirs.join(inside).display().to_string())
    })
}

#[cfg(test)]
mod tests;
