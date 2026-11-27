//! Presence snapshots over the mesh's agents protocol.

use crate::agents::Agents;
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{ExternalSession, SessionsReply},
};
use beep_proto::watched;
use tokio::sync::broadcast::error::RecvError;

/// Sends the known presence first, then after every open, update, close or
/// ownership change.
pub(in crate::agents) async fn follow(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: &Agents,
) -> Result<()> {
    let mut changes = agents.presence.subscribe();
    let mut events = agents.events.subscribe();
    let mut sent: Option<Vec<ExternalSession>> = None;

    loop {
        watched::read_and_update(&mut changes, |_| ());
        let owned = agents.owned();
        let starting = agents.starting_pids();
        let mut sessions = agents.presence.snapshot();

        for session in &mut sessions {
            session.taken_over = owned.get(&session.id).cloned().or_else(|| {
                session
                    .pid
                    .filter(|pid| starting.contains(pid))
                    .map(|_| session.id.clone())
            });
        }

        if sent.as_ref() != Some(&sessions) {
            tx.send(&SessionsReply::External {
                sessions: sessions.clone(),
            })
            .await?;
            sent = Some(sessions);
        }

        loop {
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Ok(());
                    }
                    break;
                }
                event = events.recv() => match event {
                    Ok(change) if change.topic.name() == "agents" => break,
                    Ok(_) => continue,
                    Err(RecvError::Lagged(_)) => break,
                    Err(RecvError::Closed) => return Ok(()),
                },
                _ = rx.recv::<serde_json::Value>() => return Ok(()),
            }
        }
    }
}
