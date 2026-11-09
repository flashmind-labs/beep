//! Questions for the phone: device queries a tool waits on, and approvals a run stops on.

use super::{App, Reach, UserNotice};
use crate::{
    account::user::User,
    agent::live::LiveRun,
    apps::permissions::ApprovalPayload,
    protocol::{Approval, DeviceAnswer, DeviceQuery, Notice, PendingApproval, Run, RunEvent},
};
use anyhow::Result;
use flashmind::types::tool::ToolResult;
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{sync::oneshot, time::timeout};
use uuid::Uuid;

/// A guarded call the run stopped on.
pub(super) struct Parked {
    pub(super) call_id: String,
    pub(super) approval: Approval,
}

impl Parked {
    pub(super) fn from(call_id: &str, result: &ToolResult) -> Option<Self> {
        let payload = result.payload()?;
        let payload = payload.as_any().downcast_ref::<ApprovalPayload>()?;
        Some(Self {
            call_id: call_id.to_owned(),
            approval: payload.approval.clone(),
        })
    }
}

impl App {
    /// Sends a `device_query` event to the connected device and waits for its answer or the timeout.
    pub async fn device_query(
        &self,
        run_id: &str,
        query: DeviceQuery,
        timeout_secs: u64,
    ) -> Result<DeviceAnswer> {
        let Some(live) = self.live(run_id).await else {
            anyhow::bail!("run {run_id} is not running");
        };
        ask(&self.device_queries, &live, query, timeout_secs).await
    }

    /// Shows the call the run stopped on as a card and notification; the answer starts a new run.
    pub(super) fn park(&self, user: &User, run: &Run, live: &LiveRun, parked: Parked) {
        let Parked { call_id, approval } = parked;
        live.push(RunEvent::DeviceQuery {
            query_id: call_id.clone(),
            query: DeviceQuery::Approve {
                approval: approval.clone(),
            },
        });
        let body = approval
            .fields
            .iter()
            .map(|field| format!("{}: {}", field.label, field.value))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = self.notifications.send(UserNotice {
            user: user.id().to_owned(),
            notice: Notice {
                title: format!("Allow {}: {}?", approval.app, approval.action),
                body,
                session_id: run.session_id.clone(),
                app: None,
                approval: Some(PendingApproval {
                    run_id: run.id.clone(),
                    query_id: call_id,
                    approval,
                }),
                changed: false,
                group: false,
            },
            desktop: None,
            to: Reach::Everyone,
        });
    }

    /// Delivers a device's answer to a pending query; `false` when `query_id` is unknown.
    pub async fn resolve_device_query(&self, query_id: &str, answer: DeviceAnswer) -> bool {
        match pending(&self.device_queries).remove(query_id) {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }
}

/// Questions waiting for the phone's answer, keyed by query id.
pub(super) type DeviceQueries = Mutex<HashMap<String, oneshot::Sender<DeviceAnswer>>>;

fn pending(
    queries: &DeviceQueries,
) -> MutexGuard<'_, HashMap<String, oneshot::Sender<DeviceAnswer>>> {
    queries.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts `query` on the run's event stream and waits for the phone's answer.
async fn ask(
    queries: &DeviceQueries,
    live: &LiveRun,
    query: DeviceQuery,
    timeout_secs: u64,
) -> Result<DeviceAnswer> {
    let query_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    pending(queries).insert(query_id.clone(), tx);
    // Removes the query however the wait ends, including the tool call
    // being dropped when its run is cancelled.
    let _waiting = Waiting {
        queries,
        query_id: &query_id,
    };
    live.push(RunEvent::DeviceQuery {
        query_id: query_id.clone(),
        query,
    });

    match timeout(Duration::from_secs(timeout_secs), rx).await {
        Ok(Ok(answer)) => Ok(answer),
        Ok(Err(_)) => anyhow::bail!("device query channel closed without a response"),
        Err(_) => anyhow::bail!("device query timed out after {timeout_secs}s"),
    }
}

/// A query in `queries`, removed on drop.
struct Waiting<'a> {
    queries: &'a DeviceQueries,
    query_id: &'a str,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        pending(self.queries).remove(self.query_id);
    }
}

#[cfg(test)]
#[path = "../tests/app_device.rs"]
mod tests;
