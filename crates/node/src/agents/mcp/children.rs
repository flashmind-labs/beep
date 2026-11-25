//! `start_agent` and its companions: an agent starts another agent, of any
//! harness, on any machine of the mesh, and follows it.
//!
//! The child is a normal session linked to the caller (see
//! `agents::family`). On this machine the node starts it itself; on another
//! it sends the order over `tools/1`, with the link, and the other node
//! starts it and checks the limits. An agent stops and checks only its own
//! children; it lists and messages any agent the user runs.

use super::machines::{Machine, find_named, machines};
use crate::agents::{calls::command_name, child_view::{ChildView, full_id}};
use super::reply::{fail, json_of, ok, parse};
use crate::agents::{
    Agents, Command, Session, live::{Live, Mark},
    family::{ChildNote, ChildStart, RemoteKid, WAIT_LIMIT, Waiting, Who},
};
use crate::link::Offers;
use beep_mesh::Mesh;
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{Entry, EntryKind, ParentLink},
    mesh_state::SyncSpec,
    tools::{Reply, Request, Tool},
};
use beep_proto::watched;
use beep_proto::{AccountName, CustomAgent, ModelId};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeSet, path::Path, sync::Arc, time::Duration};
use std::future::Future;
use tokio::{sync::watch, time::sleep};

mod peer;

use peer::{Answer, Call, GONE, KEPT, REMOTE_LIMIT, fetch_view, parse_view};
pub(crate) use peer::{
    CountCall, Views, ask_parent, count_on, NoteFailed, note_parent, stop_remote, stop_remote_retrying,
};

/// How long a machine has to list its agents.
const LIST_LIMIT: Duration = Duration::from_secs(5);
/// The call that started a child, as its parent's transcript shows it:
/// `Beep: Start <harness> agent ...`, then the JSON `start_agent` returned.
const STARTED: &str = "Beep: Start ";

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
    /// The name its parent gives it. A node from before threads ignores it.
    #[serde(default)]
    pub(crate) title: Option<String>,
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

impl PeerCall {
    pub(super) fn id(id: SessionId) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub(super) fn message(id: SessionId, text: String) -> Self {
        Self {
            id,
            text: Some(text),
            ..Self::default()
        }
    }
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

    reach.dispatch(&session, request).await.unwrap_or_else(fail)
}

/// Answers an agent tool another node's agent called on this machine. The
/// caller's own node checked its side; the limits are checked here, from the
/// link the call carries.
pub(crate) async fn answer_peer(agents: &Agents, peer: EndpointId, request: Request) -> Reply {
    serve_peer(agents, peer, &request)
        .await
        .unwrap_or_else(fail)
}

/// Shows each `start_agent` call among `entries` as the subagent card the
/// apps draw: its title, harness and machine, how it stands now and what it
/// last said. The child's own log is its own session, which the card links
/// to with a `[child]` line. A call whose result cannot be read stays a
/// plain call. A child on another machine shows as last read: the read
/// never waits for the machine. A child that is gone shows as ended.
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
        entries[at].text = child_entry(&text, &reach.known_view(&first));
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartArgs {
    /// None for a thread, which runs on its chat's harness unless it names one.
    harness: Option<String>,
    prompt: String,
    machine: Option<String>,
    project: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    title: Option<String>,
    /// A custom agent's name: it runs as that one, here only.
    persona: Option<String>,
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
    title: Option<String>,
    persona: Option<CustomAgent>,
}

