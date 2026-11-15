//! Asks every machine of the mesh at once for its part of a report, each
//! under its own time limit, so one quiet machine holds up nobody.

use beep_mesh::machine::log;
use beep_mesh::{DeviceKind, MeshError, PeerClient};
use beep_proto::node::logs::MAX_LINES;
use beep_proto::node::report::{MAX_BYTES, PROTOCOL, Reply, Request};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use std::{fs, io};
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

/// The most log bytes a share keeps (`u32` to `usize` is lossless here).
const MAX_LOG: usize = MAX_BYTES as usize;

/// Every target's share, in `targets` order, from log lines at or after
/// `since` (Unix seconds).
pub async fn collect(targets: Vec<Target>, since: i64) -> Vec<Share> {
    collect_within(targets, since, MAX_BYTES).await
}

/// `collect` with at most `max_bytes` of each log (up to `MAX_BYTES`); 0
/// asks for the status alone.
pub async fn collect_within(targets: Vec<Target>, since: i64, max_bytes: u32) -> Vec<Share> {
    let max_bytes = max_bytes.min(MAX_BYTES);
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
                    let asked = ask(&client, since, max_bytes, &mut share);
                    if let Ok(answer) = timeout(ASK_LIMIT, asked).await {
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
async fn ask(client: &PeerClient, since: u64, max_bytes: u32, share: &mut Share) -> Answer {
    let request = Request { since, max_bytes };

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
                reason: format!("{e:#}"),
            },
        },
        Err(e) => Answer::Failed {
            reason: format!("{e:#}"),
        },
    }
}

/// The newest lines of the daily `beep.*.log` files in `dir`, as much as a
/// share keeps: the log of an app with no node to ask, such as a phone's.
pub fn local_log(dir: &Path) -> io::Result<String> {
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

    Ok(newest_lines(log, MAX_LOG))
}

/// The newest whole lines of `log` that fit in `max` bytes.
pub(crate) fn newest_lines(log: String, max: usize) -> String {
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
#[path = "tests/collect.rs"]
mod tests;
