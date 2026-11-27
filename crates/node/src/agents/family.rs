//! Agents that start agents (`start_agent`): the link from a child session
//! to its parent, the limits on how far and how wide that goes, who may
//! reach whom, and what happens to children when the parent stops.
//!
//! A child is a normal session. Its `SessionInfo::parent` says who started
//! it; the parent's node keeps only the children that run on other machines
//! (`RemoteKid`). Depth and count are checked by the node that starts the
//! child, from the link the caller passes, so a chain across machines
//! cannot get around them.

use beep_proto::SessionId;
use super::{
    Agents, Begin, Command, Session, known, live::{Live, Mark}, session::ended_for_good,
};
use crate::membership::Membership;
use beep_proto::{AccountName, AgentAccount, CustomAgent, ModelId};
use beep_proto::node::agent::{Effort, ParentLink, Persona, SessionInfo, Status};
use beep_proto::watched;
use iroh::EndpointId;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use futures::{StreamExt, stream::FuturesUnordered};
use tokio::sync::{
    SetOnce,
    mpsc::{UnboundedReceiver, UnboundedSender},
    watch,
};
use tokio_util::task::AbortOnDropHandle;

mod changes;
pub(crate) mod notes;
pub(crate) mod peer;
mod orphans;
mod remote;


pub(crate) use notes::{ChildNote, Waiting, Waits};
pub(crate) use orphans::Parent;
pub(crate) use remote::RemoteKid;

/// How many starts deep a chain may go: a session started by a session
/// started by a session started by one the user began.
pub(crate) const MAX_DEPTH: u8 = 3;
/// The most live descendants one root session may have, and the most direct
/// children one session may have.
pub(crate) const MAX_CHILDREN: usize = 8;
/// The session a chain of starts began from, and its machine.
type Root = (Option<EndpointId>, SessionId);
/// How long the count of a root's agents on another machine may take.
const COUNT_LIMIT: Duration = Duration::from_secs(3);
/// What a machine says when it has no child of that name for the caller: also
/// what stopping one that is already gone reads as success on.
pub(crate) const NO_AGENT: &str = "You have no agent";

/// What the node knows of the family trees it takes part in.
#[derive(Default)]
pub(crate) struct Family {
    remote: Arc<Mutex<Vec<RemoteKid>>>,
    membership: Arc<SetOnce<Membership>>,
    /// Per parent with children elsewhere, the task that stops them when it
    /// ends for good.
    watchers: Mutex<HashMap<SessionId, AbortOnDropHandle<()>>>,
    /// The child starts under way here, each under its root. They count
    /// with the root's live agents, so two starts cannot both take the last
    /// place.
    starting: Mutex<Vec<(Root, Weak<()>)>>,
    /// What transcripts show of children on other machines.
    pub(crate) views: peer::Views,
    /// The children parents wait on in `start_agent`, which send no note
    /// meanwhile.
    pub(crate) waits: Waits,
    /// The background children that closed for good, to delete.
    closed: Tidy,
}

/// A background child that closed for good.
pub(super) struct Closed {
    agent: String,
    id: SessionId,
}

/// Where closed background children report, and the end that deletes them,
/// taken once by the upkeep.
pub(super) struct Tidy {
    tx: UnboundedSender<Closed>,
    rx: Mutex<Option<UnboundedReceiver<Closed>>>,
}

impl Default for Tidy {
    fn default() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            tx,
            rx: Mutex::new(Some(rx)),
        }
    }
}

impl Family {
    /// The receiver of the closed background children, for the one task that
    /// deletes them.
    pub(super) fn take_closed(&self) -> Option<UnboundedReceiver<Closed>> {
        crate::lock(&self.closed.rx).take()
    }
}

/// Who calls a family tool: a session of some machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Who {
    /// `None` for a node that belongs to no mesh.
    pub(crate) machine: Option<EndpointId>,
    /// The session's id as its children's links name it.
    pub(crate) session: SessionId,
}

