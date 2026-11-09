//! How many reports each key may send in a UTC day, kept by one task.

use std::collections::HashMap;

use anyhow::Context;
use chrono::NaiveDate;
use iroh::EndpointId;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

const QUEUE: usize = 256;

/// Keys counted in a day. Keys cost nothing to make, so past this a key not
/// yet counted is refused for the rest of the day.
const MAX_KEYS: usize = 10_000;

/// The handle to the task; the task ends when the last handle drops.
#[derive(Clone)]
pub(crate) struct KeyLimit {
    asks: mpsc::Sender<Ask>,
}

enum Ask {
    /// Uses one report; the reply says whether one was left.
    Take {
        key: EndpointId,
        day: NaiveDate,
        reply: oneshot::Sender<bool>,
    },
    /// Returns a report taken for one the store then failed to keep.
    GiveBack { key: EndpointId, day: NaiveDate },
}

struct Count {
    day: NaiveDate,
    sent: u32,
}

impl KeyLimit {
    /// Starts the task; fails outside a tokio runtime.
    pub(crate) fn spawn(per_day: u32) -> anyhow::Result<Self> {
        Self::spawn_with(per_day, MAX_KEYS)
    }

    fn spawn_with(per_day: u32, max_keys: usize) -> anyhow::Result<Self> {
        let runtime = Handle::try_current().context("the report limit needs a tokio runtime")?;
        let (asks, rx) = mpsc::channel(QUEUE);
        runtime.spawn(run(rx, per_day, max_keys));
        Ok(Self { asks })
    }

    /// Uses one of `key`'s reports for `day`; false when none are left.
    pub(crate) async fn take(&self, key: EndpointId, day: NaiveDate) -> anyhow::Result<bool> {
        let (reply, answer) = oneshot::channel();
        self.ask(Ask::Take { key, day, reply }).await?;
        Ok(answer.await?)
    }

    /// Returns the report `take` used for `day`.
    pub(crate) async fn give_back(&self, key: EndpointId, day: NaiveDate) -> anyhow::Result<()> {
        self.ask(Ask::GiveBack { key, day }).await
    }

    async fn ask(&self, ask: Ask) -> anyhow::Result<()> {
        self.asks
            .send(ask)
            .await
            .map_err(|_| anyhow::anyhow!("the report limit task ended"))
    }
}

async fn run(mut asks: mpsc::Receiver<Ask>, per_day: u32, max_keys: usize) {
    let mut counts: HashMap<EndpointId, Count> = HashMap::new();
    let mut today = NaiveDate::MIN;

    while let Some(ask) = asks.recv().await {
        match ask {
            Ask::Take { key, day, reply } => {
                if day > today {
                    today = day;
                    counts.retain(|_, count| count.day >= today);
                }
                if counts.len() >= max_keys && !counts.contains_key(&key) {
                    // A sender that stopped waiting needs no answer.
                    let _ = reply.send(false);
                    continue;
                }

                let count = counts.entry(key).or_insert(Count { day, sent: 0 });

                if count.day != day {
                    *count = Count { day, sent: 0 };
                }

                let allowed = count.sent < per_day;
                if allowed {
                    count.sent += 1;
                }
                // A sender that stopped waiting needs no answer.
                let _ = reply.send(allowed);
            }
            Ask::GiveBack { key, day } => {
                if let Some(count) = counts.get_mut(&key).filter(|count| count.day == day) {
                    count.sent = count.sent.saturating_sub(1);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/reports_limit.rs"]
mod tests;
