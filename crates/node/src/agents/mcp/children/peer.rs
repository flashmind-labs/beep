//! What the family tools say to other machines over `tools/1`: one call and
//! how it went, stopping a child (again, if its machine does not answer),
//! counting a root's agents, asking about a parent, and reading a child's
//! state without making a transcript read wait for it.

use super::{ChildView, PeerCall};
use crate::agents::{
    family::{NO_AGENT, Parent, RemoteKid},
    mcp::machines::over_the_mesh,
};
use beep_mesh::Mesh;
use beep_proto::node::tools::{Reply, Request, Tool};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::{Instant, sleep, timeout};
use tokio_util::task::AbortOnDropHandle;

/// How long a call to a child on another machine may take.
pub(super) const REMOTE_LIMIT: Duration = Duration::from_secs(20);
/// How many times a stop is sent, and the pause before the next one grows
/// by, when the machine does not answer.
const STOP_TRIES: u32 = 4;
const STOP_PAUSE: Duration = Duration::from_secs(5);
/// How long a child's state read for a transcript is shown before it is read
/// again.
const VIEW_FRESH: Duration = Duration::from_secs(5);

/// What a parent's node answers a child that asks whether it is kept.
pub(super) const KEPT: &str = "kept";
pub(super) const GONE: &str = "gone";

/// How a call to another machine went.
pub(super) enum Answer {
    /// It answered and did what was asked.
    Text(String),
    /// It answered and did not: nothing was done there.
    Refused(String),
    /// No answer: it may or may not have done it.
    Silent(String),
}

/// A call to the node of `machine`, which `name` is to agents.
pub(super) struct Call<'a> {
    pub(super) mesh: &'a Mesh,
    /// The calling session's id as the machine knows it.
    pub(super) session: &'a str,
    pub(super) machine: EndpointId,
    pub(super) name: &'a str,
}

impl Call<'_> {
    pub(super) async fn ask(&self, tool: Tool, args: &Value, limit: Duration) -> Answer {
        let request = Request {
            session: self.session.to_owned(),
            tool,
            args: args.to_string(),
            approved: Default::default(),
        };
        let name = self.name;
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
}

/// Stops a child on another machine: its session, or only its running turn.
/// A child the machine does not have is as good as stopped.
pub(crate) async fn stop_remote(
    mesh: &Mesh,
    parent: &str,
    kid: &RemoteKid,
    turn_only: bool,
) -> Result<(), String> {
    let call = Call {
        mesh,
        session: parent,
        machine: kid.machine,
        name: &kid.name,
    };
    let args = json!(PeerCall {
        id: kid.id.clone(),
        text: None,
        turn_only
    });
    match call.ask(Tool::StopAgent, &args, REMOTE_LIMIT).await {
        Answer::Text(_) => Ok(()),
        Answer::Refused(why) if !turn_only && why.starts_with(NO_AGENT) => Ok(()),
        Answer::Refused(why) | Answer::Silent(why) => Err(why),
    }
}

/// Stops a child's session, again a few times when its machine does not
/// answer.
pub(crate) async fn stop_remote_retrying(
    mesh: &Mesh,
    parent: &str,
    kid: &RemoteKid,
) -> Result<(), String> {
    retrying(STOP_TRIES, STOP_PAUSE, || {
        stop_remote(mesh, parent, kid, false)
    })
    .await
}

/// Runs `once` up to `tries` times, `pause` longer before each, until it
/// works; the last failure when none does.
pub(super) async fn retrying<F, Fut>(tries: u32, pause: Duration, mut once: F) -> Result<(), String>
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
    pub(crate) root: String,
}

/// How many live agents `machine` runs under the root `root`; none when it
/// does not say in time.
pub(crate) async fn count_on(
    mesh: &Mesh,
    machine: EndpointId,
    root_machine: Option<EndpointId>,
    root: &str,
    limit: Duration,
) -> usize {
    let name = machine.fmt_short().to_string();
    let call = Call {
        mesh,
        session: root,
        machine,
        name: &name,
    };
    let args = json!(CountCall {
        root_machine,
        root: root.to_owned()
    });
    match call.ask(Tool::AgentCount, &args, limit).await {
        Answer::Text(text) => text.trim().parse().unwrap_or(0),
        Answer::Refused(why) | Answer::Silent(why) => {
            log::debug!("{name} did not count the agents under {root}: {why}");
            0
        }
    }
}

/// Asks `peer` whether its session `parent` still keeps the child that goes
/// by `id` (or `key`) here.
pub(crate) async fn ask_parent(
    mesh: &Mesh,
    peer: EndpointId,
    parent: &str,
    id: &str,
    key: &str,
) -> Parent {
    let name = peer.fmt_short().to_string();
    let call = Call {
        mesh,
        session: parent,
        machine: peer,
        name: &name,
    };
    let args = json!(PeerCall {
        id: id.to_owned(),
        text: Some(key.to_owned()),
        turn_only: false
    });
    match call.ask(Tool::AgentParent, &args, REMOTE_LIMIT).await {
        Answer::Text(text) if text == GONE => Parent::Gone,
        // Anything else that answers is not a no: a node that does not know
        // the question is too old to ask it of.
        Answer::Text(_) | Answer::Refused(_) => Parent::Keeps,
        Answer::Silent(_) => Parent::Silent,
    }
}

/// How a child on another machine stands, asked of it now.
pub(super) async fn fetch_view(
    mesh: &Mesh,
    parent: &str,
    kid: &RemoteKid,
) -> Result<ChildView, String> {
    let call = Call {
        mesh,
        session: parent,
        machine: kid.machine,
        name: &kid.name,
    };
    let args = json!(PeerCall {
        id: kid.id.clone(),
        text: None,
        turn_only: false
    });
    match call.ask(Tool::AgentStatus, &args, REMOTE_LIMIT).await {
        Answer::Text(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} answered something unreadable: {e}", kid.name)),
        Answer::Refused(why) | Answer::Silent(why) => Err(why),
    }
}

/// The last state read of each child on another machine, for transcripts: a
/// read of a transcript shows what is known and never waits on a machine,
/// which may be off. A state older than `VIEW_FRESH` is read again in the
/// background, for the next read.
#[derive(Clone, Default)]
pub(crate) struct Views(Arc<Mutex<HashMap<(EndpointId, String), Seen>>>);

#[derive(Default)]
struct Seen {
    view: Option<ChildView>,
    /// When the last read ended, well or not.
    at: Option<Instant>,
    reading: Option<AbortOnDropHandle<()>>,
}

impl Views {
    /// The last state read of `kid`, `parent`'s child; none before the first
    /// read comes back.
    pub(super) fn of(&self, mesh: &Mesh, parent: &str, kid: &RemoteKid) -> Option<ChildView> {
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