/// What a child starts with, from the call that starts it.
#[derive(Debug)]
pub(crate) struct ChildStart {
    pub(crate) harness: String,
    pub(crate) prompt: String,
    pub(crate) cwd: Option<String>,
    pub(crate) model: Option<ModelId>,
    /// A temporary reviewer, started in its harness's read-only mode.
    pub(crate) review: Option<Review>,
    /// The named account its parent runs as. A child of an agent that keeps
    /// accounts runs as that agent's account of the same name, or not at all.
    pub(crate) account: Option<AccountName>,
    /// The name its parent gives it, which its chat lists it under.
    pub(crate) title: Option<String>,
    /// The custom agent it runs as, whose instructions it is told.
    pub(crate) persona: Option<CustomAgent>,
}

/// What a reviewer child starts with besides `ChildStart`'s.
#[derive(Debug, Default)]
pub(crate) struct Review {
    pub(crate) effort: Option<Effort>,
    pub(crate) fast: Option<bool>,
}

impl Agents {
    /// Lets the family reach other machines, once the node serves the Beep
    /// MCP server. Only the first call counts.
    pub(crate) fn serve_mesh(&self, membership: Membership) {
        let _ = self.family.membership.set(membership);
    }

    /// The mesh, when this node belongs to one.
    pub(crate) fn mesh(&self) -> Option<beep_mesh::Mesh> {
        self.family.membership.get().and_then(Membership::get)
    }

    /// This machine's id in its mesh.
    fn me(&self) -> Option<EndpointId> {
        self.mesh().map(|mesh| mesh.me())
    }

    /// The id a session goes by in links: the agent's own when it has one,
    /// which outlives a restart.
    pub(crate) fn name_of(session: &Session) -> SessionId {
        watched::read(&session.live, |live| live.agent_session.clone())
            .unwrap_or_else(|| session.id.clone())
    }

    /// The calling session `session` of this node, and who it is to its
    /// children.
    pub(crate) fn caller(&self, session: &SessionId) -> Option<(Arc<Session>, Who)> {
        let found = self.get(session)?;
        let who = self.who_of(&found);
        Some((found, who))
    }

    /// Who `session` is to its children.
    pub(super) fn who_of(&self, session: &Session) -> Who {
        Who {
            machine: self.me(),
            session: Self::name_of(session),
        }
    }

    /// The link a child of `parent` carries, or why it may not be started.
    pub(crate) fn link_for(&self, parent: &Session, who: &Who) -> Result<ParentLink, String> {
        let info = parent.info();
        let above = info.parent.as_ref();
        let depth = above.map_or(0, |link| link.depth).saturating_add(1);

        if depth > MAX_DEPTH {
            return Err(too_deep());
        }

        if self.children_count(who) >= MAX_CHILDREN {
            return Err(too_many(&format!("this agent has {MAX_CHILDREN} children")));
        }

        let (root_machine, root) = match above {
            Some(link) => (link.root_machine, link.root.clone()),
            None => (who.machine, who.session.clone()),
        };
        Ok(ParentLink {
            machine: who.machine,
            session: who.session.clone(),
            key: parent.id.clone(),
            root_machine,
            root,
            depth,
            others: 0,
            agent: info.agent.clone(),
            mode: held_to(&info)?,
            background: false,
        })
    }

    /// The live sessions on other machines that descend from `root`, as
    /// each online machine says of its own. A machine that does not answer
    /// in time counts none.
    async fn live_elsewhere(&self, root: &Root) -> usize {
        let Some(mesh) = self.mesh() else {
            return 0;
        };
        let me = mesh.me();
        let there = mesh
            .code_hosts()
            .into_iter()
            .filter(|status| status.online && status.device.id != me)
            .map(|status| peer::count_on(&mesh, status.device.id, root.0, &root.1, COUNT_LIMIT));

        futures::future::join_all(there).await.into_iter().sum()
    }

