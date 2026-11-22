//! `start_agent` and its companions: an agent starts another agent, of any
//! harness, on any machine of the mesh, and follows it.
//!
//! The child is a normal session linked to the caller (see
//! `agents::family`). On this machine the node starts it itself; on another
//! it sends the order over `tools/1`, with the link, and the other node
//! starts it and checks the limits. An agent stops and checks only its own
//! children; it lists and messages any agent the user runs.

use beep_proto::SessionId;
use super::machines::{Machine, command_name, find, machines};
use super::reply::{fail, json_of, ok, parse};
use crate::agents::{
    Agents, Command, Session,
    family::{ChildNote, ChildStart, RemoteKid, WAIT_LIMIT, WAIT_POLL, Who},
};
use crate::link::Offers;
use beep_mesh::{Key, Mesh};
use beep_proto::{AccountName, ModelId};
use beep_proto::node::{
    agent::{Entry, EntryKind, ParentLink, SessionInfo, Status},
    mesh_state::{StateValue, SyncSpec},
    tools::{Reply, Request, Tool},
};
use beep_proto::watched;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeSet, path::Path, sync::Arc, time::Duration};
use tokio::time::{Instant, sleep};

mod peer;

use peer::{Answer, Call, GONE, KEPT, REMOTE_LIMIT, fetch_view};
pub(crate) use peer::{
    CountCall, Views, ask_parent, count_on, note_parent, stop_remote, stop_remote_retrying,
};

/// The longest title or message the agent reads back.
const MAX_TEXT: usize = 2000;
/// How long a machine has to list its agents.
const LIST_LIMIT: Duration = Duration::from_secs(5);

/// What an agent reads of one child.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChildView {
    /// The id the other tools take: `<session>@<machine>`.
    pub(crate) id: String,
    /// The id its machine lists it under, which the agent's own id
    /// replaces after a restart: how an app finds its row.
    #[serde(default)]
    pub(crate) key: SessionId,
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
    /// Said when `wait` ran out of time or the child's turn is done.
    #[serde(default)]
    pub(crate) note: Option<String>,
    /// The project whose checkout holds its folder, in a list of every agent.
    #[serde(default)]
    pub(crate) project: Option<String>,
    /// The branch of the checkout it works in, as its node last read it.
    #[serde(default)]
    pub(crate) branch: Option<String>,
}

impl ChildView {
    /// The view of a session as its own node reports it; `machine` and the
    /// id's machine part are set by whoever names the machine.
    pub(crate) fn of(info: &SessionInfo, id: &SessionId) -> Self {
        let state = match info.state.status {
            Status::Ended => "ended",
            Status::Asking => "waiting_on_user",
            Status::Idle if info.state.subagents == 0 => "idle",
            _ => "working",
        };
        Self {
            id: id.to_string(),
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
            project: None,
            branch: info.place.as_ref().and_then(|place| place.branch.clone()),
        }
    }

    pub(crate) fn named(mut self, machine: &str) -> Self {
        self.machine = machine.to_owned();
        self.id = full_id(&self.id, machine);
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
    project: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    #[serde(default)]
    wait: bool,
    #[serde(default)]
    background: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    project: Option<String>,
    #[serde(default)]
    all: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    id: String,
    text: String,
}

/// The id the other tools take for the child `id` on `machine`:
/// `<session>@<machine>`, or the bare id without a machine name.
pub(crate) fn full_id(id: &str, machine: &str) -> String {
    if machine.is_empty() || id.contains('@') {
        id.to_owned()
    } else {
        format!("{id}@{machine}")
    }
}

/// This machine's name in `mesh`, as agents say it; empty without one.
pub(crate) fn machine_name(mesh: Option<&Mesh>) -> String {
    let Some(mesh) = mesh else {
        return String::new();
    };
    machines(mesh)
        .into_iter()
        .find(|machine| machine.id == mesh.me())
        .map(|machine| machine.name)
        .unwrap_or_default()
}

/// What `start_agent` sends the machine that starts the child.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PeerStart {
    pub(crate) harness: String,
    pub(crate) prompt: String,
    pub(crate) cwd: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) parent: ParentLink,
    /// The named account the parent runs as. A node from before accounts
    /// ignores it.
    #[serde(default)]
    pub(crate) account: Option<AccountName>,
}

