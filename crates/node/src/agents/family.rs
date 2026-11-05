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
use crate::link::Membership;
use beep_proto::ModelId;
use beep_proto::node::agent::{ParentLink, SessionInfo, Status};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{sync::watch, time::timeout};

/// How many starts deep a chain may go: a session started by a session
/// started by a session started by one the user began.
pub(crate) const MAX_DEPTH: u8 = 3;
/// The most live descendants one root session may have, and the most direct
/// children one session may have.
pub(crate) const MAX_CHILDREN: usize = 8;

/// A child that runs on another machine, as its parent's node keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemoteKid {
    /// The parent's id as its children's links name it.
    pub(crate) parent: String,
    pub(crate) machine: EndpointId,
    /// The machine as agents name it in `list_machines`.
    pub(crate) name: String,
    /// The child's id on that machine.
    pub(crate) id: String,
    /// It ended or was stopped: it stays readable but no longer counts.
    #[serde(default)]
    pub(crate) ended: bool,
}

/// What the node knows of the family trees it takes part in.
#[derive(Default)]
pub(super) struct Family {
    remote: Mutex<Vec<RemoteKid>>,
    membership: OnceLock<Membership>,
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
    /// `across` when the child runs on another machine, where this node's
    /// own count of the root's descendants is passed on.
    pub(super) fn link_for(
        &self,
        parent: &Session,
        who: &Who,
        across: bool,
    ) -> Result<ParentLink, String> {
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
        let mut others = above.map_or(0, |link| link.others);
        if across {
            let here = self.live_descendants(root_machine, &root);
            others = others.saturating_add(u8::try_from(here).unwrap_or(u8::MAX));
        }
        Ok(ParentLink {
            machine: who.machine,
            session: who.session.clone(),
            key: parent.id.clone(),
            root_machine,
            root,
            depth,
            others,
            agent: info.agent.clone(),
            mode: held_to(&info),
        })
    }

    /// The live sessions here that descend from the root `root`.
    fn live_descendants(&self, root_machine: Option<EndpointId>, root: &str) -> usize {
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
            .filter(|kid| !kid.ended)
            .count();
        local + remote
    }

    /// The child `id` of `who` that runs here; the same refusal for a
    /// session that is not there and one that is not `who`'s, so an agent
    /// learns nothing of the others.
    pub(super) fn child_of(&self, who: &Who, id: &str) -> Result<Arc<Session>, String> {
        self.get(id)
            .filter(|session| is_child_of(&session.live.borrow(), who))
            .ok_or_else(|| format!("You have no agent {id}."))
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
        let here = self.live_descendants(link.root_machine, &link.root);
        if usize::from(link.others) + here >= MAX_CHILDREN {
            return Err(too_many(&format!(
                "the session this chain started from has {MAX_CHILDREN} agents running"
            )));
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
        let begin = Begin {
            prompt: Some(start.prompt),
            model: start.model,
            mode,
            parent: Some(link),
            follows,
            ..Begin::default()
        };
        let session = self
            .start_in(&kind, start.cwd.as_deref(), None, begin)
            .await
            .map_err(|e| format!("{e:#}"))?;
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

    /// Notes a child that runs on `machine`, and has it stopped with its
    /// parent.
    pub(super) fn adopt_remote(&self, parent: &Arc<Session>, kid: RemoteKid) {
        {
            let mut remote = crate::lock(&self.family.remote);
            if remote.contains(&kid) {
                return;
            }
            remote.push(kid.clone());
        }
        let Some(mesh) = self.mesh() else {
            return;
        };
        let mut live = parent.live.clone();
        let task = tokio::spawn(async move {
            if parent_gone(&mut live).await {
                let caller = kid.parent.clone();
                if let Err(e) = children::stop_remote(&mesh, &caller, &kid, false).await {
                    log::warn!("child {} on {} was not stopped: {e}", kid.id, kid.name);
                }
            }
        });
        parent.keep(task);
    }

    /// The children of `parent` that run on other machines.
    pub(super) fn remote_kids(&self, parent: &str) -> Vec<RemoteKid> {
        crate::lock(&self.family.remote)
            .iter()
            .filter(|kid| kid.parent == parent)
            .cloned()
            .collect()
    }

    /// Notes that a child ended or was stopped: it stays in the parent's
    /// list to read, but frees its place.
    pub(super) fn end_remote(&self, kid: &RemoteKid) {
        for known in crate::lock(&self.family.remote).iter_mut() {
            if known.parent == kid.parent && known.machine == kid.machine && known.id == kid.id {
                known.ended = true;
            }
        }
    }

    /// Stops the running turns of everything `session` started, here and on
    /// other machines, to go with the parent's own Stop.
    pub(super) async fn cancel_family(&self, session: &Arc<Session>) {
        let mut queue = vec![session.clone()];
        let mut visited = 0;
        while let Some(next) = queue.pop() {
            visited += 1;
            // The tree is bounded by the limits; this guards a loop in links.
            if visited > MAX_CHILDREN * usize::from(MAX_DEPTH) + 1 {
                break;
            }
            let who = self.who_of(&next);
            queue.extend(self.children_of(&who));
            if let Some(mesh) = self.mesh() {
                for kid in self.remote_kids(&who.session) {
                    if let Err(e) = children::stop_remote(&mesh, &who.session, &kid, true).await {
                        log::warn!("child {} on {} was not stopped: {e}", kid.id, kid.name);
                    }
                }
            }
            if !Arc::ptr_eq(&next, session) {
                // Refused when the child has no turn running.
                let _ = next.order(Command::Cancel).await;
            }
        }
    }

    /// The links the open sessions keep over a restart: the remote children
    /// of the session `name`.
    pub(super) fn kids_to_keep(&self, name: &str) -> Vec<RemoteKid> {
        self.remote_kids(name)
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

/// The mode a parent is held to: when it runs below full access, the id of
/// the mode it is in. A harness that does not name a full access mode has
/// none held to.
fn held_to(info: &SessionInfo) -> Option<String> {
    let full = known(&info.agent)?.bypass_mode?;
    info.mode.clone().filter(|mode| mode != full)
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
