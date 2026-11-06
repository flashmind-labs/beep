//! Logs each notice for the events streams, and reaches the phones that did
//! not take it off theirs through FCM or APNs.

use crate::{app::UserNotice, protocol::DeviceKind, push::remote::Fallback, store::Store};
use iroh::EndpointId;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::broadcast::{self, error::RecvError},
    task::{JoinHandle, JoinSet},
    time::sleep,
};

/// How long a phone has to take a notice off its stream before it is pushed.
pub const ACK_WAIT: Duration = Duration::from_secs(5);
/// Notices waiting on their acks at once; the next waits for a slot.
const MAX_WAITING: usize = 64;

/// Logs every notice sent on `notices` until the channel closes. With a
/// `fallback`, a phone that has not acked one after `ack_wait` gets it
/// pushed. The returned task owns the waits.
pub fn start(
    store: Store,
    mut notices: broadcast::Receiver<UserNotice>,
    fallback: Option<Arc<dyn Fallback>>,
    ack_wait: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut waiting = JoinSet::new();
        loop {
            let notice = match notices.recv().await {
                Ok(notice) => notice,
                Err(RecvError::Lagged(missed)) => {
                    log::warn!("push: missed {missed} notices");
                    continue;
                }
                Err(RecvError::Closed) => break,
            };
            let id = match store.log_notice(&notice).await {
                Ok(id) => id,
                Err(e) => {
                    log::warn!("push: logging a notice failed; user={} error={e:#}", notice.user);
                    continue;
                }
            };
            let Some(fallback) = fallback.clone() else {
                continue;
            };
            while waiting.len() >= MAX_WAITING {
                waiting.join_next().await;
            }
            // Finished waits are reaped as new ones come.
            while waiting.try_join_next().is_some() {}
            waiting.spawn(push_unacked(store.clone(), fallback, id, notice, ack_wait));
        }
        while waiting.join_next().await.is_some() {}
    })
}

/// Pushes notice `id` to each of the user's phones that has not acked it.
async fn push_unacked(
    store: Store,
    fallback: Arc<dyn Fallback>,
    id: i64,
    notice: UserNotice,
    ack_wait: Duration,
) {
    sleep(ack_wait).await;
    let user = &notice.user;
    let (keys, acked) = match tokio::try_join!(store.live_keys(user), store.notice_acks(id)) {
        Ok(found) => found,
        Err(e) => {
            log::warn!("push: reading keys failed; user={user} error={e:#}");
            return;
        }
    };
    for key in keys {
        if key.kind != (DeviceKind::Phone {}) || acked.contains(&key.endpoint_id) {
            continue;
        }
        let Ok(phone) = key.endpoint_id.parse::<EndpointId>() else {
            continue;
        };
        let Some(sent) = notice.for_device(&phone, &key.kind) else {
            continue;
        };
        if !fallback.push(phone, sent).await {
            log::debug!("push: no push service took a notice; user={user}");
        }
    }
}