    /// Takes a place under `root` for a child that is starting, held until
    /// what it returns is dropped. Refused when the root's live agents,
    /// `elsewhere` and here, and the starts under way here fill it.
    fn take_place(&self, root: &Root, elsewhere: usize) -> Result<Arc<()>, String> {
        let mut starting = crate::lock(&self.family.starting);
        starting.retain(|(_, held)| held.strong_count() > 0);
        let here = self.live_descendants(root.0, &root.1)
            + starting.iter().filter(|(under, _)| under == root).count();

        if elsewhere + here >= MAX_CHILDREN {
            return Err(too_many(&format!(
                "the session this chain started from has {MAX_CHILDREN} agents running"
            )));
        }

        let held = Arc::new(());
        starting.push((root.clone(), Arc::downgrade(&held)));
        Ok(held)
    }

    /// The live sessions here that descend from the root `root`.
    pub(crate) fn live_descendants(&self, root_machine: Option<EndpointId>, root: &SessionId) -> usize {
        self.session_map()
            .values()
            .filter(|session| {
                watched::read(&session.live, |live| {
                    live.status() != Status::Ended
                        && live.info.parent.as_ref().is_some_and(|link| {
                            link.root == *root && link.root_machine == root_machine
                        })
                })
            })
            .count()
    }

    /// The live children of `who` that run here.
    pub(crate) fn children_of(&self, who: &Who) -> Vec<Arc<Session>> {
        let mut kids: Vec<Arc<Session>> = self
            .session_map()
            .values()
            .filter(|session| watched::read(&session.live, |live| is_child_of(live, who)))
            .cloned()
            .collect();
        kids.sort_by_key(|session| watched::read(&session.live, |live| live.info.started_at));
        kids
    }

    fn children_count(&self, who: &Who) -> usize {
        let local = self
            .children_of(who)
            .iter()
            .filter(|session| watched::read(&session.live, |live| live.status() != Status::Ended))
            .count();
        let remote = self
            .remote_kids(&who.session)
            .iter()
            .filter(|kid| kid.counts())
            .count();
        local + remote
    }

    /// The child `id` of `who` that runs here; the same refusal for a
    /// session that is not there and one that is not `who`'s, so an agent
    /// learns nothing of the others.
    pub(crate) fn child_of(&self, who: &Who, id: &SessionId) -> Result<Arc<Session>, String> {
        self.get(id)
            .filter(|session| watched::read(&session.live, |live| is_child_of(live, who)))
            .ok_or_else(|| format!("{NO_AGENT} {id}."))
    }

    /// The live sessions here that other agents may list and message: every
    /// one the user sees, oldest first; with those that ended and were last
    /// active at or after `ended_since`, Unix seconds, when given.
    pub(super) fn peers(&self, ended_since: Option<u64>) -> Vec<Arc<Session>> {
        let mut peers: Vec<Arc<Session>> = self
            .session_map()
            .values()
            .filter(|session| {
                !session.reviewer
                    && watched::read(&session.live, |live| {
                        let shown = match live.status() {
                            Status::Ended => ended_since
                                .is_some_and(|since| live.info.last_activity >= since),
                            _ => true,
                        };
                        live.agent_session.is_some() && shown
                    })
            })
            .cloned()
            .collect();
        peers.sort_by_key(|session| watched::read(&session.live, |live| live.info.started_at));
        peers
    }

    /// Hands `text`, from the agent `from`, to the live session `id` here,
    /// which counts it against `PEER_MESSAGES`.
    pub(crate) async fn message(&self, from: &SessionId, id: &SessionId, text: String) -> Result<(), String> {
        let session = self
            .peers(None)
            .into_iter()
            .find(|session| session.id == *id || Self::name_of(session) == *id)
            .filter(|session| Self::name_of(session) != *from)
            .ok_or_else(|| format!("No agent {id} runs there. Use list_agents to find one."))?;

        session.take_peer_message()?;
        session.order(Command::Steer(text.into())).await
    }

    /// Hands `text`, from its parent, to the child `child` here. What the
    /// child does with it is the parent's to hear the end of, whoever began
    /// the turn it joins.
    pub(crate) async fn tell_child(child: &Session, text: String) -> Result<(), String> {
        child.set_user_turn(false).await;
        child.order(Command::Steer(text.into())).await
    }

