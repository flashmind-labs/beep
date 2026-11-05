//! Agents that start agents (`start_agent`): the link from a child session
//! to its parent, the limits on how far and how wide that goes, who may
//! reach whom, and what happens to children when the parent stops.
//!
//! A child is a normal session. Its `SessionInfo::parent` says who started
//! it; the parent's node keeps only the children that run on other machines
//! (`RemoteKid`). Depth and count are checked by the node that starts the
//! child, from the link the caller passes, so a chain across machines
//! cannot get around them.

use super::{AGENT_ID_WAIT, Agents, Begin, Command, Session, known, live::Live, mcp::children};
use crate::{AsyncMutex, link::Membership};
use beep_proto::ModelId;
use beep_proto::node::agent::{ParentLink, SessionInfo, Status};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{sync::watch, time::timeout};
use tokio_util::task::AbortOnDropHandle;

mod orphans;
mod remote;

pub(crate) use orphans::Parent;
pub(crate) use remote::RemoteKid;

/// How many starts deep a chain may go: a session started by a session
/// started by a session started by one the user began.
pub(crate) const MAX_DEPTH: u8 = 3;
/// The most live descendants one root session may have, and the most direct
/// children one session may have.
pub(crate) const MAX_CHILDREN: usize = 8;
/// How long the count of a root's agents on another machine may take.
const COUNT_LIMIT: Duration = Duration::from_secs(3);
/// What a machine says when it has no child of that name for the caller: also
/// what stopping one that is already gone reads as success on.
pub(crate) const NO_AGENT: &str = "You have no agent";

/// What the node knows of the family trees it takes part in.
#[derive(Default)]
pub(super) struct Family {
    remote: Arc<Mutex<Vec<RemoteKid>>>,
    membership: Arc<OnceLock<Membership>>,
    /// Per parent with children elsewhere, the task that stops them when it
    /// ends for good.
    watchers: Mutex<HashMap<String, AbortOnDropHandle<()>>>,
    /// Held from counting a root's agents until the child exists, so two
    /// starts here cannot both take the last place.
    starting: AsyncMutex<()>,
    /// What transcripts show of children on other machines.
    pub(crate) views: children::Views,
}

/// Who calls a family tool: a session of some machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Who {
    /// `None` for a node that belongs to no mesh.
    pub(crate) machine: Option<EndpointId>,
    /// The session's id as its children's links name it.
    pub(crate) session: String,
}

/// What a child starts with, from the call that starts it.
#[derive(Debug)]
pub(crate) struct ChildStart {
    pub(crate) harness: String,
    pub(crate) prompt: String,
    pub(crate) cwd: Option<String>,
    pub(crate) model: Option<ModelId>,
}

impl Agents {
    /// Lets the family reach other machines, once the node serves the Beep
    /// MCP server. Only the first call counts.
    pub(super) fn serve_mesh(&self, membership: Membership) {
        let _ = self.family.membership.set(membership);
    }

    /// The mesh, when this node belongs to one.
    pub(super) fn mesh(&self) -> Option<beep_mesh::Mesh> {
        self.family.membership.get().and_then(Membership::get)
    }

    /// This machine's id in its mesh.
    fn me(&self) -> Option<EndpointId> {
        self.mesh().map(|mesh| mesh.me())
    }

    /// The id a session goes by in links: the agent's own when it has one,
    /// which outlives a restart.
    pub(super) fn name_of(session: &Session) -> String {
        let live = session.live.borrow();
        live.agent_session
            .clone()
            .unwrap_or_else(|| session.id.clone())
    }

    /// The calling session `session` of this node, and who it is to its
    /// children.
    pub(super) fn caller(&self, session: &str) -> Option<(Arc<Session>, Who)> {
        let found = self.get(session)?;
        let who = Who {
            machine: self.me(),
            session: Self::name_of(&found),
        };
        Some((found, who))
    }

    /// Who `session` is to its children.
    fn who_of(&self, session: &Session) -> Who {
        Who {
            machine: self.me(),
            session: Self::name_of(session),
        }
    }

