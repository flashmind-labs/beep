//! The sessions of the team's other machines, as each of them pushes its
//! `agents` list to the acting machine: a follow per machine, so the cards are
//! made again when a session changes and not on a timer.

use crate::peer_stream::{PeerWatch, Refused};
use anyhow::Result;
use beep_mesh::Mesh;
use beep_proto::node::{
    agent::{SessionInfo, SessionsReply},
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

        // A machine whose stream dropped keeps its last rows until the next list.
        if refused {
            let refused = Sighted {
                peer,
                sight: Sight::Refused,
            };

            sighted.send(refused).await.ok();

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