/// What a call to a child on another machine carries besides its id.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct PeerCall {
    pub(crate) id: SessionId,
    #[serde(default)]
    pub(crate) text: Option<String>,
    /// Stop only the running turn, not the session.
    #[serde(default)]
    pub(crate) turn_only: bool,
    /// With `turn_only`: stop its background commands and subagents too, as
    /// a parent's Stop all does. A node from 0.5.0 and before ignores it and
    /// stops the turn.
    #[serde(default)]
    pub(crate) stop_all: bool,
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

    reach
        .dispatch(&session, request)
        .await
        .unwrap_or_else(fail)
}

/// The caller, and the ways it has to its children.
struct Reach<'a> {
    agents: &'a Agents,
    mesh: Option<Mesh>,
    who: &'a Who,
}

/// A child's id split into its session and the machine it runs on.
struct ChildId<'a> {
    session: SessionId,
    machine: Option<&'a str>,
}

impl<'a> From<&'a str> for ChildId<'a> {
    fn from(text: &'a str) -> Self {
        match text.trim().split_once('@') {
            Some((session, machine)) => Self {
                session: Into::<SessionId>::into(session),
                machine: Some(machine),
            },
            None => Self {
                session: Into::<SessionId>::into(text.trim()),
                machine: None,
            },
        }
    }
}

