//! Questions for the phone: device queries a tool waits on, and the
//! approvals a run stops on.

use super::{App, UserNotice};
use crate::{
    live::LiveRun,
    permissions::ApprovalPayload,
    protocol::{Approval, DeviceAnswer, DeviceQuery, Notice, PendingApproval, Run, RunEvent},
    user::User,
};
use anyhow::Result;
use flashmind::types::tool::ToolResult;
use std::time::Duration;
use tokio::sync::oneshot;
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
    /// Send a query to the connected device and wait for its response.
    ///
    /// Emits a `device_query` event on the run's event stream so the client
    /// picks it up, then blocks until the device POSTs the result back (or the
    /// timeout expires).
    pub async fn device_query(
        &self,
        run_id: &str,
        query: DeviceQuery,
        timeout_secs: u64,
    ) -> Result<DeviceAnswer> {
        let Some(live) = self.live(run_id).await else {
            anyhow::bail!("run {run_id} is not running");
        };
        let (query_id, rx) = self.ask_device(&live, query).await;
        self.wait_device(query_id, rx, timeout_secs).await
    }

    /// Asks the user about the call the run stopped on: the chat shows a
    /// card and the phone a notification. Nothing waits; the answer starts a
    /// new run that resumes the session.
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
            },
        });
    }

    /// Puts `query` on the run's event stream for the phone to answer.
    async fn ask_device(
        &self,
        live: &LiveRun,
        query: DeviceQuery,
    ) -> (String, oneshot::Receiver<DeviceAnswer>) {
        let query_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.device_queries
            .lock()
            .await
            .insert(query_id.clone(), tx);
        live.push(RunEvent::DeviceQuery {
            query_id: query_id.clone(),
            query,
        });
        (query_id, rx)
    }

    async fn wait_device(
        &self,
        query_id: String,
        rx: oneshot::Receiver<DeviceAnswer>,
        timeout_secs: u64,
    ) -> Result<DeviceAnswer> {
        let result = tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await;
        // Clean up on timeout or channel close.
        self.device_queries.lock().await.remove(&query_id);
        match result {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(_)) => anyhow::bail!("device query channel closed without a response"),
            Err(_) => anyhow::bail!("device query timed out after {timeout_secs}s"),
        }
    }

    /// Called by the device-result endpoint to deliver a response for a pending
    /// device query. Returns `true` if the query_id was found and resolved.
    pub async fn resolve_device_query(&self, query_id: &str, answer: DeviceAnswer) -> bool {
        if let Some(tx) = self.device_queries.lock().await.remove(query_id) {
            tx.send(answer).is_ok()
        } else {
            false
        }
    }
}
