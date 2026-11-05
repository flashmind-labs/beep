//! Answering an approval a chat's turn waits on, from the bar, the orb, the
//! command center or a notification, and following the run it resumes.

use crate::{
    assistant::{Runs, local},
    beep::{BarEvent, Beep, Decision, RunNews, Service},
    error::Result,
    fleet::projects::FleetState,
    tray::waiting::WaitingList,
    voice::{Answered, Voice},
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, ipc::Channel};

/// An approval card that was answered, for every webview to drop.
#[derive(Debug, Clone, Serialize)]
struct ApprovalAnswered {
    session: String,
    query_id: String,
}

/// An answer to an approval a chat's turn waits on.
pub(crate) struct ChatAnswer {
    pub(crate) session: String,
    pub(crate) query_id: String,
    pub(crate) decision: Decision,
}

/// Answers an approval card and streams the run it resumes into `on_event`.
// Tauri fills in the state arguments; the window passes only the rest.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn answer_approval(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    runs: State<'_, Runs>,
    session: String,
    query_id: String,
    decision: Decision,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    // A chat on this Mac waits in its own reply for the answer; no server is asked.
    if local::on(&beep, &fleet).await {
        return local::answer(&runs, &session, &query_id, decision, on_event).await;
    }
    let answer = ChatAnswer {
        session,
        query_id,
        decision,
    };
    answer_chat(&app, answer, Some(on_event)).await
}

/// Answers an approval a turn waits on and streams the run it resumes into
/// `on_event`. Without one, from a notification, the run is followed only
/// when live mode reads it out; the bar follows it on `approval-answered`.
pub(crate) async fn answer_chat(
    app: &AppHandle,
    answer: ChatAnswer,
    on_event: Option<Channel<BarEvent>>,
) -> Result<()> {
    let ChatAnswer {
        session,
        query_id,
        decision,
    } = answer;
    let beep = app.state::<Arc<Beep>>().inner().clone();
    let voice = app.state::<Arc<Voice>>().inner().clone();
    let run = beep.approve(&session, query_id.clone(), decision).await?;
    app.state::<WaitingList>().poke();
    let answered = ApprovalAnswered {
        session: session.clone(),
        query_id: query_id.clone(),
    };
    // Whichever of the bar and the orb answered, the other drops its card,
    // and live mode reads the rest of the reply out when it waits on it.
    let news = voice.answered(Answered {
        session,
        query_id,
        run: run.clone(),
        decision,
    });
    // Then every webview still showing the card drops it. Sent after live
    // mode's event, so the bar follows the run with the decision it names.
    if let Err(e) = app.emit("approval-answered", answered) {
        log::warn!("telling the windows an approval was answered: {e}");
    }
    let on_event = match (on_event, &news) {
        (Some(channel), _) => channel,
        (None, Some(_)) => Channel::new(|_| Ok(())),
        (None, None) => return Ok(()),
    };
    let followed = beep.follow_answered(run, on_event, news.as_ref()).await;
    if let (Err(e), Some(news)) = (&followed, &news) {
        news.tell(RunNews::Failed(e.to_string()));
    }
    followed
}

#[cfg(test)]
mod tests {
    use super::ApprovalAnswered;

    #[test]
    fn approval_answered_names_its_keys() {
        let payload = serde_json::to_value(ApprovalAnswered {
            session: "chat".into(),
            query_id: "query".into(),
        })
        .expect("a plain struct serializes");
        assert_eq!(
            payload,
            serde_json::json!({ "session": "chat", "query_id": "query" })
        );
    }
}
