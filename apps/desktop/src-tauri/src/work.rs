//! Background work: the agents and jobs a chat started, which go on after
//! its turn ends, and the questions they ask through notices.

use crate::{
    beep::{Beep, Field},
    commands::Decision,
    error::{DesktopError, Result},
};
use beep_proto::{
    AnswerApproval, ApiError, CancelWork, ListQuestions, ListWork, PendingApproval, WorkItem,
    WorkKind,
};
use serde::Serialize;
use std::{sync::Arc, time::UNIX_EPOCH};
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

/// An agent or job as the bar lists it.
#[derive(Debug, Serialize)]
pub struct WorkView {
    id: String,
    /// A sub-agent; otherwise one tool call moved to the background.
    agent: bool,
    task: String,
    /// In milliseconds since the Unix epoch.
    started_at: u64,
    step: String,
    /// `None` while it runs.
    outcome: Option<OutcomeView>,
}

#[derive(Debug, Serialize)]
struct OutcomeView {
    ok: bool,
    text: String,
}

impl From<WorkItem> for WorkView {
    fn from(item: WorkItem) -> Self {
        let started_at = item
            .started_at
            .duration_since(UNIX_EPOCH)
            .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default();
        Self {
            id: item.id,
            agent: matches!(item.kind, WorkKind::Agent {}),
            task: item.task,
            started_at,
            step: item.step,
            outcome: item.outcome.map(|outcome| OutcomeView {
                ok: outcome.ok,
                text: outcome.text,
            }),
        }
    }
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

#[tauri::command]
pub async fn list_work(beep: Service<'_>, session: String) -> Result<Vec<WorkView>> {
    let items = beep
        .call(ListWork {
            session_id: session_id(&session)?,
        })
        .await?;
    Ok(items.into_iter().map(WorkView::from).collect())
}

/// Stops one agent or job; `false` when it had already ended.
#[tauri::command]
pub async fn cancel_work(beep: Service<'_>, session: String, id: String) -> Result<bool> {
    beep.call(CancelWork {
        session_id: session_id(&session)?,
        id,
    })
    .await
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
    session: String,
    query_id: String,
    decision: Decision,
) -> Result<WorkAnswer> {
    let asked = beep
        .call(AnswerApproval {
            session_id: session_id(&session)?,
            query_id,
            decision: decision.into(),
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