impl Reach<'_> {
    fn my_name(&self) -> String {
        machine_name(self.mesh.as_ref())
    }

    fn mesh(&self) -> Result<&Mesh, String> {
        self.mesh
            .as_ref()
            .ok_or_else(|| "This machine is in no mesh.".to_owned())
    }

    /// `text` as the caller `sender` sends it, tagged with `id`, the id
    /// the receiver answers to, so its log tells it from the user's words.
    fn tagged(sender: &Session, id: &str, text: &str) -> String {
        let info = sender.info();
        let name = info.title.unwrap_or(info.agent);

        crate::agents::tags::from_session(id, &name, text)
    }

    /// `child` as it stands on this machine, which the caller has now seen.
    fn view_here(&self, child: &Session) -> ChildView {
        self.shown([child]);
        ChildView::of(&child.info(), &Agents::name_of(child)).named(&self.my_name())
    }

    /// Notes that the caller was shown `children` as they stand, so its next
    /// turn is not told of them again.
    fn shown<'a>(&self, children: impl IntoIterator<Item = &'a Session>) {
        if let Some(caller) = self.agents.get(&self.who.session) {
            caller.see_soon(
                children
                    .into_iter()
                    .map(|child| {
                        (
                            child.id.clone(),
                            watched::read(&child.live, |live| Into::<Mark>::into(live)),
                        )
                    })
                    .collect(),
            );
        }
    }

    async fn dispatch(&self, session: &Arc<Session>, request: &Request) -> Result<Reply, String> {
        match request.tool {
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
                } => Ok(self.list().await),
                ListArgs { project, .. } => self.everyone(project).await,
            },
            other => Err(format!("{other} is not an agent tool.")),
        }
    }

    /// The machine `name` names: `None` is this one.
    fn target(&self, name: Option<&str>) -> Result<Option<Machine>, String> {
        let Some(name) = name.map(command_name).filter(|name| !name.is_empty()) else {
            return Ok(None);
        };
        let mesh = self.mesh.as_ref().ok_or_else(|| {
            format!("This machine is in no mesh, so it reaches no machine called {name}.")
        })?;
        let machine = find_named(mesh, &name)?;

        Ok((machine.id != mesh.me()).then_some(machine))
    }

    async fn start(&self, parent: &Arc<Session>, args: StartArgs) -> Result<Reply, String> {
        let StartArgs {
            harness,
            prompt,
            machine,
            project,
            cwd,
            model,
            title,
            persona,
            wait,
            background,
        } = args;

        if prompt.trim().is_empty() {
            return Err("Give a `prompt`.".to_owned());
        }

        if let Some(model) = &model
            && !beep_proto::node::agent::valid_model(model)
        {
            return Err(format!("{model:?} is not a model name."));
        }

        let prompt = Self::tagged(parent, self.who.session.as_ref(), &prompt);
        let target = self.target(machine.as_deref())?;
        let cwd = match project {
            Some(project) => {
                if cwd.is_some() {
                    return Err("Give either `project` or `cwd`, not both.".to_owned());
                }

                let mesh = self
                    .mesh
                    .as_ref()
                    .ok_or("This machine is in no mesh, so it has no registered projects.")?;
                let on = target.as_ref().map_or(mesh.me(), |m| m.id);
                Some(super::projects::folder(mesh, &project, on)?)
            }
            None => cwd,
        };

        let harness = harness.map(|name| name.trim().to_owned());
        let persona = match persona.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(_) if target.is_some() => {
                return Err(
                    "A persona runs on this machine only: start it without `machine`.".to_owned(),
                );
            }
            Some(name) => {
                let at = cwd.clone().unwrap_or_else(|| parent.info().cwd);
                let wanted = harness.clone().unwrap_or_else(|| parent.agent.clone());
                Some(find_persona(at, name.to_owned(), wanted).await?)
            }
        };
        // The persona's own harness when none is named and this machine has
        // it. Its `model` is left out: the file names it as its harness does
        // (`opus`, `inherit`), which need not be an id the thread's harness
        // takes, so only an explicit `model` picks one.
        let harness = match (harness, &persona) {
            (Some(harness), _) => harness,
            (None, Some(persona)) if self.agents.named(persona.harness.as_ref()).is_some() => {
                persona.harness.as_ref().to_owned()
            }
            (None, _) => parent.agent.clone(),
        };

        let link = ParentLink {
            background,
            ..self.agents.link_for(parent, self.who)?
        };
        let parts = StartParts {
            harness,
            prompt,
            cwd,
            model,
            title,
            persona,
        };
        let view = match target {
            None => self.start_here(parent, link, parts).await?,
            Some(machine) => self.start_there(&machine, parent, link, parts).await?,
        };

        Ok(json_of(&if wait { self.waited(view).await } else { view }))
    }

    async fn start_here(
        &self,
        parent: &Arc<Session>,
        link: ParentLink,
        start: StartParts,
    ) -> Result<ChildView, String> {
        let made = match start.cwd {
            Some(_) => None,
            None => self
                .agents
                .child_folder(parent, start.title.as_deref())
                .map_err(|e| format!("{e:#}"))?,
        };
        let cwd = match (start.cwd, &made) {
            (Some(cwd), _) => cwd,
            (None, Some(made)) => made.to_string_lossy().into_owned(),
            (None, None) => parent.info().cwd,
        };
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
                    title: start.title,
                    persona: start.persona,
                },
            )
            .await;

        // A start that was refused keeps no folder and no name: its folder,
        // still empty, goes.
        if let (Err(_), Some(made)) = (&child, &made) {
            let _ = std::fs::remove_dir(made);
        }

        let child = child?;

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
        let mesh = self.mesh()?;
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
            title: start.title,
        };
        let me = &self.who.session;
        self.agents
            .adopt_remote(parent, RemoteKid::expected(me, machine.id, &machine.name));

        let text = match Call::to_machine(mesh, me, machine)
            .ask(Tool::StartAgent, &order, REMOTE_LIMIT)
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
        let view = parse_view(&machine.name, &text)?.named(&machine.name);
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

    async fn status(&self, id: &str) -> Result<Reply, String> {
        // A review's id: its reviewer is gone once it is done.
        if let Some(run) = self.agents.review_for(self.who, &ChildId::from(id).session) {
            return Ok(super::review::standing(run));
        }

        Ok(json_of(&self.view_of(id).await?))
    }

    async fn view_of(&self, id: &str) -> Result<ChildView, String> {
        match self.find(id)? {
            Found::Here(child) => Ok(self.view_here(&child)),
            Found::There(kid) => self.view_there(&kid).await,
        }
    }

    /// How the child `started` names stands, without waiting on its machine.
    /// One elsewhere that was never read shows as it started; one this parent
    /// no longer has has ended.
    fn known_view(&self, started: &ChildView) -> ChildView {
        let Ok(found) = self.find(&started.id) else {
            return ChildView {
                state: "ended".to_owned(),
                ..started.clone()
            };
        };

        match found {
            Found::Here(child) => self.view_here(&child),
            Found::There(kid) => self
                .mesh
                .as_ref()
                .and_then(|mesh| self.agents.family.views.of(mesh, &self.who.session, &kid))
                .map_or_else(|| started.clone(), |view| view.named(&kid.name)),
        }
    }

    async fn view_there(&self, kid: &RemoteKid) -> Result<ChildView, String> {
        let view = fetch_view(self.mesh()?, &self.who.session, kid).await?;

        if view.state == "ended" {
            self.agents.end_remote(kid);
        }

        Ok(view.named(&kid.name))
    }

    async fn send(&self, parent: &Session, id: &str, text: String) -> Result<Reply, String> {
        if text.trim().is_empty() {
            return Err("Give the `text` to send.".to_owned());
        }

        let Ok(found) = self.find(id) else {
            return self.message(parent, id, &text).await;
        };
        let text = Self::tagged(parent, self.who.session.as_ref(), &text);

        match found {
            Found::Here(child) => {
                Agents::tell_child(&child, text).await?;
                Ok(ok("Sent. It reads it at its next step."))
            }
            Found::There(kid) => {
                let args = PeerCall::message(kid.id.clone(), text);
                let reply = Call::to_kid(self.mesh()?, &self.who.session, &kid)
                    .ask_ok(Tool::SendToAgent, &args, REMOTE_LIMIT)
                    .await?;
                Ok(ok(reply))
            }
        }
    }

    /// `text` to an agent the caller did not start, here or on its machine,
    /// tagged with the caller's full id so the agent can answer.
    async fn message(&self, sender: &Session, id: &str, text: &str) -> Result<Reply, String> {
        let text = Self::tagged(
            sender,
            &full_id(self.who.session.as_ref(), &self.my_name()),
            text,
        );
        let id = ChildId::from(id);

        match self.target(id.machine)? {
            None => {
                self.agents
                    .message(&self.who.session, &id.session, text)
                    .await?
            }
            Some(machine) => {
                let args = PeerCall::message(id.session, text);
                Call::to_machine(self.mesh()?, &self.who.session, &machine)
                    .ask_ok(Tool::SendToAgent, &args, REMOTE_LIMIT)
                    .await?;
            }
        }

        Ok(ok(
            "Sent. It reads it at its next step and answers only if it sends one back.",
        ))
    }

    /// Every live agent but the caller, on every machine that answers in
    /// time, only those in `project` when given.
    async fn everyone(&self, project: Option<String>) -> Result<Reply, String> {
        let project = match project {
            None => None,
            Some(name) => {
                let mesh = self
                    .mesh
                    .as_ref()
                    .ok_or("This machine is in no mesh, so it has no registered projects.")?;
                Some(super::projects::project_name(mesh, &name)?)
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

        Ok(json_of(&views))
    }

    async fn stop(&self, id: &str) -> Result<Reply, String> {
        match self.find(id)? {
            Found::Here(child) => child.close().await,
            Found::There(kid) => {
                // It keeps its place until its machine says it stopped.
                stop_remote(self.mesh()?, &self.who.session, &kid, false)
                    .await
                    .map_err(|e| {
                        format!("{e} It is not stopped and still counts as running: try again.")
                    })?;
                self.agents.end_remote(&kid);
            }
        }

        Ok(ok(
            "Stopped. Its session ended and stays in the user's list.",
        ))
    }

    /// Every child of the caller, here and elsewhere.
    async fn list(&self) -> Reply {
        let here = self.my_name();
        let children = self.agents.children_of(self.who);
        self.shown(children.iter().map(|child| &**child));
        let mut views: Vec<ChildView> = children
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
    /// checked with agent_status instead. A child here wakes the wait through
    /// its `live`, one on another machine through its note; a node too old to
    /// note every settle is also read every `WAIT_RECHECK`.
    async fn waited(&self, view: ChildView) -> ChildView {
        // What this wait answers goes in no note.
        let waiting = self.agents.family.waits.hold(&self.who.session, &view.id);
        let (live, remote) = match self.find(&view.id) {
            Ok(Found::Here(child)) => (Some(child.live.clone()), false),
            Ok(Found::There(_)) => (None, true),
            Err(_) => (None, false),
        };
        let mut view = wait_for_settled(&waiting, live, remote, view, |id| async move { self.view_of(&id).await }).await;

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

/// How often a wait on a child on another machine that does not note every
/// settle (an older node) reads its view: a turn that ends without a note
/// (`family::notes`) costs at most this much. A node that does note retries
/// a note it could not send, so its wait is never read on a timer.
const WAIT_RECHECK: Duration = Duration::from_secs(5);

/// Reads `view` again with `read` each time `waiting`, `live` or, for a
/// `remote` child, the recheck says to,
/// until it is settled or `WAIT_LIMIT` is over.
async fn wait_for_settled<F>(
    waiting: &Waiting,
    mut live: Option<watch::Receiver<Live>>,
    remote: bool,
    mut view: ChildView,
    read: impl Fn(String) -> F,
) -> ChildView
where
    F: Future<Output = Result<ChildView, String>>,
{
    let deadline = sleep(WAIT_LIMIT);
    tokio::pin!(deadline);

    while !view.settled() {
        // A change before the hold and the clone went unseen.
        match read(view.id.clone()).await {
            Ok(next) => view = next,
            Err(e) => {
                view.note = Some(e);
                return view;
            }
        }

        if view.settled() {
            break;
        }

        let recheck = (remote && !view.notes_settles).then_some(WAIT_RECHECK);
        let gone = tokio::select! {
            () = waiting.woken() => false,
            changed = async {
                match live.as_mut() {
                    Some(live) => live.changed().await.is_err(),
                    None => std::future::pending().await,
                }
            } => changed,
            () = async {
                match recheck {
                    Some(every) => sleep(every).await,
                    None => std::future::pending().await,
                }
            } => false,
            () = &mut deadline => {
                view.note = Some(format!(
                    "Still working after {} seconds. Call agent_status with this id to \
                     check again.",
                    WAIT_LIMIT.as_secs()
                ));
                return view;
            }
        };

        if gone {
            live = None;
        }
    }

    view
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
            .ask_ok(Tool::ListAgents, &asked, LIST_LIMIT)
            .await
    }))
    .await;

    // A machine that does not answer, or an older one that lists no agents
    // for others, adds none.
    for ((machine, display), answer) in others.iter().zip(lists) {
        let Ok(text) = answer else { continue };
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

/// The custom agent called `name` for the folder `at` and the user's own,
/// the one for `harness` first (`custom_agents::persona`).
async fn find_persona(at: String, name: String, harness: String) -> Result<CustomAgent, String> {
    let home = dirs::home_dir();

    tokio::task::spawn_blocking(move || {
        let folder = crate::agents::running::folder(Some(&at)).map_err(|e| format!("{e:#}"))?;
        crate::agents::custom_agents::persona(&folder, home.as_deref(), &name, &harness)
    })
    .await
    .map_err(|e| format!("The custom agents were not read: {e}"))?
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
        line.split_once(": ")
            .map_or(line, |(_, prompt)| prompt)
            .to_owned()
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
        "{title} [{status}]\n[steps] {}\n[agent] {} on {}\n[child] {} {} {}",
        view.turns, view.harness, view.machine, view.id, view.state, view.key
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

async fn serve_peer(agents: &Agents, peer: EndpointId, request: &Request) -> Result<Reply, String> {
    let who = Who {
        machine: Some(peer),
        session: request.session.clone(),
    };
    // A node says it notes every settle only when it can send a note: one
    // with no mesh has nobody to tell, and its parent must keep reading.
    let notes_settles = agents.mesh().is_some();
    let child_view = |child: &Session| {
        json_of(&ChildView {
            notes_settles,
            ..ChildView::of(&child.info(), &Agents::name_of(child))
        })
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
                title: order.title,
                persona: None,
            };
            let child = agents.start_child(order.parent, start).await?;

            Ok(child_view(&child))
        }
        // A child's node asks the node of its parent whether it is kept: the
        // call's session is the parent here, and `peer` the machine of the child.
        Tool::AgentParent => {
            let call: PeerCall = parse(&request.args)?;
            let key = Into::<SessionId>::into(call.text.unwrap_or_default());
            let keeps = agents.keeps_child(peer, &request.session, &call.id, &key);

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
                Ok(child) => Agents::tell_child(&child, text).await?,
                Err(_) => agents.message(&who.session, &call.id, text).await?,
            }

            Ok(ok("Sent. It reads it at its next step."))
        }
        Tool::AgentStatus | Tool::StopAgent => {
            let call: PeerCall = parse(&request.args)?;
            let child = agents.child_of(&who, &call.id)?;

            Ok(match request.tool {
                Tool::AgentStatus => child_view(&child),
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
    let specs: Vec<SyncSpec> = super::projects::sync_specs(mesh)
        .into_iter()
        .map(|(_, spec)| spec)
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
