//! What the family tools say to other machines over `tools/1`: one call and
//! how it went, stopping a child (again, if its machine does not answer),
//! counting a root's agents, asking about a parent, and reading a child's
//! state without making a transcript read wait for it.

use super::links::{ChildNote, NO_AGENT, Parent, RemoteKid, Views};
use crate::agents::sessions;
use crate::agents::{
    Agents,
    child_view::ChildView,
    machine_list::{Machine, machine_name},
    reply::json_of,
};
use crate::peer_stream::over_the_mesh;
use beep_mesh::Mesh;
use beep_proto::SessionId;
use beep_proto::node::tools::{Reply, Request, Tool};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{borrow::Cow, collections::BTreeSet, future::Future, time::Duration};
use tokio::time::{Instant, sleep, timeout};
use tokio_util::task::AbortOnDropHandle;

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
    pub(crate) fn id(id: SessionId) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub(crate) fn message(id: SessionId, text: String) -> Self {
        Self {
            id,
            text: Some(text),
            ..Self::default()
        }
    }
}

/// How long a call to a child on another machine may take.
pub(crate) const REMOTE_LIMIT: Duration = Duration::from_secs(20);
/// How many times a stop is sent, and the pause before the next one grows
/// by, when the machine does not answer.
const STOP_TRIES: u32 = 4;
const STOP_PAUSE: Duration = Duration::from_secs(5);
/// How long a child's state read for a transcript is shown before it is read
/// again.
const VIEW_FRESH: Duration = Duration::from_secs(5);

/// What a parent's node answers a child that asks whether it is kept.
pub(crate) const KEPT: &str = "kept";
pub(crate) const GONE: &str = "gone";

/// How a call to another machine went.
pub(crate) enum Answer {
    /// It answered and did what was asked.
    Text(String),
    /// It answered and did not: nothing was done there.
    Refused(String),
    /// No answer: it may or may not have done it.
    Silent(String),
}

/// A call to the node of `machine`, which `name` is to agents.
pub(crate) struct Call<'a> {
    mesh: &'a Mesh,
    /// The calling session's id as the machine knows it.
    session: &'a SessionId,
    machine: EndpointId,
    name: Cow<'a, str>,
}

impl<'a> Call<'a> {
    /// A call from `session` to the machine of `kid`.
    pub(crate) fn to_kid(mesh: &'a Mesh, session: &'a SessionId, kid: &'a RemoteKid) -> Self {
        Self {
            mesh,
            session,
            machine: kid.machine,
            name: Cow::Borrowed(&kid.name),
        }
    }

    /// A call from `session` to `machine`.
    pub(crate) fn to_machine(mesh: &'a Mesh, session: &'a SessionId, machine: &'a Machine) -> Self {
        Self {
            mesh,
            session,
            machine: machine.id,
            name: Cow::Borrowed(&machine.name),
        }
    }

    fn to_peer(mesh: &'a Mesh, session: &'a SessionId, peer: EndpointId) -> Self {
        Self {
            mesh,
            session,
            machine: peer,
            name: Cow::Owned(peer.fmt_short().to_string()),
        }
    }

    pub(crate) async fn ask<T: Serialize>(&self, tool: Tool, args: &T, limit: Duration) -> Answer {
        let request = Request {
            session: self.session.clone(),
            tool,
            args: serde_json::to_string(args).unwrap_or_default(),
            approved: Default::default(),
        };
        let name = &self.name;
        let sent = match timeout(limit, over_the_mesh(self.mesh, self.machine, &request)).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(e)) => return Answer::Silent(format!("Could not reach {name}: {e:#}")),
            Err(_) => return Answer::Silent(format!("{name} did not answer in time.")),
        };

        match sent {
            Reply::Done {
                text,
                failed: false,
            } => Answer::Text(text),
            Reply::Done { text, .. } => Answer::Refused(text),
            other => Answer::Refused(format!("{name} answered {other:?}.")),
        }
    }

    pub(crate) async fn ask_ok<T: Serialize>(
        &self,
        tool: Tool,
        args: &T,
        limit: Duration,
    ) -> Result<String, String> {
        match self.ask(tool, args, limit).await {
            Answer::Text(text) => Ok(text),
            Answer::Refused(why) | Answer::Silent(why) => Err(why),
        }
    }
}

pub(crate) fn parse_view(name: &str, text: &str) -> Result<ChildView, String> {
    serde_json::from_str(text).map_err(|e| format!("{name} answered something unreadable: {e}"))
}

/// Stops a child on another machine: its session, or, for a parent's Stop
/// all, its running turn, background commands and subagents.
/// A child the machine does not have is as good as stopped.
pub(crate) async fn stop_remote(
    mesh: &Mesh,
    parent: &SessionId,
    kid: &RemoteKid,
    turn_only: bool,
) -> Result<(), String> {
    let args = PeerCall {
        id: kid.id.clone(),
        turn_only,
        // Only a parent's Stop all stops a child's turn alone.
        stop_all: turn_only,
        ..Default::default()
    };

    match Call::to_kid(mesh, parent, kid)
        .ask(Tool::StopAgent, &args, REMOTE_LIMIT)
        .await
    {
        Answer::Text(_) => Ok(()),
        Answer::Refused(why) if !turn_only && why.starts_with(NO_AGENT) => Ok(()),
        Answer::Refused(why) | Answer::Silent(why) => Err(why),
    }
}

/// Stops a child's session, again a few times when its machine does not
/// answer.
pub(crate) async fn stop_remote_retrying(
    mesh: &Mesh,
    parent: &SessionId,
    kid: &RemoteKid,
) -> Result<(), String> {
    retrying(STOP_TRIES, STOP_PAUSE, || {
        stop_remote(mesh, parent, kid, false)
    })
    .await
}

