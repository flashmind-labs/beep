//! Asks every machine of the mesh at once for its part of a report, each
//! under its own time limit, so one quiet machine holds up nobody.

use beep_mesh::machine::log;
use beep_mesh::{DeviceKind, MeshError, PeerClient};
use beep_proto::node::logs::MAX_LINES;
use beep_proto::node::report::{MAX_BYTES, PROTOCOL, Reply, Request};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::timeout;

/// How long one machine has to give its share, the `logs/1` fallback included.
pub const ASK_LIMIT: Duration = Duration::from_secs(8);

/// A machine to ask.
pub struct Target {
    pub id: EndpointId,
    pub name: String,
    pub kind: DeviceKind,
    /// `Err` says why it cannot be reached, such as "offline".
    pub client: Result<PeerClient, String>,
}

/// How a machine answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    Answered,
    /// It has no `report/1`: the share holds its log only.
    OlderNode,
    Offline {
        reason: String,
    },
    /// It did not answer within `ASK_LIMIT`.
    Silent,
    Failed {
        reason: String,
    },
}

/// One machine's part of a report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Share {
    pub id: EndpointId,
    pub name: String,
    pub kind: DeviceKind,
    pub answer: Answer,
    pub log: String,
    pub status: String,
    pub version: String,
    pub os: String,
    pub os_version: String,
}

impl Share {
    fn empty(id: EndpointId, name: String, kind: DeviceKind, answer: Answer) -> Self {
        Self {
            id,
            name,
            kind,
            answer,
            log: String::new(),
            status: String::new(),
            version: String::new(),
            os: String::new(),
            os_version: String::new(),
        }
    }
}

/// The most log bytes a share keeps (`u32` to `usize` is lossless here).
const MAX_LOG: usize = MAX_BYTES as usize;

/// Every target's share, in `targets` order, from log lines at or after
/// `since` (Unix seconds).
pub async fn collect(targets: Vec<Target>, since: i64) -> Vec<Share> {
    // A time before 1970 asks for the whole log.
    let since = u64::try_from(since).unwrap_or(0);
    let mut shares = Vec::with_capacity(targets.len());
    let mut tasks = JoinSet::new();
    let mut slots = HashMap::new();
    for (slot, target) in targets.into_iter().enumerate() {
        let Target {
            id,
            name,
            kind,
            client,
        } = target;
        match client {
            Ok(client) => {
                let mut share = Share::empty(id, name, kind, Answer::Silent);
                shares.push(share.clone());
                let task = tasks.spawn(async move {
                    if let Ok(answer) = timeout(ASK_LIMIT, ask(&client, since, &mut share)).await {
                        share.answer = answer;
                    }
                    share.log = newest_lines(share.log, MAX_LOG);
                    share
                });
                slots.insert(task.id(), slot);
            }
            Err(reason) => shares.push(Share::empty(id, name, kind, Answer::Offline { reason })),
        }
    }
    while let Some(joined) = tasks.join_next_with_id().await {
        match joined {
            Ok((task, share)) => {
                if let Some(&slot) = slots.get(&task) {
                    shares[slot] = share;
                }
            }
            Err(e) => {
                if let Some(&slot) = slots.get(&e.id()) {
                    shares[slot].answer = Answer::Failed {
                        reason: format!("asking it failed: {e}"),
                    };
                }
            }
        }
    }
    shares
}

/// Asks `report/1`, or `logs/1` from a node that predates it, filling
/// `share` as answers come in.
async fn ask(client: &PeerClient, since: u64, share: &mut Share) -> Answer {
    let request = Request {
        since,
        max_bytes: MAX_BYTES,
    };
    match client.call::<_, Reply>(PROTOCOL, &request).await {
        Ok(Reply::Share {
            log,
            status,
            version,
            os,
            os_version,
        }) => {
            share.log = log;
            share.status = status;
            share.version = version;
            share.os = os;
            share.os_version = os_version;
            Answer::Answered
        }
        Ok(Reply::Failed { reason }) => Answer::Failed { reason },
        Ok(other) => Answer::Failed {
            reason: format!("an answer this version cannot read: {other:?}"),
        },
        Err(MeshError::Declined { .. }) => match log(client, MAX_LINES).await {
            Ok(text) => {
                share.log = text;
                Answer::OlderNode
            }
            Err(MeshError::Declined { .. }) => Answer::OlderNode,
            Err(e) => Answer::Failed {
                reason: e.to_string(),
            },
        },
        Err(e) => Answer::Failed {
            reason: e.to_string(),
        },
    }
}

