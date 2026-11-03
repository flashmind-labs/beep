//! The questions background sub-agents ask through notices, answered at the
//! end of their chat.

use crate::{
    beep::{Beep, Decision, Field},
    error::{DesktopError, Result},
    voice::Voice,
};
use beep_proto::{AnswerApproval, ApiError, ListQuestions, PendingApproval};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

/// How the server answers a delivered answer to background work: no run
/// resumes, so it says so as a conflict.
const GOES_ON: &str = "background work goes on";

/// A question from background work, as the bar shows it at the end of its
/// chat.
#[derive(Debug, Clone, Serialize)]
pub struct WorkQuestion {
    pub session_id: String,
    pub query_id: String,
    pub app: String,
    pub action: String,
    pub fields: Vec<Field>,
}

impl WorkQuestion {
    fn new(session_id: &str, pending: &PendingApproval) -> Self {
        Self {
            session_id: session_id.to_owned(),
            query_id: pending.query_id.clone(),
            app: pending.approval.app.clone(),
            action: pending.approval.action.clone(),
            fields: pending
                .approval
                .fields
                .iter()
                .cloned()
                .map(Field::from)
                .collect(),
        }
    }

    /// The question a notice carries for `session_id`, or `None` when it
    /// carries none or a turn asked it: a turn's run streams its own card.
    pub fn from_notice(session_id: &str, approval: Option<&PendingApproval>) -> Option<Self> {
        let pending = approval.filter(|pending| pending.run_id.is_empty())?;
        Some(Self::new(session_id, pending))
    }
}

/// A question from background work that no longer waits: answered here or
/// elsewhere, or its work stopped.
#[derive(Debug, Clone, Serialize)]
pub struct Resolved {
    pub session_id: String,
    pub query_id: String,
}

/// What became of an answer to background work.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkAnswer {
    /// The work has it and goes on.
    Delivered,
    /// Nothing waits for it any more: the work stopped or was answered
    /// elsewhere.
    Withdrawn,
}

fn session_id(session: &str) -> Result<Uuid> {
    Uuid::parse_str(session).map_err(|_| DesktopError::BadRequest("bad session".into()))
}

/// The questions background work in chat `session` still waits on, oldest
/// first, so their cards outlive a restart and show on every device.
#[tauri::command]
pub async fn list_questions(beep: Service<'_>, session: String) -> Result<Vec<WorkQuestion>> {
    let pending = beep
        .call(ListQuestions {
            session_id: session_id(&session)?,
        })
        .await?;
    Ok(pending
        .iter()
        .map(|question| WorkQuestion::new(&session, question))
        .collect())
}

#[tauri::command]
pub async fn answer_work(
    beep: Service<'_>,
    voice: State<'_, Arc<Voice>>,
    session: String,
    query_id: String,
    decision: Decision,
) -> Result<WorkAnswer> {
    let answered = answer(&beep, &session, query_id.clone(), decision).await?;
    // Live mode's orb drops its card, however it was answered.
    voice.withdrawn(&query_id);
    Ok(answered)
}

/// Answers a question from background work in chat `session`, from its
/// card or its notification.
pub(crate) async fn answer(
    beep: &Beep,
    session: &str,
    query_id: String,
    decision: Decision,
) -> Result<WorkAnswer> {
    let asked = beep
        .call(AnswerApproval {
            session_id: session_id(session)?,
            query_id,
            decision: decision.into(),
            for_session: decision.for_session(),
        })
        .await;
    match asked {
        // A turn was waiting on it after all, and its run goes on.
        Ok(_) => Ok(WorkAnswer::Delivered),
        Err(DesktopError::Api(ApiError::Conflict { message })) if message.contains(GOES_ON) => {
            Ok(WorkAnswer::Delivered)
        }
        Err(DesktopError::Api(ApiError::Conflict { .. })) => Ok(WorkAnswer::Withdrawn),
        Err(e) => Err(e),
    }
}