    /// The link a child of `parent` carries, or why it may not be started.
    pub(super) fn link_for(&self, parent: &Session, who: &Who) -> Result<ParentLink, String> {
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
        })
    }

    /// The live sessions on every machine that descend from the root `root`:
    /// this node's own, and what each online machine says of its own. A
    /// machine that does not answer in time counts none.
    async fn live_under(&self, root_machine: Option<EndpointId>, root: &str) -> usize {
        let here = self.live_descendants(root_machine, root);
        let Some(mesh) = self.mesh() else {
            return here;
        };
        let me = mesh.me();
        let there = mesh
            .devices()
            .into_iter()
            .filter(|status| status.online && status.device.id != me)
            .map(|status| {
                children::count_on(&mesh, status.device.id, root_machine, root, COUNT_LIMIT)
            });
        let counts = futures::future::join_all(there).await;
        here + counts.into_iter().sum::<usize>()
    }

    /// The live sessions here that descend from the root `root`.
    pub(crate) fn live_descendants(&self, root_machine: Option<EndpointId>, root: &str) -> usize {
        self.lock()
            .values()
            .filter(|session| {
                let live = session.live.borrow();
                live.status() != Status::Ended
                    && live
                        .info
                        .parent
                        .as_ref()
                        .is_some_and(|link| link.root == root && link.root_machine == root_machine)
            })
            .count()
    }

    /// The live children of `who` that run here.
    pub(super) fn children_of(&self, who: &Who) -> Vec<Arc<Session>> {
        let mut kids: Vec<Arc<Session>> = self
            .lock()
            .values()
            .filter(|session| is_child_of(&session.live.borrow(), who))
            .cloned()
            .collect();
        kids.sort_by_key(|session| session.live.borrow().info.started_at);
        kids
    }

    fn children_count(&self, who: &Who) -> usize {
        let local = self
            .children_of(who)
            .iter()
            .filter(|session| session.live.borrow().status() != Status::Ended)
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
    pub(super) fn child_of(&self, who: &Who, id: &str) -> Result<Arc<Session>, String> {
        self.get(id)
            .filter(|session| is_child_of(&session.live.borrow(), who))
            .ok_or_else(|| format!("{NO_AGENT} {id}."))
    }

    /// Starts the child `link` describes, here. The node that starts it
    /// checks the limits, whichever machine the parent is on.
    pub(super) async fn start_child(
        &self,
        link: ParentLink,
        start: ChildStart,
    ) -> Result<Arc<Session>, String> {
        if link.depth == 0 || link.depth > MAX_DEPTH {
            return Err(too_deep());
        }
        let kind = self
            .kinds
            .iter()
            .find(|kind| kind.name == start.harness)
            .ok_or_else(|| {
                let have: Vec<&str> = self.kinds.iter().map(|kind| kind.name.as_str()).collect();
                format!(
                    "This machine has no {} agent. It has: {}.",
                    start.harness,
                    have.join(", ")
                )
            })?
            .clone();
        let mode = child_mode(&link, &kind.name)?;
        let follows = self.parent_live_of(&link);
        let root = (link.root_machine, link.root.clone());
        let begin = Begin {
            prompt: Some(start.prompt),
            model: start.model,
            mode,
            parent: Some(link),
            follows,
            ..Begin::default()
        };
        let session = {
            // From counting to the child's place in the list, one start at a time.
            let _starting = self.family.starting.lock().await;
            if self.live_under(root.0, &root.1).await >= MAX_CHILDREN {
                return Err(too_many(&format!(
                    "the session this chain started from has {MAX_CHILDREN} agents running"
                )));
            }
            self.start_in(&kind, start.cwd.as_deref(), None, begin)
                .await
                .map_err(|e| format!("{e:#}"))?
        };
        self.watch_parent(&session);
        // The child goes by the agent's own id once it has one.
        let mut live = session.live.clone();
        let named =
            live.wait_for(|live| live.agent_session.is_some() || live.status() == Status::Ended);
        if timeout(AGENT_ID_WAIT, named).await.is_err() {
            log::info!("child session {} goes by its short id", session.id);
        }
        Ok(session)
    }

    /// The parent's state, when it runs here: the child ends with it.
    pub(super) fn parent_live_of(&self, link: &ParentLink) -> Option<watch::Receiver<Live>> {
        (link.machine == self.me())
            .then(|| self.get(&link.session))
            .flatten()
            .map(|parent| parent.live.clone())
    }

    /// Stops the running turns of everything `session` started, here and on
    /// other machines, to go with the parent's own Stop. They stop at once,
    /// side by side, so one machine that does not answer holds up none of
    /// the others; what already ended is left alone.
    pub(in crate::agents) async fn cancel_family(&self, session: &Arc<Session>) {
        let mut local: Vec<Arc<Session>> = Vec::new();
        let mut remote: Vec<(String, RemoteKid)> = Vec::new();
        let mut queue = vec![session.clone()];
        while let Some(next) = queue.pop() {
            // The tree is bounded by the limits; this guards a loop in links.
            if local.len() + remote.len() > MAX_CHILDREN * usize::from(MAX_DEPTH) {
                break;
            }
            let who = self.who_of(&next);
            for kid in self.children_of(&who) {
                if kid.live.borrow().status() != Status::Ended {
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
            if let Err(e) = children::stop_remote(mesh, parent, kid, true).await {
                log::warn!("child {} on {} was not stopped: {e}", kid.id, kid.name);
            }
        });
        let local = local.iter().map(|kid| async {
            // Refused when the child has no turn running.
            let _ = kid.order(Command::Cancel).await;
        });
        futures::future::join(
            futures::future::join_all(remote),
            futures::future::join_all(local),
        )
        .await;
    }

    /// Starts children first on a restart: a parent before its children.
    pub(super) fn parents_first(sessions: &mut [super::OpenSession]) {
        sessions.sort_by_key(|open| open.depth());
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

/// Whether the parent ended for good: not just with the node, which
/// resumes it, and its children, after the restart.
fn ended_for_good(live: &Live) -> bool {
    live.status() == Status::Ended
        && live.info.state.error.as_deref() != Some(super::session::NODE_STOPPED)
}

/// Waits until the parent session ends for good; false when it ends with
/// the node.
pub(super) async fn parent_gone(parent: &mut watch::Receiver<Live>) -> bool {
    match parent.wait_for(|live| live.status() == Status::Ended).await {
        Ok(live) => ended_for_good(&live),
        // The session was dropped.
        Err(_) => true,
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

/// How long a settled child is polled at most by `wait`, and how often.
pub(crate) const WAIT_LIMIT: Duration = Duration::from_secs(45);
pub(crate) const WAIT_POLL: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests;
