//! Hears what every instance tells `EVENTS_CHANNEL` and hands it to the
//! events streams on this one.

use crate::store::{Bumped, EVENTS_CHANNEL, Store};
use anyhow::Result;
use iroh::EndpointId;
use sqlx::postgres::PgListener;
use std::time::Duration;
use tokio::{sync::broadcast, task::JoinHandle, time::sleep};

/// Events waiting for a slow stream; one that falls behind reads again.
const BACKLOG: usize = 1024;
/// The wait before listening again after the database refused.
const RELISTEN: Duration = Duration::from_secs(2);

/// What a stream hears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// `user` has notices up to `id`.
    Notice { user: String, id: i64 },
    /// End the streams of `key`.
    End { key: EndpointId },
    /// Events may have been missed: read everything again.
    Resync,
}

/// The listener of this instance. Dropping it stops listening.
pub struct Hub {
    heard: broadcast::Sender<Heard>,
    task: JoinHandle<()>,
}

impl Hub {
    /// Listens on `store`'s database from now on. Update counters that move
    /// wake `Store::watch_updates` here too.
    pub async fn start(store: Store) -> Result<Self> {
        let (heard, _) = broadcast::channel(BACKLOG);
        let mut listener = PgListener::connect_with(store.pool()).await?;
        listener.listen(EVENTS_CHANNEL).await?;
        let task = tokio::spawn(listen(listener, store, heard.clone()));
        Ok(Self { heard, task })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Heard> {
        self.heard.subscribe()
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn listen(mut listener: PgListener, store: Store, heard: broadcast::Sender<Heard>) {
    loop {
        match listener.try_recv().await {
            Ok(Some(notification)) => {
                if let Some(event) = parse(notification.payload(), &store) {
                    // No stream open is no error.
                    let _ = heard.send(event);
                }
            }
            // The connection dropped; the next call opens another.
            Ok(None) => {
                log::warn!("events: the database connection dropped; listening again");
                let _ = heard.send(Heard::Resync);
            }
            Err(e) => {
                log::warn!("events: listening failed: {e:#}");
                let _ = heard.send(Heard::Resync);
                sleep(RELISTEN).await;
            }
        }
    }
}

/// `n <id> <user>`, `u <last> <user>` or `k 0 <key>`. An update wakes the
/// update streams here and is not passed on.
fn parse(payload: &str, store: &Store) -> Option<Heard> {
    let mut parts = payload.splitn(3, ' ');
    let (kind, number, name) = (parts.next()?, parts.next()?, parts.next()?);
    let number: i64 = number.parse().ok()?;
    match kind {
        "n" => Some(Heard::Notice {
            user: name.to_owned(),
            id: number,
        }),
        "u" => {
            store.updated_elsewhere(Bumped {
                user_id: name.to_owned(),
                last: number,
            });
            None
        }
        "k" => Some(Heard::End {
            key: name.parse().ok()?,
        }),
        _ => {
            log::warn!("events: unknown event {kind}");
            None
        }
    }
}
