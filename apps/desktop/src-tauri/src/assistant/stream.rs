//! A reply on its way to the window: runs the prompt and draws what the model and the tools
//! produce as the `BarEvent`s the chat already draws.

use super::work;
use crate::{beep::BarEvent, chat::markdown};
use beep_assistant::{Chat, Event, Outcome, Setup, WorkQuery, run_turn};
use tauri::{AppHandle, ipc::Channel};
use tokio::sync::mpsc::{self, Receiver};
use tokio_util::sync::CancellationToken;

/// Events waiting to be drawn; a full queue slows the model's stream down.
const EVENT_QUEUE: usize = 256;

/// Runs the prompt and draws what comes, then says how it ended.
pub(super) async fn drive(
    app: AppHandle,
    setup: Setup,
    mut chat: Chat,
    message: String,
    channel: Channel<BarEvent>,
) {
    let id = chat.id.to_string();
    let started = BarEvent::Started {
        run_id: id.clone(),
        session_id: id,
    };
    if channel.send(started).is_err() {
        return;
    }
    let (tx, rx) = mpsc::channel(EVENT_QUEUE);
    let cancel = setup.cancel.clone();
    // Each answer owns its handle, so the task's future stays `Send`.
    let ask = |query: WorkQuery| {
        let app = app.clone();
        async move { work::answer(&app, query).await }
    };
    let run = async {
        let outcome = run_turn(&setup, &mut chat, message, &tx, ask).await;
        // Closing the queue ends `forward`.
        drop(tx);
        outcome
    };
    let (outcome, ()) = tokio::join!(run, forward(rx, &channel, &cancel));
    let status = match outcome {
        Ok(Outcome::Done) => "completed",
        Ok(Outcome::Stopped) => "cancelled",
        Err(e) => {
            // A closed window cannot be told; its reply is saved either way.
            channel
                .send(BarEvent::Error {
                    message: e.to_string(),
                })
                .ok();
            "failed"
        }
    };
    channel
        .send(BarEvent::Finished {
            status: status.into(),
        })
        .ok();
}

/// Sends the window what the run produces. The reply goes as the whole text so far, as HTML,
/// batched so a fast stream is not rendered once per word. A window that stops taking events
/// cancels the run.
async fn forward(mut rx: Receiver<Event>, channel: &Channel<BarEvent>, cancel: &CancellationToken) {
    let mut reply = String::new();
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }
        let mut delivered = true;
        let mut dirty = false;
        for event in batch {
            if dirty && !matches!(event, Event::Text(_)) {
                delivered &= channel.send(reply_event(&reply)).is_ok();
                dirty = false;
            }
            match event {
                Event::Text(more) => {
                    reply.push_str(&more);
                    dirty = true;
                }
                Event::Reasoning(delta) => {
                    delivered &= channel.send(BarEvent::Reasoning { delta }).is_ok();
                }
                Event::ToolStarted {
                    call_id,
                    name,
                    label,
                } => {
                    // The next round's text starts a new paragraph.
                    if !reply.is_empty() && !reply.ends_with("\n\n") {
                        reply.push_str("\n\n");
                    }
                    let started = BarEvent::ToolStarted {
                        call_id,
                        name,
                        description: label,
                    };
                    delivered &= channel.send(started).is_ok();
                }
                Event::ToolResult { call_id, name, ok } => {
                    delivered &= channel.send(tool_result(call_id, name, ok)).is_ok();
                }
            }
        }
        if dirty {
            delivered &= channel.send(reply_event(&reply)).is_ok();
        }
        if !delivered {
            cancel.cancel();
        }
    }
}

fn reply_event(reply: &str) -> BarEvent {
    BarEvent::Text {
        html: markdown::untrusted_html(reply),
    }
}

fn tool_result(call_id: String, name: String, success: bool) -> BarEvent {
    BarEvent::ToolResult {
        call_id,
        name,
        success,
        file: None,
        images: Vec::new(),
        products: None,
        cards: None,
        chart: None,
        code_agent: None,
    }
}
