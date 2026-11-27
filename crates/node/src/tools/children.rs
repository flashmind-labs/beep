//! `start_agent` and its companions: an agent starts another agent, of any
//! harness, on any machine of the mesh, and follows it.
//!
//! The child is a normal session linked to the caller (see
//! `agents::family`). On this machine the node starts it itself; on another
//! it sends the order over `tools/1`, with the link, and the other node
//! starts it and checks the limits. An agent stops and checks only its own
//! children; it lists and messages any agent the user runs.

use super::Node;
use crate::agents::child_reach::{ChildId, Found, Reach};
use crate::agents::child_view::{ChildView, full_id};
use crate::agents::family::Family;
use crate::agents::machine_list::Machine;
use crate::agents::reply::{fail, json_of, ok, parse};
use crate::agents::sessions;
use crate::agents::{
    Agents, Session,
    family::{ChildNote, ChildStart, RemoteKid, WAIT_LIMIT, Waiting, Who},
    live::Live,
};
use beep_mesh::Mesh;
use beep_proto::SessionId;
use beep_proto::node::{
    agent::ParentLink,
    mesh_state::SyncSpec,
    tools::{Reply, Request, Tool},
};
use beep_proto::watched;
use beep_proto::{AccountName, CustomAgent, ModelId};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};

use std::future::Future;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{sync::watch, time::sleep};

use crate::agents::family::peer::{
    Answer, Call, CountCall, GONE, KEPT, PeerCall, REMOTE_LIMIT, fetch_view, in_the_mesh, listed,
    parse_view, stop_remote,
};

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

/// Answers one agent tool for the session that called it.
pub(super) async fn call(node: &Node, request: &Request) -> Reply {
    let agents = &node.agents;
    let Some((session, who)) = agents.family.caller(&request.session) else {
        return fail("This session is not one this node runs.");
    };
    let calls = Calls {
        agents,
        reach: Reach {
            family: &agents.family,
            mesh: agents.links.mesh(),
            who: &who,
        },
    };

    calls.dispatch(&session, request).await.unwrap_or_else(fail)
}

/// Answers an agent tool another node's agent called on this machine. The
/// caller's own node checked its side; the limits are checked here, from the
/// link the call carries.
pub(crate) async fn answer_peer(agents: &Agents, peer: EndpointId, request: Request) -> Reply {
    serve_peer(agents, peer, &request)
        .await
        .unwrap_or_else(fail)
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

/// What `StartArgs` hands on without the `wait`.
struct StartParts {
    harness: String,
    prompt: String,
    cwd: Option<String>,
    model: Option<String>,
    title: Option<String>,
    persona: Option<CustomAgent>,
}

/// The caller's reach and the node's agents the tools start children on.
struct Calls<'a> {
    agents: &'a Agents,
    reach: Reach<'a>,
}

