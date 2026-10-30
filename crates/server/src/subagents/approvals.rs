//! Approvals background calls wait for: the card goes to the user's devices
//! as a notice on the chat the work belongs to, and the answer comes back
//! through the same `AnswerApproval` a turn's card uses. Once a question
//! stops waiting, answered or withdrawn, a resolved notice tells every
//! device to clear its card, and `ListQuestions` lists the ones still
//! waiting.

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
    mem,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

/// Calls waiting for the user, by the query id on their card.
pub struct Approvals {
    notifications: broadcast::Sender<UserNotice>,
    waiting: Mutex<Questions>,
}

#[derive(Default)]
struct Questions {
    by_id: HashMap<String, Waiting>,
    /// Counts the questions asked, so they list in the order asked.
    asked: u64,
}

/// Who a question is for, the chat it opens and its card.
struct Question {
    user: String,
    chat: Uuid,
    approval: Approval,
}

struct Waiting {
    question: Question,
    tool: String,
    /// When it was asked, by [`Questions::asked`].
    order: u64,
    answer: oneshot::Sender<GateAnswer>,
}

impl Approvals {
    pub fn new(notifications: broadcast::Sender<UserNotice>) -> Self {
        Self {
            notifications,
            waiting: Mutex::default(),
        }
    }

    fn waiting(&self) -> MutexGuard<'_, Questions> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Tells the user who was asked `query_id` that it stopped waiting.
    fn resolved(&self, query_id: String, asked: Question) {
        let question = PendingApproval {
            run_id: String::new(),
            query_id,
            approval: asked.approval,
        };
        // Nobody listening only means no device is connected, as in `ask`.
        let _ = self.notifications.send(UserNotice {
            user: asked.user,
            notice: Notice::resolved(asked.chat.to_string(), question),
        });
    }

    /// The questions `user`'s background work in `chat` waits on, oldest
    /// first.
    pub(crate) fn pending(&self, user: &str, chat: Uuid) -> Vec<PendingApproval> {
        let waiting = self.waiting();
        let mut pending: Vec<(u64, PendingApproval)> = waiting
            .by_id
            .iter()
            .filter(|(_, asked)| asked.question.user == user && asked.question.chat == chat)
            .map(|(query_id, asked)| {
                let question = PendingApproval {
                    run_id: String::new(),
                    query_id: query_id.clone(),
                    approval: asked.question.approval.clone(),
                };
                (asked.order, question)
            })
            .collect();
        pending.sort_by_key(|(order, _)| *order);
        pending.into_iter().map(|(_, question)| question).collect()
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
        {
            let mut waiting = self.waiting();
            waiting.asked += 1;
            let order = waiting.asked;
            waiting.by_id.insert(
                query_id.clone(),
                Waiting {
                    question: Question {
                        user: user.to_owned(),
                        chat,
                        approval: approval.clone(),
                    },
                    tool: tool.to_owned(),
                    order,
                    answer,
                },
            );
        }
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
            match waiting.by_id.get(query_id) {
                Some(asked) if asked.question.user == user.id() => waiting.by_id.remove(query_id),
                _ => None,
            }
        };
        let Some(Waiting {
            question,
            tool,
            answer,
            ..
        }) = waiting
        else {
            return Ok(false);
        };
        self.resolved(query_id.to_owned(), question);
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
            .by_id
            .iter()
            .filter(|(_, asked)| asked.question.user == user)
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
        let withdrawn = self.approvals.waiting().by_id.remove(&self.query_id);
        if let Some(asked) = withdrawn {
            self.approvals
                .resolved(mem::take(&mut self.query_id), asked.question);
        }
    }
}