impl Reach<'_> {
    /// This machine's name in the mesh, as agents say it; empty without one.
    fn my_name(&self) -> String {
        machine_name(self.mesh.as_ref())
    }

    /// `text` as the caller `sender` sends it, tagged with `id`, the id
    /// the receiver answers to, so its log tells it from the user's words.
    fn tagged(sender: &Session, id: &str, text: &str) -> String {
        let info = sender.info();
        let name = info.title.unwrap_or(info.agent);
        crate::agents::tags::from_session(id, &name, text)
    }

    /// `child` as it stands on this machine.
    fn view_here(&self, child: &Session) -> ChildView {
        ChildView::of(&child.info(), &Agents::name_of(child)).named(&self.my_name())
    }

    async fn dispatch(&self, session: &Arc<Session>, request: &Request) -> Result<Reply, String> {
        Ok(match request.tool {
            Tool::StartAgent => self.start(session, parse(&request.args)?).await,
            Tool::AgentStatus => self.status(&parse::<IdArgs>(&request.args)?.id).await,
            Tool::SendToAgent => {
                let SendArgs { id, text } = parse(&request.args)?;
                self.send(session, &id, text).await
            }
            Tool::StopAgent => self.stop(&parse::<IdArgs>(&request.args)?.id).await,
            Tool::ListAgents => match parse::<ListArgs>(&request.args)? {
                ListArgs {
                    project: None,
                    all: false,
                } => self.list().await,
                ListArgs { project, .. } => self.everyone(project).await,
            },
            other => fail(format!("{other} is not an agent tool.")),
        })
    }

    /// The machine `name` names: `None` is this one.
    fn target(&self, name: Option<&str>) -> Result<Option<Machine>, String> {
        let Some(name) = name.map(command_name).filter(|name| !name.is_empty()) else {
            return Ok(None);
        };
        let Some(mesh) = &self.mesh else {
            return Err(format!(
                "This machine is in no mesh, so it reaches no machine called {name}."
            ));
        };

        match find(mesh, &name) {
            Some(machine) if machine.id == mesh.me() => Ok(None),
            Some(machine) => Ok(Some(machine)),
            None => {
                let all = machines(mesh);
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
            project,
            cwd,
            model,
            wait,
            background,
        } = args;

        if prompt.trim().is_empty() {
            return fail("Give a `prompt`.");
        }

        let prompt = Self::tagged(parent, self.who.session.as_ref(), &prompt);

        if let Some(model) = &model
            && !beep_proto::node::agent::valid_model(model)
        {
            return fail(format!("{model:?} is not a model name."));
        }

        let target = match self.target(machine.as_deref()) {
            Ok(target) => target,
            Err(e) => return fail(e),
        };
        let cwd = match project {
            Some(project) => {
                if cwd.is_some() {
                    return fail("Give either `project` or `cwd`, not both.");
                }

                let Some(mesh) = &self.mesh else {
                    return fail("This machine is in no mesh, so it has no registered projects.");
                };
                let machine = target.as_ref().map_or(mesh.me(), |machine| machine.id);

                match super::projects::folder(mesh, &project, machine) {
                    Ok(folder) => Some(folder),
                    Err(e) => return fail(e),
                }
            }
            None => cwd,
        };

        let link = match self.agents.link_for(parent, self.who) {
            Ok(link) => ParentLink { background, ..link },
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
                    review: None,
                    account: parent.account.clone(),
                },
            )
            .await?;
        Ok(self.view_here(&child))
    }

    /// Sends the start to `machine`, which starts the child and checks the
    /// limits again. The child is noted before the order goes: an answer
    /// that never comes leaves it running, and it keeps its place until the
    /// machine's own check closes it.
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
            account: parent.account.clone(),
        };
        let me = &self.who.session;
        self.agents
            .adopt_remote(parent, RemoteKid::expected(me, machine.id, &machine.name));
        let sent = Call::to_machine(mesh, me, machine);
        let text = match sent
            .ask(Tool::StartAgent, &json!(order), REMOTE_LIMIT)
            .await
        {
            Answer::Text(text) => text,
            Answer::Refused(why) => {
                self.agents.forget_expected(me, machine.id);
                return Err(why);
            }
            Answer::Silent(why) => {
                return Err(format!(
                    "{why} The agent may have started anyway: it keeps its place for two \
                     minutes, and closes itself if nothing claims it."
                ));
            }
        };
        let view: ChildView = serde_json::from_str(&text)
            .map_err(|e| format!("{} answered something unreadable: {e}", machine.name))?;
        let view = view.named(&machine.name);
        self.agents
            .settle_remote(me, machine.id, &ChildId::from(view.id.as_str()).session);
        Ok(view)
    }

    /// Where a child named by `id` is, and the child itself on this machine.
    /// The same refusal whether it is not there or not the caller's.
    fn find(&self, id: &str) -> Result<Found, String> {
        let id = ChildId::from(id);
        let none = || format!("You have no agent {}.", id.session);

        match self.target(id.machine).map_err(|_| none())? {
            None => Ok(Found::Here(self.agents.child_of(self.who, &id.session)?)),
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
        // A review's id: its reviewer is gone once it is done.
        if let Some(run) = self.agents.review_for(self.who, &ChildId::from(id).session) {
            return super::review::standing(run);
        }

        match self.view_of(id).await {
            Ok(view) => json_of(&view),
            Err(e) => fail(e),
        }
    }

    /// How a child stands now.
    async fn view_of(&self, id: &str) -> Result<ChildView, String> {
        match self.find(id)? {
            Found::Here(child) => Ok(self.view_here(&child)),
            Found::There(kid) => self.view_there(&kid).await,
        }
    }

    /// A child as it stands, without waiting on any machine: what its own
    /// node says when it runs here, else what was last read of it.
    fn known_view(&self, id: &str) -> Option<ChildView> {
        match self.find(id).ok()? {
            Found::Here(child) => Some(self.view_here(&child)),
            Found::There(kid) => {
                let mesh = self.mesh.as_ref()?;
                let view = self.agents.family.views.of(mesh, &self.who.session, &kid)?;
                Some(view.named(&kid.name))
            }
        }
    }

    async fn view_there(&self, kid: &RemoteKid) -> Result<ChildView, String> {
        let Some(mesh) = &self.mesh else {
            return Err("This machine is in no mesh.".to_owned());
        };
        let view = fetch_view(mesh, &self.who.session, kid).await?;

        if view.state == "ended" {
            self.agents.end_remote(kid);
        }

        Ok(view.named(&kid.name))
    }

    async fn send(&self, parent: &Session, id: &str, text: String) -> Reply {
        if text.trim().is_empty() {
            return fail("Give the `text` to send.");
        }

        let Ok(found) = self.find(id) else {
            return self.message(parent, id, &text).await;
        };
        let text = Self::tagged(parent, self.who.session.as_ref(), &text);

        match found {
            Found::Here(child) => match child.order(Command::Steer(text.into())).await {
                Ok(()) => ok("Sent. It reads it at its next step."),
                Err(e) => fail(e),
            },
            Found::There(kid) => {
                let Some(mesh) = &self.mesh else {
                    return fail("This machine is in no mesh.");
                };
                let sent = Call::to_kid(mesh, &self.who.session, &kid);
                let args = json!(PeerCall {
                    id: kid.id.clone(),
                    text: Some(text),
                    ..Default::default()
                });

                match sent.ask(Tool::SendToAgent, &args, REMOTE_LIMIT).await {
                    Answer::Text(text) => ok(text),
                    Answer::Refused(why) | Answer::Silent(why) => fail(why),
                }
            }
        }
    }

    /// `text` to an agent the caller did not start, here or on its machine,
    /// tagged with the caller's full id so the agent can answer.
    async fn message(&self, sender: &Session, id: &str, text: &str) -> Reply {
        let text = Self::tagged(sender, &full_id(self.who.session.as_ref(), &self.my_name()), text);
        let id = ChildId::from(id);
        let sent = match (self.target(id.machine), &self.mesh) {
            (Err(e), _) => Err(e),
            (Ok(None), _) => {
                self.agents
                    .message(&self.who.session, &id.session, text)
                    .await
            }
            (Ok(Some(machine)), Some(mesh)) => {
                let args = json!(PeerCall {
                    id: id.session.clone(),
                    text: Some(text),
                    ..Default::default()
                });
                let call = Call::to_machine(mesh, &self.who.session, &machine);

                match call.ask(Tool::SendToAgent, &args, REMOTE_LIMIT).await {
                    Answer::Text(_) => Ok(()),
                    Answer::Refused(why) | Answer::Silent(why) => Err(why),
                }
            }
            (Ok(Some(_)), None) => Err("This machine is in no mesh.".to_owned()),
        };

        match sent {
            Ok(()) => ok("Sent. It reads it at its next step and answers only if it \
                 sends one back."),
            Err(e) => fail(e),
        }
    }

    /// Every live agent but the caller, on every machine that answers in
    /// time, only those in `project` when given.
    async fn everyone(&self, project: Option<String>) -> Reply {
        let project = match (project, &self.mesh) {
            (None, _) => None,
            (Some(name), Some(mesh)) => match super::projects::project_name(mesh, &name) {
                Ok(name) => Some(name),
                Err(e) => return fail(e),
            },
            (Some(_), None) => {
                return fail("This machine is in no mesh, so it has no registered projects.");
            }
        };
        let mut views: Vec<ChildView> =
            in_the_mesh(self.agents, self.mesh.as_ref(), &self.who.session, None, None)
                .await
                .into_iter()
                .map(|listed| listed.view)
                .collect();

        if let Some(project) = project {
            views.retain(|view| view.project.as_ref() == Some(&project));
        }

        json_of(&views)
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
                // It keeps its place until its machine says it stopped.
                match stop_remote(mesh, &self.who.session, &kid, false).await {
                    Ok(()) => {
                        self.agents.end_remote(&kid);
                        ok("Stopped. Its session ended and stays in the user's list.")
                    }
                    Err(e) => fail(format!(
                        "{e} It is not stopped and still counts as running: try again."
                    )),
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
            .filter_map(|child| {
                watched::read(&child.live, |live| {
                    let id = live.agent_session.as_ref()?;
                    Some(ChildView::of(&live.info, id).named(&here))
                })
            })
            .collect();
        // One whose start got no answer has no id to ask for yet.
        let mut kids = self.agents.remote_kids(&self.who.session);
        kids.retain(|kid| !kid.pending());
        let there = futures::future::join_all(kids.iter().map(|kid| self.view_there(kid))).await;

        for (kid, view) in kids.iter().zip(there) {
            views.push(view.unwrap_or_else(|e| ChildView {
                id: format!("{}@{}", kid.id, kid.name),
                machine: kid.name.clone(),
                state: "unreachable".to_owned(),
                error: Some(e),
                ..Default::default()
            }));
        }

        json_of(&views)
    }

    /// Waits for `view`'s turn to end, at most `WAIT_LIMIT`: an agent's tool
    /// call must come back before its client gives up, so a long job is
    /// polled instead.
    async fn waited(&self, mut view: ChildView) -> ChildView {
        // What this wait answers goes in no note.
        let _waiting = self.agents.family.waits.hold(&self.who.session, &view.id);
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

        if view.state == "idle" {
            view.note = Some(
                "Continue it with send_to_agent, or stop it with stop_agent once you need \
                 nothing more from it."
                    .to_owned(),
            );
        }

        view
    }
}

/// What another machine asks of this one's agents: `ended_since`, Unix
/// seconds, lists the sessions that ended then or later too. An older node
/// sends none.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct PeerList {
    ended_since: Option<u64>,
}

