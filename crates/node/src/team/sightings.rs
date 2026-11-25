//! The sessions of the team's other machines, as each of them pushes its
//! `agents` list to the acting machine: a follow per machine, so the cards are
//! made again when a session changes and not on a timer.

use crate::{
    agents::mcp::{
        Refused, command_name,
        children::{ChildView, Listed},
        projects,
    },
    link::peer_watch::PeerWatch,
};
use anyhow::Result;
use beep_mesh::Mesh;
use beep_proto::node::{
    agent::{SessionInfo, SessionsReply, Status},
    events::{Topic, Value},
};
use iroh::EndpointId;
use std::time::Duration;
use tokio::{sync::mpsc, time::sleep};

/// How long a follow waits before it follows a machine that dropped again.
const REDIAL: Duration = Duration::from_secs(5);

/// What a follow heard of one machine.
pub(super) struct Sighted {
    pub(super) peer: EndpointId,
    pub(super) sight: Sight,
}

pub(super) enum Sight {
    /// The machine's rows of its sessions, whole.
    Rows(Vec<SessionInfo>),
    /// The stream dropped; the follow dials again.
    Gone,
    /// The machine does not serve its list to a node, as one from before it:
    /// it is asked for its sessions instead.
    Refused,
}

/// Follows `peer`'s `agents` list until the gateway is gone.
pub(super) async fn follow(mesh: Mesh, peer: EndpointId, sighted: mpsc::Sender<Sighted>) {
    loop {
        let ended = heard(&mesh, peer, &sighted).await;
        let refused = matches!(&ended, Err(e) if e.downcast_ref::<Refused>().is_some());

        if let Err(e) = &ended {
            log::debug!("the sessions of {peer} are not followed: {e:#}");
        }

        let sight = if refused { Sight::Refused } else { Sight::Gone };

        if sighted.send(Sighted { peer, sight }).await.is_err() || refused {
            return;
        }

        sleep(REDIAL).await;
    }
}

/// Passes on each list `peer` pushes, until the stream ends.
async fn heard(mesh: &Mesh, peer: EndpointId, sighted: &mpsc::Sender<Sighted>) -> Result<()> {
    let mut watch = PeerWatch::open(mesh, peer, Topic::new("agents")).await?;

    loop {
        if let Value::Sessions(reply) = watch.next().await?
            && let SessionsReply::Sessions { sessions } = *reply
        {
            let rows = Sighted {
                peer,
                sight: Sight::Rows(sessions),
            };

            if sighted.send(rows).await.is_err() {
                return Ok(());
            }
        }
    }
}

/// `rows`, the sessions `peer` pushed, as the agents list names them: those
/// that run, and those that ended since `ended_since`, Unix seconds.
pub(super) fn listed(
    mesh: &Mesh,
    peer: EndpointId,
    rows: &[SessionInfo],
    ended_since: u64,
) -> Vec<Listed> {
    let Some(status) = mesh
        .code_hosts()
        .into_iter()
        .find(|status| status.device.id == peer)
    else {
        return Vec::new();
    };
    let name = command_name(&status.device.name);

    rows.iter()
        .filter(|info| !info.reviewer)
        .filter(|info| info.state.status != Status::Ended || info.last_activity >= ended_since)
        .map(|info| {
            let mut view = ChildView::of(info, &info.id).named(&name);
            view.project = projects::of_folder(mesh, &info.cwd, peer);

            Listed {
                view,
                machine: status.device.name.clone(),
                folder: None,
            }
        })
        .collect()
}
