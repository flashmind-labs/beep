//! How many reports each key may send in a UTC day, kept by one task.

use std::collections::HashMap;

use anyhow::Context;
use chrono::NaiveDate;
use iroh::EndpointId;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

const QUEUE: usize = 256;

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
        let runtime = Handle::try_current().context("the report limit needs a tokio runtime")?;
        let (asks, rx) = mpsc::channel(QUEUE);
        runtime.spawn(run(rx, per_day));
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

async fn run(mut asks: mpsc::Receiver<Ask>, per_day: u32) {
    let mut counts: HashMap<EndpointId, Count> = HashMap::new();
    let mut today = NaiveDate::MIN;
    while let Some(ask) = asks.recv().await {
        match ask {
            Ask::Take { key, day, reply } => {
                if day > today {
                    today = day;
                    counts.retain(|_, count| count.day >= today);
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
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn monday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 2).expect("a real date")
    }

    #[tokio::test]
    async fn each_key_gets_its_own_count_and_a_new_day_starts_over() {
        let limit = KeyLimit::spawn(2).expect("inside a runtime");
        let a = SecretKey::generate().public();
        let b = SecretKey::generate().public();
        let tuesday = monday().succ_opt().expect("a next day");
        for expected in [true, true, false] {
            assert_eq!(
                limit.take(a, monday()).await.expect("the task runs"),
                expected
            );
        }
        assert!(limit.take(b, monday()).await.expect("the task runs"));
        assert!(limit.take(a, tuesday).await.expect("the task runs"));
    }

    #[tokio::test]
    async fn a_report_given_back_can_be_sent_again() {
        let limit = KeyLimit::spawn(1).expect("inside a runtime");
        let key = SecretKey::generate().public();
        assert!(limit.take(key, monday()).await.expect("the task runs"));
        limit.give_back(key, monday()).await.expect("the task runs");
        assert!(limit.take(key, monday()).await.expect("the task runs"));
        assert!(!limit.take(key, monday()).await.expect("the task runs"));
    }

    #[test]
    fn outside_a_runtime_spawn_fails_instead_of_panicking() {
        assert!(KeyLimit::spawn(1).is_err());
    }
}