/// The agents this node runs that other machines may list, as `args` asks,
/// each with its project.
pub(crate) fn listed(agents: &Agents, args: &str) -> Reply {
    let PeerList { ended_since } = serde_json::from_str(args).unwrap_or_default();
    let views: Vec<ChildView> = listed_here(agents, ended_since)
        .into_iter()
        .map(|listed| listed.view)
        .collect();

    json_of(&views)
}

/// An agent, its machine's display name, and the folder it works in
/// when it runs on this machine: the worktree Beep made for it, or its own.
pub(crate) struct Listed {
    pub(crate) view: ChildView,
    pub(crate) machine: String,
    pub(crate) folder: Option<String>,
}

fn listed_here(agents: &Agents, ended_since: Option<u64>) -> Vec<Listed> {
    let mesh = agents.mesh();

    agents
        .peers(ended_since)
        .iter()
        .map(|session| {
            let info = session.info();
            let mut view = ChildView::of(&info, &Agents::name_of(session));
            view.project = mesh
                .as_ref()
                .and_then(|mesh| super::projects::of_folder(mesh, &info.cwd, mesh.me()));
            let worktree = info.worktrees.first().cloned();
            Listed {
                view,
                machine: String::new(),
                folder: Some(worktree.unwrap_or(info.cwd)),
            }
        })
        .collect()
}

