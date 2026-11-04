//! Chats and runs: sending a message, following its events and answering approvals.

use super::Reply;
use super::owned_run;
use crate::{
    agent::images::{MAX_IMAGE_BYTES, MAX_IMAGES},
    agent::live::Subscription,
    api::router::{Cx, Sink},
    app::{Answer, Draws, Message},
    apps::permissions,
    protocol::*,
    store::CreateRun,
};
use std::collections::VecDeque;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

pub(super) async fn chat(cx: Cx, mut req: Chat, sink: Sink<Event>) -> Reply<()> {
    let untyped = req.message.trim().is_empty();
    if (untyped && req.voice.is_none()) || req.message.len() > 32768 {
        return Err(ApiError::bad_request("message must be 1-32768 bytes"));
    }
    if req.voice.is_some() && req.research {
        return Err(ApiError::bad_request("a research starts from text"));
    }
    check_images(&req.images)?;
    check_files(&req.files)?;
    let model = match &req.model {
        Some(id) => Some(cx.app.models.find(id).ok_or(ApiError::NotFound {})?.clone()),
        None => None,
    };
    // Transcribed last: a refused or retried message costs no transcription.
    if let Some(voice) = &req.voice {
        let request = req.request_id.to_string();
        match cx.user.run_for_spoken(req.session_id, &request).await? {
            Some(CreateRun::Created(run) | CreateRun::Existing(run)) => {
                return follow(&cx, run, 0, &sink).await;
            }
            Some(CreateRun::Conflict) => {
                return Err(ApiError::Conflict {
                    message: "session busy".into(),
                });
            }
            None => {}
        }
        let said = cx.app.transcribe(&cx.user, voice).await?;
        req.message = spoken_message(&req.message, &said);
    }
    if req.research {
        if cx.app.models.research().is_none() {
            return Err(ApiError::bad_request(
                "research is not set up on this server",
            ));
        }
        cx.user.start_research(req.session_id, &req.message).await?;
    }
    let message = Message {
        session_id: req.session_id,
        request_id: req.request_id,
        message: req.message,
        cron_id: None,
        scheduled: None,
        always_sent: false,
        woken: None,
        images: req.images,
        files: req.files,
        answer: None,
        model,
        thinking: req.thinking,
        origin: cx.device,
        platform: req.platform,
        draws: draws(&cx),
        finished_research: false,
        voice: req.voice,
        spoken: req.spoken,
    };
    match cx.app.submit(&cx.user, message).await? {
        CreateRun::Created(run) | CreateRun::Existing(run) => follow(&cx, run, 0, &sink).await,
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}

/// The transcript, after anything typed with it.
pub(super) fn spoken_message(typed: &str, said: &str) -> String {
    match typed.trim() {
        "" => said.to_owned(),
        typed => format!("{typed}\n\n{said}"),
    }
}

/// A notice, or a reply, as a voice note.
pub(super) async fn speak_notice(
    cx: Cx,
    SpeakNotice { title, body }: SpeakNotice,
) -> Reply<VoiceNote> {
    if title.len() + body.len() > 32768 {
        return Err(ApiError::bad_request("a notice is at most 32768 bytes"));
    }
    if title.trim().is_empty() && body.trim().is_empty() {
        return Err(ApiError::bad_request("nothing to say"));
    }
    cx.app.speak_notice(&cx.user, &title, &body).await
}

/// What a run started on this connection can show; only the macOS app draws charts.
pub(super) fn draws(cx: &Cx) -> Draws {
    let desktop = cx.device == (DeviceKind::Desktop {});
    Draws {
        charts: desktop && cx.supports(Feature::CHARTS),
        cards: cx.supports(Feature::CARDS),
        sources: cx.supports(Feature::SOURCES),
    }
}

/// Refuses too many images, or one too large.
pub(super) fn check_images(images: &[String]) -> Reply<()> {
    if images.len() > MAX_IMAGES {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_IMAGES} images per message"
        )));
    }
    if images.iter().any(|image| image.len() > MAX_IMAGE_BYTES) {
        return Err(ApiError::bad_request(format!(
            "each image must be at most {} MiB",
            MAX_IMAGE_BYTES >> 20
        )));
    }
    Ok(())
}

/// Refuses files past `MAX_UPLOAD` in all, or one without a name.
pub(super) fn check_files(files: &[Upload]) -> Reply<()> {
    if files.len() > MAX_IMAGES {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_IMAGES} files per message"
        )));
    }
    if files.iter().map(|file| file.data.len()).sum::<usize>() > MAX_UPLOAD {
        return Err(ApiError::bad_request(format!(
            "files may be at most {} MiB in all",
            MAX_UPLOAD >> 20
        )));
    }
    if files
        .iter()
        .any(|file| file.name.trim().is_empty() || file.name.len() > 255 || file.mime.len() > 255)
    {
        return Err(ApiError::bad_request("each file needs a name"));
    }
    Ok(())
}