    /// Starts the child `link` describes, here. The node that starts it
    /// checks the limits, whichever machine the parent is on.
    pub(crate) async fn start_child(
        &self,
        link: ParentLink,
        start: ChildStart,
    ) -> Result<Arc<Session>, String> {
        if link.depth == 0 || link.depth > MAX_DEPTH {
            return Err(too_deep());
        }

        let kind = self.kind_or_refusal(&start.harness)?;
        let mode = match &start.review {
            Some(_) => review_mode(&link, &kind.name)?,
            None => child_mode(&link, &kind.name)?,
        };
        let follows = self.parent_live_of(&link);
        let parent = (link.machine == self.me())
            .then(|| self.get(&link.session))
            .flatten();
        let root = (link.root_machine, link.root.clone());
        let title = start.title.filter(|title| !title.trim().is_empty());
        let begin = Begin {
            prompt: Some(start.prompt),
            named: title.is_some(),
            title,
            model: start.model,
            effort: start.review.as_ref().and_then(|review| review.effort),
            fast: start.review.as_ref().and_then(|review| review.fast),
            mode,
            parent: Some(link),
            follows,
            review: start.review.is_some(),
            account: start
                .account
                .filter(|_| AgentAccount::from_str(&kind.name).is_ok()),
            persona: start.persona.as_ref().map(Into::<Persona>::into),
            instructions: start.persona.as_ref().map(crate::custom_agents::persona_brief),
            ..Begin::default()
        };
        let elsewhere = self.live_elsewhere(&root).await;
        // Held until the child is listed. It then counts twice for an
        // instant, which can refuse a start but never admit one too many.
        let place = self.take_place(&root, elsewhere)?;
        let session = self
            .start_in(&kind, start.cwd.as_deref(), None, begin)
            .await
            .map_err(|e| format!("{e:#}"))?;
        drop(place);
        self.watch_parent(&session);

        // Its prompt says what the parent knew: it has seen the parent so far.
        if let Some(parent) = parent {
            session
                .see(vec![(
                    parent.id.clone(),
                    watched::read(&parent.live, |live| Into::<Mark>::into(live)),
                )])
                .await;
        }

        session.named_info().await?;
        Ok(session)
    }

    /// Deletes a background child that closed for good, all those that did
    /// together. One that cannot be deleted is still dropped from the node's
    /// sessions.
    pub(super) async fn delete_closed_background(&self, mut closed: UnboundedReceiver<Closed>) {
        let mut deleting = FuturesUnordered::new();

        loop {
            tokio::select! {
                Some(Closed { agent, id }) = closed.recv() => deleting.push(async move {
                    if let Err(e) = self.delete_session(&agent, &id).await {
                        log::warn!("background child {id} was not deleted: {e:#}");
                        self.sessions.remove(std::slice::from_ref(&id));
                    }
                }),
                Some(()) = deleting.next() => crate::events::open_changed(),
                else => return,
            }
        }
    }

    /// Has `session`, a background child, say when it closes for good.
    pub(super) fn tidy_when_closed(&self, session: &Session) {
        let (closed, mut live) = (self.family.closed.tx.clone(), session.live.clone());
        let (agent, id) = (session.agent.clone(), session.id.clone());
        let task = tokio::spawn(async move {
            if watched::wait_for(&mut live, ended_for_good).await.is_ok() {
                let _ = closed.send(Closed { agent, id });
            }
        });
        session.keep(task);
    }

    /// Puts `session`, which came from another machine, in its family
    /// here: it tells its parent how it stands, and its children that came
    /// before it tell it and end with it, as those started after it do.
    pub(in crate::agents) fn join_family(&self, session: &Arc<Session>) {
        self.watch_parent(session);

        for child in self.children_of(&self.who_of(session)) {
            self.watch_parent(&child);
            child.follow(session.live.clone());
        }
    }

    /// The parent's state, when it runs here: the child ends with it.
    pub(super) fn parent_live_of(&self, link: &ParentLink) -> Option<watch::Receiver<Live>> {
        (link.machine == self.me())
            .then(|| self.get(&link.session))
            .flatten()
            .map(|parent| parent.live.clone())
    }