/// Every live agent but `caller`, the session asking: this machine's, then
/// those of every other online machine of `mesh`, or of `only` those, that
/// answers in time, each named by its machine. With `ended_since`, Unix
/// seconds, the sessions that ended then or later too.
pub(crate) async fn in_the_mesh(
    agents: &Agents,
    mesh: Option<&Mesh>,
    caller: &SessionId,
    only: Option<&BTreeSet<EndpointId>>,
    ended_since: Option<u64>,
) -> Vec<Listed> {
    let here = machine_name(mesh);
    let display = mesh
        .and_then(|mesh| mesh.code_hosts().into_iter().find(|status| status.device.id == mesh.me()))
        .map(|status| status.device.name)
        .unwrap_or_default();
    let mut listed: Vec<Listed> = listed_here(agents, ended_since)
        .into_iter()
        .filter(|listed| listed.view.id != *caller)
        .map(|listed| Listed {
            view: listed.view.named(&here),
            machine: display.clone(),
            ..listed
        })
        .collect();
    let Some(mesh) = mesh else {
        return listed;
    };

    let others: Vec<(Machine, String)> = mesh
        .code_hosts()
        .into_iter()
        .filter(|status| status.online && status.device.id != mesh.me())
        .filter(|status| only.is_none_or(|only| only.contains(&status.device.id)))
        .map(|status| {
            let display = status.device.name.clone();
            (Machine::from(status), display)
        })
        .collect();
    let asked = json!(PeerList { ended_since });
    let lists = futures::future::join_all(others.iter().map(|(machine, _)| async {
        Call::to_machine(mesh, caller, machine)
            .ask(Tool::ListAgents, &asked, LIST_LIMIT)
            .await
    }))
    .await;

    // A machine that does not answer, or an older one that lists no agents
    // for others, adds none.
    for ((machine, display), answer) in others.iter().zip(lists) {
        let Answer::Text(text) = answer else { continue };
        let Ok(there) = serde_json::from_str::<Vec<ChildView>>(&text) else {
            continue;
        };
        listed.extend(there.into_iter().map(|view| Listed {
            view: view.named(&machine.name),
            machine: display.clone(),
            folder: None,
        }));
    }

    listed
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

/// Shows each `start_agent` call among `entries` as the subagent card the
/// apps draw: its title, harness and machine, how it stands now and what it
/// last said. The child's own log is its own session, which the card links
/// to with a `[child]` line. A call whose result cannot be read stays a
/// plain call. A child on another machine shows as last read: the read
/// never waits for the machine.
pub(in crate::agents) fn show_children(agents: &Agents, session: &Session, entries: &mut [Entry]) {
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

    for at in calls {
        let text = entries[at].text.clone();
        let Some(first) = started_view(&text) else {
            continue;
        };
        let now = reach.known_view(&first.id).unwrap_or_else(|| first.clone());
        entries[at].text = child_entry(&text, &now);
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
    // Keep the original prompt below the display title. The call's final
    // line is its JSON result, so prompt lines may themselves contain JSON.
    let prompt = call
        .rsplit_once("\n{")
        .and_then(|(head, _)| head.rsplit_once(" ["))
        .and_then(|(title, _)| title.strip_prefix(STARTED))
        .and_then(|start| start.split_once(": "))
        .map(|(_, prompt)| prompt);
    let title = title.lines().next().unwrap_or_default();
    let title = prompt.map_or_else(|| title.to_owned(), |prompt| format!("{title}\n{prompt}"));
    let mut text = format!(
        "{} [{status}]\n[steps] {}\n[agent] {} on {}\n[child] {} {} {}",
        title, view.turns, view.harness, view.machine, view.id, view.state, view.key
    );
    // What the child spent, for the card only: it is the child's session's
    // own spend, which the parent's numbers never include.
    if view.tokens > 0 || view.cost_usd.is_some() {
        let cost = view
            .cost_usd
            .map_or_else(|| "-".to_owned(), |usd| format!("{usd:.4}"));
        text.push_str(&format!("\n[spend] {} {cost}", view.tokens));
    }

    if let Some(question) = &view.question {
        text.push_str(&format!("\nWaiting for you: {question}"));
    } else if !view.last_message.is_empty() {
        text.push('\n');
        text.push_str(&view.last_message);
    }
    text
}

/// Answers an agent tool another node's agent called on this machine. The
/// caller's own node checked its side; the limits are checked here, from the
/// link the call carries.
pub(crate) async fn answer_peer(agents: &Agents, peer: EndpointId, request: Request) -> Reply {
    serve_peer(agents, peer, &request)
        .await
        .unwrap_or_else(fail)
}

async fn serve_peer(
    agents: &Agents,
    peer: EndpointId,
    request: &Request,
) -> Result<Reply, String> {
    let who = Who {
        machine: Some(peer),
        session: request.session.clone(),
    };

    match request.tool {
        Tool::StartAgent => {
            let order: PeerStart = parse(&request.args)?;

            if order.parent.machine != Some(peer) {
                return Err("The link names another machine than the one calling.".to_owned());
            }

            if order.parent.session != who.session {
                return Err("The link names another session than the one calling.".to_owned());
            }

            let start = ChildStart {
                harness: order.harness,
                prompt: order.prompt,
                cwd: order.cwd,
                model: order.model.map(ModelId::new),
                review: None,
                account: order.account,
            };
            let child = agents.start_child(order.parent, start).await?;

            Ok(json_of(&ChildView::of(
                &child.info(),
                &Agents::name_of(&child),
            )))
        }
        // A child's node asks the node of its parent whether it is kept: the
        // call's session is the parent here, and `peer` the machine of the child.
        Tool::AgentParent => {
            let call: PeerCall = parse(&request.args)?;
            let key = call.text.unwrap_or_default();
            let keeps = agents.keeps_child(peer, &request.session, &call.id, &Into::<SessionId>::into(key));

            Ok(ok(if keeps { KEPT } else { GONE }))
        }
        // A child on `peer` finished a turn or asks the user: the call's
        // session is its parent here.
        Tool::AgentNote => {
            let note: ChildNote = parse(&request.args)?;
            agents.note_from(peer, &request.session, &note).await?;

            Ok(ok("Told."))
        }
        // How many live agents under a root run here.
        Tool::AgentCount => {
            let call: CountCall = parse(&request.args)?;

            Ok(ok(agents
                .live_descendants(call.root_machine, &call.root)
                .to_string()))
        }
        // Another machine's agent lists the agents here.
        Tool::ListAgents => Ok(listed(agents, &request.args)),
        // To its own child, or to any agent here as a message from another.
        Tool::SendToAgent => {
            let call: PeerCall = parse(&request.args)?;
            let Some(text) = call.text.filter(|text| !text.trim().is_empty()) else {
                return Ok(fail("Give the `text` to send."));
            };

            match agents.child_of(&who, &call.id) {
                Ok(child) => child.order(Command::Steer(text.into())).await?,
                Err(_) => agents.message(&who.session, &call.id, text).await?,
            }

            Ok(ok("Sent. It reads it at its next step."))
        }
        Tool::AgentStatus | Tool::StopAgent => {
            let call: PeerCall = parse(&request.args)?;
            let child = agents.child_of(&who, &call.id)?;

            Ok(match request.tool {
                Tool::AgentStatus => {
                    json_of(&ChildView::of(&child.info(), &Agents::name_of(&child)))
                }
                _ if call.turn_only => {
                    // Refused when no turn runs. A parent's node from 0.5.0
                    // and before asks without `stop_all`.
                    let command = if call.stop_all {
                        Command::StopAll { by_user: false }
                    } else {
                        Command::Cancel { by_user: false }
                    };
                    let _ = child.order(command).await;
                    agents.cancel_family(&child).await;
                    ok("Its turn is stopped.")
                }
                _ => {
                    child.close().await;
                    ok("Stopped.")
                }
            })
        }
        other => Err(format!("A node does not serve {other} for another node.")),
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
#[path = "../../tests/agents_mcp_children.rs"]
mod tests;