pub(super) async fn get_run(cx: Cx, GetRun { id }: GetRun) -> Reply<Run> {
    owned_run(&cx, &id).await
}

pub(super) async fn active_run(cx: Cx, ActiveRun { session_id }: ActiveRun) -> Reply<Option<Run>> {
    Ok(cx.user.active_run(session_id).await?)
}

pub(super) async fn list_active_runs(cx: Cx, _: ListActiveRuns) -> Reply<Vec<Run>> {
    Ok(cx.user.active_runs().await?)
}

pub(super) async fn cancel_run(cx: Cx, CancelRun { id }: CancelRun) -> Reply<()> {
    owned_run(&cx, &id).await?;
    cx.app.cancel(&id).await;
    Ok(())
}

/// Resumes a run's events after `after`, the last `seq` the client saw.
pub(super) async fn get_events(cx: Cx, req: GetEvents, sink: Sink<Event>) -> Reply<()> {
    let GetEvents { run_id, after } = req;
    if after < 0 {
        return Err(ApiError::bad_request("after must be >= 0"));
    }
    let run = owned_run(&cx, &run_id).await?;
    follow(&cx, run, after, &sink).await
}

/// Sends the run's events after `after` until it finishes; a finished run keeps only its status.
pub(super) async fn follow(cx: &Cx, run: Run, mut after: i64, sink: &Sink<Event>) -> Reply<()> {
    let Some(live) = cx.app.live(&run.id).await else {
        let finished = Event {
            seq: after + 1,
            event: RunEvent::RunFinished {
                status: run.status,
                updated_at: None,
            },
        };
        sink.send(&finished).await?;
        return Ok(());
    };

    let Subscription {
        backlog,
        live: mut rx,
    } = live.subscribe(after);
    let mut pending = VecDeque::from(backlog);

    loop {
        let event = match pending.pop_front() {
            Some(event) => event,
            None => match rx.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => {
                    pending.extend(live.since(after));
                    continue;
                }
                Err(RecvError::Closed) => return Ok(()),
            },
        };
        if event.seq <= after {
            continue;
        }

        after = event.seq;
        // Only an app that plays voice notes reads a transcript.
        if matches!(event.event, RunEvent::Transcript { .. }) && !cx.supports(Feature::VOICE) {
            continue;
        }
        // Nor does an app from before source chips read a reply's sources.
        if matches!(event.event, RunEvent::Sources { .. }) && !cx.supports(Feature::SOURCES) {
            continue;
        }
        let finished = matches!(event.event, RunEvent::RunFinished { .. });
        sink.send(&event).await?;
        if finished {
            return Ok(());
        }
    }
}

pub(super) async fn device_result(cx: Cx, req: DeviceResult) -> Reply<bool> {
    owned_run(&cx, &req.run_id).await?;
    Ok(cx.app.resolve_device_query(&req.query_id, req.answer).await)
}

/// Resumes a session stopped on a question; answering again returns the first answer's run.
pub(super) async fn answer_approval(cx: Cx, req: AnswerApproval) -> Reply<Run> {
    let AnswerApproval {
        session_id,
        query_id,
        decision,
        for_session,
    } = req;
    // A call of background work waits on its own; nothing resumes.
    if cx
        .app
        .subagents
        .approvals
        .answer(&cx.user, &query_id, decision, for_session)
        .await?
    {
        return Err(ApiError::Conflict {
            message: "answered; the background work goes on".into(),
        });
    }
    let message = Message {
        session_id,
        request_id: Uuid::nil(),
        message: decision.as_str().to_owned(),
        cron_id: None,
        scheduled: None,
        always_sent: false,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        answer: Some(Answer {
            call_id: query_id.clone(),
            decision,
            for_session,
        }),
        model: None,
        thinking: None,
        origin: cx.device,
        platform: None,
        draws: draws(&cx),
        finished_research: false,
        voice: None,
        spoken: false,
    };
    let conversation = cx.user.context(session_id).await?;
    let open = conversation.map(|context| permissions::open_calls(&context.conversation));
    let waiting = open
        .and_then(|open| open.into_iter().next())
        .is_some_and(|call| call.id == query_id);
    let created = if waiting {
        cx.app.submit(&cx.user, message).await?
    } else {
        cx.user
            .existing_run(session_id, &message.request(), &message.message)
            .await?
            .ok_or_else(|| ApiError::Conflict {
                message: "nothing is waiting for this answer".into(),
            })?
    };
    match created {
        CreateRun::Created(run) | CreateRun::Existing(run) => Ok(run),
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}
