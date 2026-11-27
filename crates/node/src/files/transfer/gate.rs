//! The blobs protocol's gate: each connection and request is asked of sync
//! (a removed device is out, a peer of a synced project may fetch its files)
//! and then of the grants of the open offers, and the rest is refused.

use crate::files::{grants::Grants, sync::Syncs};
use iroh::EndpointId;
use iroh_blobs::{
    protocol::GetRequest,
    provider::events::{
        AbortReason, ConnectMode, EventMask, EventSender, ObserveMode, ProviderMessage,
        RequestMode, RequestUpdate, ThrottleMode,
    },
};
use irpc::channel::mpsc::Receiver;
use std::sync::Arc;
use tokio::sync::watch;
use tokio_util::task::AbortOnDropHandle;

/// How many provider events wait for the handler.
const EVENT_QUEUE: usize = 32;

/// Lets `peer` in on `connection` unless sync says it was removed, as a peer
/// of a synced project or with a grant.
fn connect(
    grants: &Grants,
    syncs: &Syncs,
    connection: u64,
    peer: Option<EndpointId>,
) -> Result<(), AbortReason> {
    if peer.is_some_and(|peer| !syncs.allows(&peer)) {
        return Err(AbortReason::Permission);
    }

    let synced = peer.is_some_and(|peer| syncs.may_connect(&peer));

    grants.connect(connection, peer, synced)
}

/// Whether the peer on `connection` may make `request`: refused when sync
/// says it was removed since it connected.
fn get(
    grants: &Grants,
    syncs: &Syncs,
    connection: u64,
    request: &GetRequest,
) -> Result<Option<watch::Sender<u64>>, AbortReason> {
    let peer = grants.peer(connection);

    if peer.is_some_and(|peer| !syncs.allows(&peer)) {
        return Err(AbortReason::Permission);
    }

    let synced = request.ranges.is_blob()
        && peer.is_some_and(|peer| syncs.may_get(&peer, &request.hash));

    grants.get(connection, request, synced)
}

/// The events the blobs protocol sends, answered from `grants` and what
/// `syncs` says of the peer by a task
/// the handle owns. Requests that could reach other blobs (`get_many`,
/// `observe`) and writes (`push`) are refused.
pub(super) fn events(grants: Arc<Grants>, syncs: Syncs) -> (EventSender, AbortOnDropHandle<()>) {
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        get: RequestMode::InterceptLog,
        get_many: RequestMode::Disabled,
        push: RequestMode::Disabled,
        observe: ObserveMode::Intercept,
        throttle: ThrottleMode::None,
    };
    let (sender, mut events) = EventSender::channel(EVENT_QUEUE, mask);
    let task = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            // A failed answer means the request is gone already.
            match event {
                ProviderMessage::ClientConnected(msg) => {
                    let answer = connect(&grants, &syncs, msg.connection_id, msg.endpoint_id);

                    if answer.is_err() {
                        log::info!("refused a blobs connection from {:?}", msg.endpoint_id);
                    }

                    msg.tx.send(answer).await.ok();
                }
                ProviderMessage::ConnectionClosed(msg) => grants.closed(msg.connection_id),
                ProviderMessage::GetRequestReceived(msg) => {
                    let answer = get(&grants, &syncs, msg.connection_id, &msg.request);
                    let sent = answer.as_ref().ok().cloned().flatten();
                    msg.tx.send(answer.map(drop)).await.ok();
                    // The fetch waits on its updates being read, so they
                    // are read to the end even when nobody counts them.
                    tokio::spawn(count(msg.rx, sent));
                }
                ProviderMessage::ObserveRequestReceived(msg) => {
                    msg.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                ProviderMessage::GetManyRequestReceived(msg) => {
                    msg.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                ProviderMessage::PushRequestReceived(msg) => {
                    msg.tx.send(Err(AbortReason::Permission)).await.ok();
                }
                // Notifications of the modes left at none never come.
                _ => {}
            }
        }
    });
    (sender, AbortOnDropHandle::new(task))
}

/// Reads one fetch's updates to their end, counting the bytes it sent of
/// the blobs it started into `sent`, which keeps the most any fetch sent.
async fn count(mut updates: Receiver<RequestUpdate>, sent: Option<watch::Sender<u64>>) {
    // The blobs this fetch finished, and the one it sends now.
    let (mut finished, mut size, mut offset) = (0u64, 0u64, 0u64);

    while let Ok(Some(update)) = updates.recv().await {
        match update {
            RequestUpdate::Started(started) => {
                finished = finished.saturating_add(size);
                (size, offset) = (started.size, 0);
            }
            RequestUpdate::Progress(progress) => offset = progress.end_offset,
            RequestUpdate::Completed(_) | RequestUpdate::Aborted(_) => {}
        }

        if let Some(sent) = &sent {
            let now = finished.saturating_add(offset.min(size));
            sent.send_if_modified(|most| {
                let more = now > *most;
                *most = (*most).max(now);
                more
            });
        }
    }
}

#[cfg(test)]
#[path = "../../tests/files_transfer_gate.rs"]
mod tests;
