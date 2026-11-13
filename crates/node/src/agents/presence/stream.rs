//! Presence snapshots over the mesh's agents protocol.

use crate::agents::Agents;
use anyhow::Result;
use beep_proto::node::{LinkRx, LinkTx, agent::SessionsReply};
use beep_proto::watched;

/// Sends the known presence first, then after every open, update or close.
pub(in crate::agents) async fn follow(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: &Agents,
) -> Result<()> {
    let mut changes = agents.presence.subscribe();

    loop {
        watched::read_and_update(&mut changes, |_| ());
        let owned = agents.owned();
        let mut sessions = agents.presence.snapshot();

        for session in &mut sessions {
            session.taken_over = owned.get(&session.id).cloned();
        }

        tx.send(&SessionsReply::External { sessions }).await?;
        tokio::select! {
            changed = changes.changed() => if changed.is_err() { return Ok(()); },
            _ = rx.recv::<serde_json::Value>() => return Ok(()),
        }
    }
}
