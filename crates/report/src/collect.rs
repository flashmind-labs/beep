//! Asks every machine of the mesh at once for its part of a report, each
//! under its own time limit, so one quiet machine holds up nobody.

use beep_mesh::machine::log;
use beep_mesh::{DeviceKind, MeshError, PeerClient};
use beep_proto::node::logs::MAX_LINES;
use beep_proto::node::report::{PROTOCOL, Reply, Request};
use chrono::DateTime;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use std::{fs, io};
use tokio::task::JoinSet;
use tokio::time::timeout;

/// How long a machine has to give each answer: its share, each further part
/// of its log, and the `logs/1` fallback.
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
    /// Its first answer did not come within `ASK_LIMIT`.
    Silent,
    Failed {
        reason: String,
    },
}

impl Answer {
    /// How the answer reads to the person sending the report.
    pub fn label(&self) -> String {
        match self {
            Answer::Failed { reason } => format!("failed: {reason}"),
            other => other.word().to_owned(),
        }
    }

    /// The answer without any reason text.
    pub(crate) fn word(&self) -> &'static str {
        match self {
            Answer::Answered => "answered",
            Answer::OlderNode => "older node",
            Answer::Silent => "did not answer",
            Answer::Offline { .. } => "offline",
            Answer::Failed { .. } => "failed",
        }
    }
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

/// Every target's share, in `targets` order, with its whole log from
/// `since` (Unix seconds) on.
pub async fn collect(targets: Vec<Target>, since: i64) -> Vec<Share> {
    gather(targets, since, None).await
}

/// `collect` with only the newest `max_bytes` of each log; 0 asks for the
/// status alone.
pub async fn collect_within(targets: Vec<Target>, since: i64, max_bytes: u32) -> Vec<Share> {
    gather(targets, since, Some(max_bytes)).await
}

/// Asks every target; `max_bytes` is `None` for the whole log.
async fn gather(targets: Vec<Target>, since: i64, max_bytes: Option<u32>) -> Vec<Share> {
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
                    share.answer = ask(&client, since, max_bytes, &mut share).await;
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
/// `share` as answers come in. Each answer has `ASK_LIMIT` to arrive, so a
/// long log takes as long as it needs while it keeps coming.
async fn ask(client: &PeerClient, since: u64, max_bytes: Option<u32>, share: &mut Share) -> Answer {
    let request = Request {
        since,
        max_bytes: max_bytes.unwrap_or(u32::MAX),
        whole: max_bytes.is_none(),
    };
    let first = timeout(ASK_LIMIT, async {
        let (tx, mut rx) = client.open_sending(PROTOCOL, &request).await?;
        let reply = rx.recv::<Reply>().await.map_err(MeshError::Link)?;
        Ok::<_, MeshError>((tx, rx, reply))
    });
    // The sending half stays open until the last part is read.
    let (_tx, mut rx) = match first.await {
        Ok(Ok((
            tx,
            rx,
            Some(Reply::Share {
                log,
                status,
                version,
                os,
                os_version,
            }),
        ))) => {
            share.log = log;
            share.status = status;
            share.version = version;
            share.os = os;
            share.os_version = os_version;
            (tx, rx)
        }
        Ok(Ok((_, _, Some(Reply::Failed { reason })))) => return Answer::Failed { reason },
        Ok(Ok((_, _, other))) => {
            return Answer::Failed {
                reason: format!("an answer this version cannot read: {other:?}"),
            };
        }
        Ok(Err(MeshError::Declined { .. })) => return older(client, share).await,
        Ok(Err(e)) => {
            return Answer::Failed {
                reason: format!("{e:#}"),
            };
        }
        Err(_) => return Answer::Silent,
    };

    loop {
        match timeout(ASK_LIMIT, rx.recv::<Reply>()).await {
            Ok(Ok(Some(Reply::Log { text }))) => share.log.push_str(&text),
            Ok(Ok(None)) => return Answer::Answered,
            Ok(Ok(Some(other))) => {
                return Answer::Failed {
                    reason: format!("an answer this version cannot read: {other:?}"),
                };
            }
            Ok(Err(e)) => {
                return Answer::Failed {
                    reason: format!("its log stopped arriving: {e:#}"),
                };
            }
            Err(_) => {
                return Answer::Failed {
                    reason: "its log stopped arriving".into(),
                };
            }
        }
    }
}

/// The latest lines of a node that predates `report/1`, read over `logs/1`.
async fn older(client: &PeerClient, share: &mut Share) -> Answer {
    match timeout(ASK_LIMIT, log(client, MAX_LINES)).await {
        Ok(Ok(text)) => {
            share.log = text;
            Answer::OlderNode
        }
        Ok(Err(MeshError::Declined { .. })) => Answer::OlderNode,
        Ok(Err(e)) => Answer::Failed {
            reason: format!("{e:#}"),
        },
        Err(_) => Answer::Silent,
    }
}

/// The lines of the daily `beep.*.log` files in `dir` from Unix second
/// `since` on: the log of an app with no node to ask, such as a phone's. It
/// is whole, as no message carries it; the draft cuts it to what a report
/// holds.
pub fn local_log(dir: &Path, since: i64) -> io::Result<String> {
    let mut files: Vec<_> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("beep.") && name.ends_with(".log"))
        })
        .collect();
    // Dated names sort oldest first.
    files.sort();
    let mut log = String::new();

    for file in files {
        log.push_str(&String::from_utf8_lossy(&fs::read(file)?));
    }

    Ok(from_time(log, since))
}

/// `log` from its first line stamped at or after Unix second `since`. Lines
/// start with a UTC time such as `2026-01-02T03:04:05.678Z`, which sorts as
/// text; a line with none belongs to the one above it.
fn from_time(log: String, since: i64) -> String {
    let Some(since) = DateTime::from_timestamp(since, 0) else {
        return log;
    };
    let since = since.format("%Y-%m-%dT%H:%M:%S").to_string();
    let stamped = |line: &str| line.as_bytes().first().is_some_and(u8::is_ascii_digit);
    let mut at = 0;

    for line in log.split_inclusive('\n') {
        if stamped(line) && line >= since.as_str() {
            return log[at..].to_owned();
        }

        at += line.len();
    }

    String::new()
}

#[cfg(test)]
#[path = "tests/collect.rs"]
mod tests;
