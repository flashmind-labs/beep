//! The stream the server follows one session on.

use super::{AGENT_ID_WAIT, AgentKind, Agents, Begin, Command, Prompt, live};
use anyhow::{Context, Result};
use beep_proto::ModelId;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{self, Event, Request, SessionInfo, State, Status},
};
use std::sync::Arc;

/// Serves one agent stream: starts or attaches to a session, then passes
/// orders to it and its changes back until either side ends.
pub async fn serve_session(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
    kind: AgentKind,
) -> Result<()> {
    let first: Request = rx
        .recv()
        .await?
        .context("the stream closed before its first request")?;
    let new = matches!(first, Request::Start { .. });
    let session = match first {
        Request::Start {
            model: Some(model), ..
        } if !agent::valid_model(&model) => Err(format!("{model:?} is not a model name")),
        Request::Start {
            cwd,
            prompt,
            model,
            effort,
            checkout,
        } => {
            let begin = Begin {
                prompt: Some(prompt),
                model: model.map(ModelId::new),
                effort,
                ..Begin::default()
            };
            agents
                .start_in(&kind, cwd.as_deref(), checkout, begin)
                .await
                .map_err(|e| format!("{e:#}"))
        }
        Request::Resume { session, prompt } => agents
            .resume(&kind, &session, prompt)
            .await
            .map_err(|e| format!("{e:#}")),
        Request::Attach { session } => match agents.get(&session) {
            Some(found) if found.agent == kind.name => Ok(found),
            Some(found) => Err(format!("session {session} runs {}", found.agent)),
            None => Err(format!("there is no session {session}")),
        },
        other => Err(format!("expected Start, Attach or Resume, got {other:?}")),
    };
    let session = match session {
        Ok(session) => session,
        Err(reason) => return tx.send(&Event::Refused { reason }).await,
    };
    let mut live = session.live.clone();
    if new {
        // The server learns the session by the agent's own id, which
        // outlives the node, once the agent gave it in time.
        let named =
            live.wait_for(|live| live.agent_session.is_some() || live.status() == Status::Ended);
        if tokio::time::timeout(AGENT_ID_WAIT, named).await.is_err() {
            log::info!(
                "session {} goes by its short id: the agent gave none in time",
                session.id
            );
        }
    }
    let mut seen = Seen::new(&live.borrow_and_update().info);
    tx.send(&Event::Attached {
        session: Box::new(session.info()),
    })
    .await?;
    if seen.state.status == Status::Ended {
        return tx
            .send(&Event::Ended {
                reason: seen.ended(),
            })
            .await;
    }
    loop {
        tokio::select! {
            request = rx.recv::<Request>() => {
                let mut cascade = false;
                let command = match request {
                    Ok(Some(Request::Prompt { text })) => Command::Prompt(text.into()),
                    Ok(Some(Request::PromptImages { text, images })) => {
                        match Prompt::with_images(text, images) {
                            Ok(prompt) => Command::Prompt(prompt),
                            Err(error) => {
                                tx.send(&Event::Ack { error: Some(error) }).await?;
                                continue;
                            }
                        }
                    }
                    Ok(Some(Request::Steer { text, images })) => {
                        match Prompt::with_images(text, images) {
                            Ok(prompt) => Command::Steer(prompt),
                            Err(error) => {
                                tx.send(&Event::Ack { error: Some(error) }).await?;
                                continue;
                            }
                        }
                    }
                    Ok(Some(Request::DropSteers {})) => Command::DropSteers,
                    Ok(Some(Request::Answer { question, reply })) => Command::Answer { question, reply },
                    Ok(Some(Request::SetMode { mode })) => Command::SetMode(mode),
                    Ok(Some(Request::StopTask { task })) => Command::StopTask(task),
                    Ok(Some(Request::Cancel {})) => {
                        // Stop goes down to what the session started, on
                        // other machines too, after its own turn stops.
                        cascade = true;
                        Command::Cancel
                    }
                    Ok(Some(Request::Close {})) => Command::Close,
                    Ok(Some(Request::Configure { model, effort })) => Command::Configure { model, effort },
                    Ok(Some(other)) => {
                        let error = Some(format!("{other:?} is not for an open session"));
                        tx.send(&Event::Ack { error }).await?;
                        continue;
                    }
                    // The session keeps running for the next stream.
                    Ok(None) => return Ok(()),
                    Err(e) => {
                        log::debug!("skipped a request: {e:#}");
                        tx.send(&Event::Ack { error: Some("unknown request".into()) }).await?;
                        continue;
                    }
                };
                let error = session.order(command).await.err();
                if cascade {
                    // The session keeps the task, so it ends with it; it is
                    // bounded by the time each machine has to answer.
                    let (agents, family) = (agents.clone(), session.clone());
                    session.keep(tokio::spawn(async move { agents.cancel_family(&family).await }));
                }
                tx.send(&Event::Ack { error }).await?;
            }
            changed = live.changed() => {
                let events = seen.events(&live.borrow_and_update());
                let mut ended = false;
                for event in events {
                    ended |= matches!(event, Event::Ended { .. });
                    tx.send(&event).await?;
                }
                if ended {
                    return Ok(());
                }
                if changed.is_err() {
                    let reason = "the session stopped".to_owned();
                    return tx.send(&Event::Ended { reason }).await;
                }
            }
        }
    }
}

/// What a stream has told the server, to send only what changed.
struct Seen {
    state: State,
    question: Option<u32>,
    turns: u32,
}

impl Seen {
    fn new(info: &SessionInfo) -> Self {
        Self {
            state: info.state.clone(),
            question: info.question.as_ref().map(|question| question.id),
            turns: info.turns,
        }
    }

    fn ended(&self) -> String {
        self.state.error.clone().unwrap_or_else(|| "closed".into())
    }

    /// The events that bring the server up to `live`, state first.
    fn events(&mut self, live: &live::Live) -> Vec<Event> {
        let info = &live.info;
        let mut events = Vec::new();
        if info.state != self.state {
            self.state = info.state.clone();
            events.push(Event::State {
                state: self.state.clone(),
            });
        }
        let question = info.question.as_ref().map(|question| question.id);
        if question != self.question {
            self.question = question;
            if let Some(question) = &info.question {
                events.push(Event::Question {
                    question: question.clone(),
                });
            }
        }
        if info.turns > self.turns {
            self.turns = info.turns;
            events.push(Event::TurnDone {
                turn: info.turns,
                stop_reason: live.last_stop.unwrap_or(agent::StopReason::Other),
                last_message: info.last_message.clone(),
            });
        }
        if info.state.status == Status::Ended {
            events.push(Event::Ended {
                reason: self.ended(),
            });
        }
        events
    }
}
