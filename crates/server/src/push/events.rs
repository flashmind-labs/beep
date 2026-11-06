//! The apps' events stream: what a device is told, as `AppEvent` frames.

use crate::{
    account::user::User,
    api::router::frames,
    protocol::{AppEvent, DeviceKind, http},
    push::hub::{Heard, Hub},
    store::Store,
};
use bytes::Bytes;
use futures::stream::BoxStream;
use iroh::EndpointId;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, broadcast::error::RecvError, mpsc},
    time::interval,
};
use tokio_util::task::AbortOnDropHandle;

/// When every open stream sends a heartbeat. Clones share the beats; dropping
/// the last stops them.
#[derive(Clone)]
pub struct Pulse {
    beats: broadcast::Sender<()>,
    /// Held for its drop, which stops the beats.
    _task: Option<Arc<AbortOnDropHandle<()>>>,
}

impl Pulse {
    /// A beat every `period`.
    pub fn every(period: Duration) -> Self {
        let (beats, _) = broadcast::channel(1);
        let sender = beats.clone();
        let task = tokio::spawn(async move {
            let mut ticks = interval(period);
            // The first tick is at once; the streams just opened need none.
            ticks.tick().await;
            loop {
                ticks.tick().await;
                // No stream open is no error.
                let _ = sender.send(());
            }
        });
        Self {
            beats,
            _task: Some(Arc::new(AbortOnDropHandle::new(task))),
        }
    }

    /// Beats only on `beat`, for tests.
    pub fn manual() -> Self {
        let (beats, _) = broadcast::channel(1);
        Self { beats, _task: None }
    }

    /// Sends every open stream a heartbeat now.
    pub fn beat(&self) {
        // No stream open is no error.
        let _ = self.beats.send(());
    }
}

/// Serves events streams on this instance.
pub struct Events {
    store: Store,
    hub: Hub,
    pulse: Pulse,
}

/// The device a stream is for.
pub struct Device {
    pub user: Arc<User>,
    pub key: EndpointId,
    pub kind: DeviceKind,
}

/// Why a stream stopped early.
enum Stop {
    /// The client went away.
    Gone,
    Failed(anyhow::Error),
}

type Sent = Result<(), Stop>;

impl Events {
    pub fn new(store: Store, hub: Hub, pulse: Pulse) -> Self {
        Self { store, hub, pulse }
    }

    /// `device`'s stream, after notice `after` when given, else from now.
    pub fn open(&self, device: Device, after: Option<i64>) -> BoxStream<'static, Bytes> {
        // Subscribed before anything is read, so nothing falls between.
        let heard = self.hub.subscribe();
        let beats = self.pulse.beats.subscribe();
        let store = self.store.clone();
        frames(move |tx| async move {
            let key = device.key;
            match follow(store, device, after, heard, beats, &tx).await {
                Ok(()) | Err(Stop::Gone) => {}
                Err(Stop::Failed(e)) => log::warn!("events: stream ended; key={key} error={e:#}"),
            }
        })
    }
}

async fn follow(
    store: Store,
    device: Device,
    after: Option<i64>,
    mut heard: broadcast::Receiver<Heard>,
    mut beats: broadcast::Receiver<()>,
    tx: &mpsc::Sender<Bytes>,
) -> Sent {
    let user = device.user.id().to_owned();
    let mut updates = device.user.watch_updates().await.map_err(Stop::Failed)?;
    let mut cursor = match after {
        Some(after) => after,
        None => store.newest_notice(&user).await.map_err(Stop::Failed)?,
    };
    send(tx, &AppEvent::Resume { after: cursor }).await?;
    let last = *updates.borrow_and_update();
    send(tx, &AppEvent::Updated { last }).await?;
    catch_up(&store, &device, &mut cursor, tx).await?;
    loop {
        tokio::select! {
            event = heard.recv() => match event {
                Ok(Heard::Notice { user: to, id }) if to == user && id > cursor => {
                    catch_up(&store, &device, &mut cursor, tx).await?;
                }
                Ok(Heard::End { key }) if key == device.key => return Ok(()),
                Ok(Heard::Resync) | Err(RecvError::Lagged(_)) => {
                    catch_up(&store, &device, &mut cursor, tx).await?;
                }
                Ok(_) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
            changed = updates.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let last = *updates.borrow_and_update();
                send(tx, &AppEvent::Updated { last }).await?;
            }
            beat = beats.recv() => match beat {
                Ok(()) | Err(RecvError::Lagged(_)) => send(tx, &AppEvent::Heartbeat {}).await?,
                Err(RecvError::Closed) => return Ok(()),
            },
        }
    }
}

/// Sends the notices after `cursor` that are for this device.
async fn catch_up(
    store: &Store,
    device: &Device,
    cursor: &mut i64,
    tx: &mpsc::Sender<Bytes>,
) -> Sent {
    loop {
        let page = store
            .notices_after(device.user.id(), *cursor)
            .await
            .map_err(Stop::Failed)?;
        let Some(last) = page.last().map(|logged| logged.id) else {
            return Ok(());
        };
        for logged in page {
            if let Some(notice) = logged.notice.for_device(&device.key, &device.kind) {
                let event = AppEvent::Notice {
                    id: logged.id,
                    notice: Box::new(notice.clone()),
                };
                send(tx, &event).await?;
            }
        }
        *cursor = last;
    }
}

async fn send(tx: &mpsc::Sender<Bytes>, event: &AppEvent) -> Sent {
    let body = http::encode(event).map_err(|e| Stop::Failed(e.into()))?;
    tx.send(http::length_prefixed(&body))
        .await
        .map_err(|_| Stop::Gone)
}
