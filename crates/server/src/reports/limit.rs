//! How many reports each key may send in a UTC day, kept by one task.

use std::collections::HashMap;

use chrono::NaiveDate;
use iroh::EndpointId;
use tokio::sync::{mpsc, oneshot};

const QUEUE: usize = 256;

/// The handle to the task; the task ends when the last handle drops.
#[derive(Clone)]
pub(crate) struct KeyLimit {
    asks: mpsc::Sender<Ask>,
}

struct Ask {
    key: EndpointId,
    day: NaiveDate,
    reply: oneshot::Sender<bool>,
}

struct Count {
    day: NaiveDate,
    sent: u32,
}

impl KeyLimit {
    pub(crate) fn spawn(per_day: u32) -> Self {
        let (asks, rx) = mpsc::channel(QUEUE);
        tokio::spawn(run(rx, per_day));
        Self { asks }
    }

    /// Uses one of `key`'s reports for `day`; false when none are left.
    pub(crate) async fn take(&self, key: EndpointId, day: NaiveDate) -> anyhow::Result<bool> {
        let (reply, answer) = oneshot::channel();
        self.asks
            .send(Ask { key, day, reply })
            .await
            .map_err(|_| anyhow::anyhow!("the report limit task ended"))?;
        Ok(answer.await?)
    }
}

async fn run(mut asks: mpsc::Receiver<Ask>, per_day: u32) {
    let mut counts: HashMap<EndpointId, Count> = HashMap::new();
    let mut today = NaiveDate::MIN;
    while let Some(ask) = asks.recv().await {
        if ask.day > today {
            today = ask.day;
            counts.retain(|_, count| count.day >= today);
        }
        let count = counts.entry(ask.key).or_insert(Count {
            day: ask.day,
            sent: 0,
        });
        if count.day != ask.day {
            *count = Count {
                day: ask.day,
                sent: 0,
            };
        }
        let allowed = count.sent < per_day;
        if allowed {
            count.sent += 1;
        }
        // A sender that stopped waiting needs no answer.
        let _ = ask.reply.send(allowed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[tokio::test]
    async fn each_key_gets_its_own_count_and_a_new_day_starts_over() {
        let limit = KeyLimit::spawn(2);
        let a = SecretKey::generate().public();
        let b = SecretKey::generate().public();
        let monday = NaiveDate::from_ymd_opt(2026, 11, 2).expect("a real date");
        let tuesday = monday.succ_opt().expect("a next day");
        for expected in [true, true, false] {
            assert_eq!(
                limit.take(a, monday).await.expect("the task runs"),
                expected
            );
        }
        assert!(limit.take(b, monday).await.expect("the task runs"));
        assert!(limit.take(a, tuesday).await.expect("the task runs"));
    }
}