impl Calls<'_> {
    /// `text` as the caller `sender` sends it, tagged with `id`, the id
    /// the receiver answers to, so its log tells it from the user's words.
    fn tagged(sender: &Session, id: &str, text: &str) -> String {
        let info = sender.info();
        let name = info.title.unwrap_or(info.agent);

        crate::tags::from_session(id, &name, text)
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

        let prompt = Self::tagged(parent, self.reach.who.session.as_ref(), &prompt);
        let target = self.reach.target(machine.as_deref())?;
        let cwd = match project {
            Some(project) => {
                if cwd.is_some() {
                    return Err("Give either `project` or `cwd`, not both.".to_owned());
                }

                let mesh = self
                    .reach
                    .mesh
                    .as_ref()
                    .ok_or("This machine is in no mesh, so it has no registered projects.")?;
                let on = target.as_ref().map_or(mesh.me(), |m| m.id);
                Some(crate::agents::projects::folder(mesh, &project, on)?)
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
            (None, Some(persona))
                if self
                    .agents
                    .catalog
                    .named(persona.harness.as_ref())
                    .is_some() =>
            {
                persona.harness.as_ref().to_owned()
            }
            (None, _) => parent.agent.clone(),
        };

        let link = ParentLink {
            background,
            ..self.agents.family.link_for(parent, self.reach.who)?
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

        Ok(self.reach.view_here(&child))
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
        let mesh = self.reach.mesh()?;
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
        let me = &self.reach.who.session;
        self.agents
            .links
            .adopt_remote(parent, RemoteKid::expected(me, machine.id, &machine.name));

        let text = match Call::to_machine(mesh, me, machine)
            .ask(Tool::StartAgent, &order, REMOTE_LIMIT)
            .await
        {
            Answer::Text(text) => text,
            Answer::Refused(why) => {
                self.agents.links.forget_expected(me, machine.id);
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
            .links
            .settle_remote(me, machine.id, &ChildId::from(view.id.as_str()).session);

        Ok(view)
    }

    async fn status(&self, id: &str) -> Result<Reply, String> {
        // A review's id: its reviewer is gone once it is done.
        if let Some(run) = self
            .agents
            .reviews
            .review_for(self.reach.who, &ChildId::from(id).session)
        {
            return Ok(super::review::standing(run));
        }

        Ok(json_of(&self.view_of(id).await?))
    }

    async fn view_of(&self, id: &str) -> Result<ChildView, String> {
        match self.reach.find(id)? {
            Found::Here(child) => Ok(self.reach.view_here(&child)),
            Found::There(kid) => self.view_there(&kid).await,
        }
    }

    async fn view_there(&self, kid: &RemoteKid) -> Result<ChildView, String> {
        let view = fetch_view(self.reach.mesh()?, &self.reach.who.session, kid).await?;

        if view.state == "ended" {
            self.agents.links.end_remote(kid);
        }

        Ok(view.named(&kid.name))
    }

    async fn send(&self, parent: &Session, id: &str, text: String) -> Result<Reply, String> {
        if text.trim().is_empty() {
            return Err("Give the `text` to send.".to_owned());
        }

        let Ok(found) = self.reach.find(id) else {
            return self.message(parent, id, &text).await;
        };
        let text = Self::tagged(parent, self.reach.who.session.as_ref(), &text);

        match found {
            Found::Here(child) => {
                Family::tell_child(&child, text).await?;
                Ok(ok("Sent. It reads it at its next step."))
            }
            Found::There(kid) => {
                let args = PeerCall::message(kid.id.clone(), text);
                let reply = Call::to_kid(self.reach.mesh()?, &self.reach.who.session, &kid)
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
            &full_id(self.reach.who.session.as_ref(), &self.reach.my_name()),
            text,
        );
        let id = ChildId::from(id);

        match self.reach.target(id.machine)? {
            None => {
                self.agents
                    .family
                    .message(&self.reach.who.session, &id.session, text)
                    .await?
            }
            Some(machine) => {
                let args = PeerCall::message(id.session, text);
                Call::to_machine(self.reach.mesh()?, &self.reach.who.session, &machine)
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
                    .reach
                    .mesh
                    .as_ref()
                    .ok_or("This machine is in no mesh, so it has no registered projects.")?;
                Some(crate::agents::projects::project_name(mesh, &name)?)
            }
        };
        let mut views: Vec<ChildView> = in_the_mesh(
            &self.agents.sessions,
            &self.agents.links,
            self.reach.mesh.as_ref(),
            &self.reach.who.session,
            None,
            None,
        )
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
        match self.reach.find(id)? {
            Found::Here(child) => child.close().await,
            Found::There(kid) => {
                // It keeps its place until its machine says it stopped.
                stop_remote(self.reach.mesh()?, &self.reach.who.session, &kid, false)
                    .await
                    .map_err(|e| {
                        format!("{e} It is not stopped and still counts as running: try again.")
                    })?;
                self.agents.links.end_remote(&kid);
            }
        }

        Ok(ok(
            "Stopped. Its session ended and stays in the user's list.",
        ))
    }

    /// Every child of the caller, here and elsewhere.
    async fn list(&self) -> Reply {
        let here = self.reach.my_name();
        let children = self.agents.sessions.children_of(self.reach.who);
        self.reach.shown(children.iter().map(|child| &**child));
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
        let mut kids = self.agents.links.remote_kids(&self.reach.who.session);
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
        let waiting = self
            .agents
            .family
            .hold_wait(&self.reach.who.session, &view.id);
        let (live, remote) = match self.reach.find(&view.id) {
            Ok(Found::Here(child)) => (Some(child.live.clone()), false),
            Ok(Found::There(_)) => (None, true),
            Err(_) => (None, false),
        };
        let mut view = wait_for_settled(&waiting, live, remote, view, |id| async move {
            self.view_of(&id).await
        })
        .await;

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

/// The custom agent called `name` for the folder `at` and the user's own,
/// the one for `harness` first (`custom_agents::persona`).
async fn find_persona(at: String, name: String, harness: String) -> Result<CustomAgent, String> {
    let home = dirs::home_dir();

    tokio::task::spawn_blocking(move || {
        let folder =
            crate::machine::env::locate::folder(Some(&at)).map_err(|e| format!("{e:#}"))?;
        crate::custom_agents::persona(&folder, home.as_deref(), &name, &harness)
    })
    .await
    .map_err(|e| format!("The custom agents were not read: {e}"))?
}

async fn serve_peer(agents: &Agents, peer: EndpointId, request: &Request) -> Result<Reply, String> {
    let who = Who {
        machine: Some(peer),
        session: request.session.clone(),
    };
    // A node says it notes every settle only when it can send a note: one
    // with no mesh has nobody to tell, and its parent must keep reading.
    let notes_settles = agents.links.mesh().is_some();
    let child_view = |child: &Session| {
        json_of(&ChildView {
            notes_settles,
            ..ChildView::of(&child.info(), &sessions::name_of(child))
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
            let keeps = agents
                .family
                .keeps_child(peer, &request.session, &call.id, &key);

            Ok(ok(if keeps { KEPT } else { GONE }))
        }
        // A child on `peer` finished a turn or asks the user: the call's
        // session is its parent here.
        Tool::AgentNote => {
            let note: ChildNote = parse(&request.args)?;
            agents
                .family
                .note_from(peer, &request.session, &note)
                .await?;

            Ok(ok("Told."))
        }
        // How many live agents under a root run here.
        Tool::AgentCount => {
            let call: CountCall = parse(&request.args)?;

            Ok(ok(agents
                .sessions
                .live_descendants(call.root_machine, &call.root)
                .to_string()))
        }
        // Another machine's agent lists the agents here.
        Tool::ListAgents => Ok(listed(&agents.sessions, &agents.links, &request.args)),
        // To its own child, or to any agent here as a message from another.
        Tool::SendToAgent => {
            let call: PeerCall = parse(&request.args)?;
            let Some(text) = call.text.filter(|text| !text.trim().is_empty()) else {
                return Ok(fail("Give the `text` to send."));
            };

            match agents.sessions.child_of(&who, &call.id) {
                Ok(child) => Family::tell_child(&child, text).await?,
                Err(_) => agents.family.message(&who.session, &call.id, text).await?,
            }

            Ok(ok("Sent. It reads it at its next step."))
        }
        Tool::AgentStatus | Tool::StopAgent => {
            let call: PeerCall = parse(&request.args)?;
            let child = agents.sessions.child_of(&who, &call.id)?;

            Ok(match request.tool {
                Tool::AgentStatus => child_view(&child),
                _ if call.turn_only => {
                    // Refused when no turn runs. A parent's node from 0.5.0
                    // and before asks without `stop_all`.
                    agents.family.stop_turn(&child, call.stop_all).await;
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
    let specs: Vec<SyncSpec> = crate::agents::projects::sync_specs(mesh)
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
#[path = "../tests/tools_children.rs"]
mod tests;