    /// Stops everything `session` started, here and on other machines, to
    /// go with the parent's own Stop all. They stop at once,
    /// side by side, so one machine that does not answer holds up none of
    /// the others; what already ended is left alone.
    pub(crate) async fn cancel_family(&self, session: &Arc<Session>) {
        let mut local: Vec<Arc<Session>> = Vec::new();
        let mut remote: Vec<(SessionId, RemoteKid)> = Vec::new();
        let mut queue = vec![session.clone()];

        while let Some(next) = queue.pop() {
            // The tree is bounded by the limits; this guards a loop in links.
            if local.len() + remote.len() > MAX_CHILDREN * usize::from(MAX_DEPTH) {
                break;
            }

            let who = self.who_of(&next);

            for kid in self.children_of(&who) {
                if watched::read(&kid.live, |live| live.status() != Status::Ended) {
                    queue.push(kid.clone());
                    local.push(kid);
                }
            }

            for kid in self.remote_kids(&who.session) {
                if kid.counts() && !kid.pending() {
                    remote.push((who.session.clone(), kid));
                }
            }
        }

        let mesh = self.mesh();
        let remote = remote.iter().map(|(parent, kid)| async {
            let Some(mesh) = &mesh else {
                return;
            };

            if let Err(e) = peer::stop_remote(mesh, parent, kid, true).await {
                log::warn!("child {} on {} was not stopped: {e}", kid.id, kid.name);
            }
        });
        let local = local.iter().map(|kid| async {
            // Refused when the child has ended.
            let _ = kid.order(Command::StopAll { by_user: false }).await;
        });
        futures::future::join(
            futures::future::join_all(remote),
            futures::future::join_all(local),
        )
        .await;
    }
}

/// Whether `live` is a child of `who`.
fn is_child_of(live: &Live, who: &Who) -> bool {
    live.info
        .parent
        .as_ref()
        .is_some_and(|link| link.session == who.session && link.machine == who.machine)
}

/// The mode a parent is held to: the id of the mode it is in when that is
/// not its harness's full access mode. An agent that offers no modes asks
/// nothing, so nothing is held. One that offers modes but has not said which
/// it is in is refused: its mode is unknown, so a child could outrun it.
fn held_to(info: &SessionInfo) -> Result<Option<String>, String> {
    let Some(mode) = &info.mode else {
        return if info.modes.is_empty() {
            Ok(None)
        } else {
            Err(
                "Not started: the calling agent has not said which mode it is in yet. Try again \
                 in a moment."
                    .to_owned(),
            )
        };
    };
    let full = known(&info.agent).and_then(|known| known.full_mode);
    Ok(Some(mode.clone()).filter(|mode| Some(mode.as_str()) != full))
}

/// The mode a child starts in. A parent held below full access passes only
/// its own mode on, to a child of the same harness, so a child is never
/// freer than its parent.
fn child_mode(link: &ParentLink, harness: &str) -> Result<Option<String>, String> {
    match &link.mode {
        None => Ok(None),
        Some(mode) if link.agent == harness => Ok(Some(mode.clone())),
        Some(mode) => Err(format!(
            "The calling agent runs in {mode} mode, below full access, and a {harness} agent has \
             no matching mode. Start a {} agent, or ask the user to switch the caller to full \
             access.",
            link.agent
        )),
    }
}

/// The mode a reviewer child starts in: its harness's read-only mode, which
/// is never above the parent's, else what any child of the parent gets.
pub(super) fn review_mode(link: &ParentLink, harness: &str) -> Result<Option<String>, String> {
    match known(harness).and_then(|known| known.read_only_mode) {
        Some(mode) => Ok(Some(mode.to_owned())),
        None => child_mode(link, harness),
    }
}

fn too_deep() -> String {
    format!(
        "Not started: agents may start agents at most {MAX_DEPTH} levels deep, and this one is \
         at the limit."
    )
}

fn too_many(what: &str) -> String {
    format!("Not started: at most {MAX_CHILDREN} agents may run under one session, and {what}.")
}

/// How long `wait` waits at most on a child or a review.
pub(crate) const WAIT_LIMIT: Duration = Duration::from_secs(45);

#[cfg(test)]
#[path = "../tests/agents_family.rs"]
mod tests;