/// Runs `once` up to `tries` times, `pause` longer before each, until it
/// works; the last failure when none does.
pub(crate) async fn retrying<F, Fut>(tries: u32, pause: Duration, mut once: F) -> Result<(), String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let mut last = Err("not tried".to_owned());

    for attempt in 0..tries {
        if attempt > 0 {
            sleep(pause * attempt).await;
        }
        last = once().await;

        if last.is_ok() {
            break;
        }
    }

    last
}

/// What a node is asked to count: the live agents under a root session on
/// the machine that runs it.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CountCall {
    pub(crate) root_machine: Option<EndpointId>,
    pub(crate) root: SessionId,
}

/// How many live agents `machine` runs under the root `root`; none when it
/// does not say in time.
pub(crate) async fn count_on(
    mesh: &Mesh,
    machine: EndpointId,
    root_machine: Option<EndpointId>,
    root: &SessionId,
    limit: Duration,
) -> usize {
    let call = Call::to_peer(mesh, root, machine);
    let args = CountCall {
        root_machine,
        root: root.clone(),
    };

    match call.ask_ok(Tool::AgentCount, &args, limit).await {
        Ok(text) => text.trim().parse().unwrap_or(0),
        Err(why) => {
            log::debug!("{} did not count the agents under {root}: {why}", call.name);
            0
        }
    }
}

/// Asks `peer` whether its session `parent` still keeps the child that goes
/// by `id` (or `key`) here.
pub(crate) async fn ask_parent(
    mesh: &Mesh,
    peer: EndpointId,
    parent: &SessionId,
    id: &SessionId,
    key: &SessionId,
) -> Parent {
    let args = PeerCall::message(id.clone(), key.to_string());

    match Call::to_peer(mesh, parent, peer)
        .ask(Tool::AgentParent, &args, REMOTE_LIMIT)
        .await
    {
        Answer::Text(text) if text == GONE => Parent::Gone,
        // Anything else that answers is not a no: a node that does not know
        // the question is too old to ask it of.
        Answer::Text(_) | Answer::Refused(_) => Parent::Keeps,
        Answer::Silent(_) => Parent::Silent,
    }
}

/// Why a note did not reach the parent's node.
pub(crate) enum NoteFailed {
    /// The node answered and did not take it: asking again says the same.
    Refused(String),
    /// No answer: the link is down or the node is off.
    Silent(String),
}

/// Tells `peer`, the machine of the session `parent`, that its child here
/// finished a turn or asks the user.
pub(crate) async fn note_parent(
    mesh: &Mesh,
    peer: EndpointId,
    parent: &SessionId,
    note: &ChildNote,
) -> Result<(), NoteFailed> {
    match Call::to_peer(mesh, parent, peer)
        .ask(Tool::AgentNote, note, REMOTE_LIMIT)
        .await
    {
        Answer::Text(_) => Ok(()),
        Answer::Refused(why) => Err(NoteFailed::Refused(why)),
        Answer::Silent(why) => Err(NoteFailed::Silent(why)),
    }
}

/// How a child on another machine stands, asked of it now.
pub(crate) async fn fetch_view(
    mesh: &Mesh,
    parent: &SessionId,
    kid: &RemoteKid,
) -> Result<ChildView, String> {
    let text = Call::to_kid(mesh, parent, kid)
        .ask_ok(
            Tool::AgentStatus,
            &PeerCall::id(kid.id.clone()),
            REMOTE_LIMIT,
        )
        .await?;

    parse_view(&kid.name, &text)
}

impl Views {
    /// The last state read of `kid`, `parent`'s child; none before the first
    /// read comes back.
    pub(crate) fn of(&self, mesh: &Mesh, parent: &SessionId, kid: &RemoteKid) -> Option<ChildView> {
        let key = (kid.machine, kid.id.clone());
        let mut seen = crate::lock(&self.0);
        let entry = seen.entry(key.clone()).or_default();
        let stale = entry.at.is_none_or(|at| at.elapsed() >= VIEW_FRESH);
        let reading = entry
            .reading
            .as_ref()
            .is_some_and(|task| !task.is_finished());

        if stale && !reading {
            let (views, mesh) = (self.clone(), mesh.clone());
            let (parent, kid) = (parent.to_owned(), kid.clone());
            entry.reading = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                let view = fetch_view(&mesh, &parent, &kid).await;

                if let Some(entry) = crate::lock(&views.0).get_mut(&key) {
                    entry.at = Some(Instant::now());

                    match view {
                        Ok(view) => entry.view = Some(view),
                        Err(e) => log::debug!("child {} on {}: {e}", kid.id, kid.name),
                    }
                }
            })));
        }

        entry.view.clone()
    }
}

/// How long a machine has to list its agents.
const LIST_LIMIT: Duration = Duration::from_secs(5);
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

pub(crate) fn listed_here(agents: &Agents, ended_since: Option<u64>) -> Vec<Listed> {
    let mesh = agents.links.mesh();

    agents
        .sessions
        .peers(ended_since)
        .iter()
        .map(|session| {
            let info = session.info();
            let mut view = ChildView::of(&info, &sessions::name_of(session));
            view.project = mesh
                .as_ref()
                .and_then(|mesh| crate::agents::projects::of_folder(mesh, &info.cwd, mesh.me()));
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
        .and_then(|mesh| {
            mesh.code_hosts()
                .into_iter()
                .find(|status| status.device.id == mesh.me())
        })
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