/// The newest whole lines of `log` that fit in `max` bytes.
fn newest_lines(log: String, max: usize) -> String {
    if log.len() <= max {
        return log;
    }
    let from = log.len() - max;
    // A line starts right after a newline; one longer than `max` is dropped.
    match log.as_bytes()[from - 1..].iter().position(|&b| b == b'\n') {
        Some(newline) => log[from + newline..].to_owned(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_mesh::SessionFn;
    use beep_proto::node::{LinkRx, LinkTx, Open, Opened, logs, report};
    use iroh::SecretKey;
    use std::future::pending;
    use std::sync::Arc;
    use tokio::time::Instant;

    async fn share(mut tx: LinkTx, mut rx: LinkRx, log: String) {
        let Ok(Some(open)) = rx.recv::<Open>().await else {
            return;
        };
        if open.protocol != report::PROTOCOL {
            let reason = format!("this node does not offer {}", open.protocol);
            tx.send(&Opened::Refused { reason }).await.ok();
            return;
        }
        tx.send(&Opened::Ready {}).await.ok();
        let Ok(Some(Request { .. })) = rx.recv::<Request>().await else {
            return;
        };
        let reply = Reply::Share {
            log,
            status: "running".into(),
            version: "0.1.0".into(),
            os: "linux".into(),
            os_version: "6.1".into(),
        };
        tx.send(&reply).await.ok();
    }

    async fn silent(mut tx: LinkTx, mut rx: LinkRx) {
        rx.recv::<Open>().await.ok();
        tx.send(&Opened::Ready {}).await.ok();
        rx.recv::<Request>().await.ok();
        pending::<()>().await;
    }

    async fn older(mut tx: LinkTx, mut rx: LinkRx) {
        let Ok(Some(open)) = rx.recv::<Open>().await else {
            return;
        };
        if open.protocol != logs::PROTOCOL {
            let reason = format!("this node does not offer {}", open.protocol);
            tx.send(&Opened::Refused { reason }).await.ok();
            return;
        }
        tx.send(&Opened::Ready {}).await.ok();
        rx.recv::<logs::Request>().await.ok();
        let reply = logs::Reply::Log {
            text: "a\nb\n".into(),
        };
        tx.send(&reply).await.ok();
    }

    fn sharing(log: &str) -> SessionFn {
        let log = log.to_owned();
        Arc::new(move |tx, rx| Box::pin(share(tx, rx, log.clone())))
    }

    fn target(name: &str, client: Result<PeerClient, String>) -> Target {
        Target {
            id: SecretKey::generate().public(),
            name: name.into(),
            kind: DeviceKind::Node,
            client,
        }
    }

    fn local(session: SessionFn) -> Result<PeerClient, String> {
        Ok(PeerClient::local(session))
    }

    #[tokio::test]
    async fn each_machine_gives_its_share() {
        let targets = vec![
            target("build-box", local(sharing("one\n"))),
            target("gpu-box", local(sharing("two\n"))),
        ];
        let shares = collect(targets, 0).await;
        assert_eq!(shares.len(), 2);
        assert_eq!(shares[0].name, "build-box");
        assert_eq!(shares[0].answer, Answer::Answered);
        assert_eq!(shares[0].log, "one\n");
        assert_eq!(shares[0].version, "0.1.0");
        assert_eq!(shares[1].name, "gpu-box");
        assert_eq!(shares[1].answer, Answer::Answered);
        assert_eq!(shares[1].log, "two\n");
    }

    #[tokio::test(start_paused = true)]
    async fn a_machine_that_never_answers_is_silent_and_does_not_hold_up_the_rest() {
        let targets = vec![
            target(
                "build-box",
                local(Arc::new(|tx, rx| Box::pin(silent(tx, rx)))),
            ),
            target("gpu-box", local(sharing("two\n"))),
        ];
        let start = Instant::now();
        let shares = collect(targets, 0).await;
        assert!(start.elapsed() <= ASK_LIMIT + Duration::from_secs(2));
        assert_eq!(shares[0].answer, Answer::Silent);
        assert_eq!(shares[1].answer, Answer::Answered);
        assert_eq!(shares[1].log, "two\n");
    }

    #[tokio::test]
    async fn an_older_node_falls_back_to_its_log() {
        let targets = vec![target(
            "build-box",
            local(Arc::new(|tx, rx| Box::pin(older(tx, rx)))),
        )];
        let shares = collect(targets, 0).await;
        assert_eq!(shares[0].answer, Answer::OlderNode);
        assert_eq!(shares[0].log, "a\nb\n");
    }

    #[tokio::test]
    async fn an_unreachable_machine_is_listed_offline() {
        let targets = vec![target("build-box", Err("offline".into()))];
        let shares = collect(targets, 0).await;
        assert_eq!(
            shares[0].answer,
            Answer::Offline {
                reason: "offline".into()
            }
        );
    }

    #[tokio::test]
    async fn an_oversized_log_keeps_its_newest_lines() {
        let mut log = String::new();
        let mut n = 0u32;
        while log.len() < 2 * 1024 * 1024 {
            log.push_str(&format!("line {n} of the log\n"));
            n += 1;
        }
        let last = format!("line {} of the log\n", n - 1);
        let shares = collect(vec![target("build-box", local(sharing(&log)))], 0).await;
        let cut = &shares[0].log;
        assert!(cut.len() <= MAX_LOG, "{}", cut.len());
        assert!(cut.len() > 1024 * 1024 - 64, "{}", cut.len());
        assert!(cut.starts_with("line "), "{:?}", &cut[..20]);
        assert!(cut.ends_with(&last));
    }
}
