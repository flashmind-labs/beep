//! Approvals background calls wait for: the card goes to the user's devices
//! as a notice on the chat the work belongs to, and the answer comes back
//! through the same `AnswerApproval` a turn's card uses.

use crate::{
    app::UserNotice,
    permissions,
    protocol::{Approval, ApprovalDecision, Notice, PendingApproval},
    user::User,
};
use anyhow::Result;
use flashmind::types::GateAnswer;
use futures::future::BoxFuture;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

/// Calls waiting for the user, by the query id on their card.
pub struct Approvals {
    notifications: broadcast::Sender<UserNotice>,
    waiting: Mutex<HashMap<String, Waiting>>,
}

struct Waiting {
    user: String,
    tool: String,
    answer: oneshot::Sender<GateAnswer>,
}

impl Approvals {
    pub fn new(notifications: broadcast::Sender<UserNotice>) -> Self {
        Self {
            notifications,
            waiting: Mutex::default(),
        }
    }

    fn waiting(&self) -> MutexGuard<'_, HashMap<String, Waiting>> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Shows `user` the card for a call of `tool` made in `chat`, and returns
    /// the answer to wait on. Dropping it withdraws the question.
    pub(crate) fn ask(
        self: &Arc<Self>,
        user: &str,
        chat: Uuid,
        tool: &str,
        approval: Approval,
    ) -> BoxFuture<'static, GateAnswer> {
        let query_id = Uuid::new_v4().to_string();
        let (answer, answered) = oneshot::channel();
        self.waiting().insert(
            query_id.clone(),
            Waiting {
                user: user.to_owned(),
                tool: tool.to_owned(),
                answer,
            },
        );
        let body = approval
            .fields
            .iter()
            .map(|field| format!("{}: {}", field.label, field.value))
            .collect::<Vec<_>>()
            .join("\n");
        // Nobody listening only means no device is connected; the push
        // service reads the same channel, so there is nothing else to try.
        let _ = self.notifications.send(UserNotice {
            user: user.to_owned(),
            notice: Notice {
                title: format!("Allow {}: {}?", approval.app, approval.action),
                body,
                session_id: chat.to_string(),
                app: None,
                approval: Some(PendingApproval {
                    run_id: String::new(),
                    query_id: query_id.clone(),
                    approval,
                }),
                changed: false,
                group: false,
            },
        });
        let asked = Asked {
            approvals: self.clone(),
            query_id,
        };
        Box::pin(async move {
            let _asked = asked;
            answered.await.unwrap_or_else(|_| {
                GateAnswer::Deny("Not done: the question was withdrawn.".to_owned())
            })
        })
    }

    /// Answers the question `query_id` that `user` was asked. `false` when
    /// nothing of theirs waits under it, such as a turn's own card.
    pub(crate) async fn answer(
        &self,
        user: &User,
        query_id: &str,
        decision: ApprovalDecision,
    ) -> Result<bool> {
        let waiting = {
            let mut waiting = self.waiting();
            match waiting.get(query_id) {
                Some(asked) if asked.user == user.id() => waiting.remove(query_id),
                _ => None,
            }
        };
        let Some(Waiting { tool, answer, .. }) = waiting else {
            return Ok(false);
        };
        let answer_is = match decision {
            ApprovalDecision::Deny {} => GateAnswer::Deny(permissions::DENIED.to_owned()),
            ApprovalDecision::Always {} | ApprovalDecision::Allow {} => GateAnswer::Allow,
        };
        // The call may have timed out or been cancelled since; then there is
        // nobody left to tell, and the answer only counts as received.
        let _ = answer.send(answer_is);
        // Always runs this call first: the card is gone, so a failed save
        // must not turn the user's yes into a refusal.
        if decision == (ApprovalDecision::Always {})
            && let Err(error) = permissions::allow_always(user, &tool).await
        {
            log::warn!("permissions: unable to save Always; user={user} error={error:#}");
        }
        Ok(true)
    }

    /// How many calls wait for `user`.
    #[cfg(test)]
    pub(crate) fn waiting_for(&self, user: &str) -> Vec<String> {
        self.waiting()
            .iter()
            .filter(|(_, asked)| asked.user == user)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Withdraws a question when its call stops waiting.
struct Asked {
    approvals: Arc<Approvals>,
    query_id: String,
}

impl Drop for Asked {
    fn drop(&mut self) {
        self.approvals.waiting().remove(&self.query_id);
    }
}
