//! The stream the caller follows one session on.

use super::{AgentKind, Agents, Begin, acp::Prompt, live, session::Command};
use anyhow::{Context, Result};
use beep_proto::ModelId;
use beep_proto::SessionId;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{self, Event, Request, SessionInfo, State, Status},
};
use beep_proto::watched;
use std::sync::Arc;

/// Why a request is not an order for a session.
pub(crate) enum Refused {
    Images(String),
    Request(Request),
}

impl TryFrom<Request> for Command {
    type Error = Refused;

    fn try_from(request: Request) -> Result<Self, Refused> {
        Ok(match request {
            Request::Prompt { text } => Command::Prompt(text.into()),
            Request::PromptImages { text, images } => {
                Command::Prompt(Prompt::with_images(text, images).map_err(Refused::Images)?)
            }
            Request::Steer { text, images } => {
                Command::Steer(Prompt::with_images(text, images).map_err(Refused::Images)?)
            }
            Request::DropSteers {} => Command::DropSteers,
            Request::Answer { question, reply } => Command::Answer { question, reply },
            Request::SetMode { mode } => Command::SetMode(mode),
            Request::StopTask { task } => Command::StopTask(task),
            Request::SetDevice { device } => Command::SetDevice(device),
            // Stop ends the main turn alone.
            Request::Cancel {} => Command::Cancel { by_user: true },
            Request::StopAll {} => Command::StopAll { by_user: true },
            Request::Close {} => Command::Close { by_user: true },
            Request::Configure {
                model,
                effort,
                fast,
            } => Command::Configure {
                model,
                effort,
                fast,
            },
            other => return Err(Refused::Request(other)),
        })
    }
}

impl Agents {
    /// Orders from the duplex events stream use the same actor as ACP and legacy streams.
    pub(crate) async fn send_session(
        &self,
        agent: &str,
        id: &SessionId,
        request: Request,
    ) -> Result<SessionInfo, String> {
        let found = self
            .sessions
            .get(id)
            .filter(|session| session.agent == agent);
        let message = matches!(
            request,
            Request::Prompt { .. } | Request::PromptImages { .. } | Request::Steer { .. }
        );
        let ended = found
            .as_ref()
            .is_none_or(|session| matches!(session.info().state.status, Status::Ended));

        // A message to a session that ended opens it again first.
        let session = match found {
            _ if message && ended => {
                let kind = self.catalog.kind_or_refusal(agent)?;
                self.resume(&kind, id, None, None)
                    .await
                    .map_err(|e| format!("{e:#}"))?
            }
            Some(session) => session,
            None => return Err("session is no longer on the machine".to_owned()),
        };

        if matches!(request, Request::Close {}) {
            session.close().await;
            return Ok(session.info());
        }

        let cascade = matches!(request, Request::StopAll {});
        let command = Command::try_from(request).map_err(|refused| match refused {
            Refused::Images(error) => error,
            Refused::Request(_) => "this request cannot order an existing session".to_owned(),
        })?;

        self.family.order_by_user(&session, command).await?;

        if cascade {
            self.family.cancel_family(&session).await;
        }

        Ok(session.info())
    }
}

/// Serves one agent stream: starts or attaches to a session, then passes
/// orders to it and its changes back until either side ends.
pub async fn serve_session(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
    kind: AgentKind,
) -> Result<()> {
    let mut trace = beep_proto::logging::RequestTrace::new("agent attachment");
    trace.phase("reading request");
    let first: Request = rx
        .recv()
        .await
        .inspect_err(|_| trace.finish(false))?
        .context("the stream closed before its first request")
        .inspect_err(|_| trace.finish(false))?;
    trace.phase("starting or attaching");
    let (first, account) = match first {
        Request::StartAs { account, start } => (*start, Some(account)),
        first => (first, None),
    };
    // A start waits on its agent; a resume or attach should answer at once.
    let slow = match &first {
        Request::Resume { session, .. } => {
            Some(format!("resuming {} session {session}", kind.name))
        }
        Request::Attach { session } => {
            Some(format!("attaching to {} session {session}", kind.name))
        }
        _ => None,
    }
    .map(super::requests::warn_if_slow);
    let session = match first {
        first @ (Request::Attach { .. } | Request::Resume { .. }) if account.is_some() => {
            Err(format!("StartAs holds a Start, not {first:?}"))
        }
        Request::Start {
            model: Some(model), ..
        } if !agent::valid_model(&model) => Err(format!("{model:?} is not a model name")),
        Request::Start {
            cwd,
            prompt,
            model,
            effort,
            fast,
            checkout,
            images,
            assistant,
            continues,
        } => match Prompt::with_images(prompt, images) {
            Ok(Prompt { text, images, .. }) => {
                let earlier = continues.as_ref().and_then(|id| agents.sessions.get(id));
                let begin = Begin {
                    prompt: Some(text),
                    images,
                    model: model.map(ModelId::new),
                    effort,
                    fast,
                    assistant,
                    account,
                    continues,
                    ..Begin::default()
                };
                let started = agents
                    .start_in(&kind, cwd.as_deref(), checkout, begin)
                    .await
                    .map_err(|e| format!("{e:#}"));

                // The chat goes on in the new session alone.
                if let (Ok(_), Some(earlier)) = (&started, earlier) {
                    earlier.close().await;
                }

                started
            }
            Err(e) => Err(e),
        },
        Request::Resume {
            model: Some(model), ..
        } if !agent::valid_model(model.as_ref()) => Err(format!("{model:?} is not a model name")),
        Request::Resume {
            session,
            prompt,
            model,
        } => agents
            .resume(&kind, &session, prompt, model)
            .await
            .map_err(|e| format!("{e:#}")),
        Request::Attach { session } => match agents.sessions.get(&session) {
            Some(found) if found.agent == kind.name => Ok(found),
            Some(found) => Err(format!("session {session} runs {}", found.agent)),
            None => Err(format!("there is no session {session}")),
        },
        other => Err(format!("expected Start, Attach or Resume, got {other:?}")),
    };
    drop(slow);
    let session = match session {
        Ok(session) => session,
        Err(reason) => {
            trace.finish(false);
            log::warn!("{} session refused: {reason}", kind.name);
            return tx.send(&Event::Refused { reason }).await;
        }
    };
    let mut live = session.live.clone();

    trace.phase("reading session info");
    if let Err(reason) = session.named_info().await {
        trace.finish(false);
        return tx.send(&Event::Refused { reason }).await;
    }

    let info = watched::read_and_update(&mut live, |live| live.info.clone());
    let mut seen = Seen::new(&info);
    let session_id = info.id.clone();

    trace.phase("sending attachment");
    if let Err(error) = tx
        .send(&Event::Attached {
            session: Box::new(info),
        })
        .await
    {
        trace.finish(false);
        log::warn!(
            "{} session {session_id}: failed to send attachment: {error:#}",
            kind.name
        );
        return Err(error);
    }

    trace.finish(true);
    drop(trace);
    log::info!("{} session {session_id}: attachment sent", kind.name);

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
                let request = match request {
                    Ok(Some(request)) => request,
                    // The session keeps running for the next stream.
                    Ok(None) => return Ok(()),
                    Err(e) => {
                        log::debug!("skipped a request: {e:#}");
                        tx.send(&Event::Ack { error: Some("unknown request".into()) }).await?;
                        continue;
                    }
                };
                // Stop all goes down to what the session started, on other
                // machines too, after its own turn stops.
                let cascade = matches!(request, Request::StopAll {});
                let command = match Command::try_from(request) {
                    Ok(command) => command,
                    Err(refused) => {
                        let error = match refused {
                            Refused::Images(error) => error,
                            Refused::Request(other) => format!("{other:?} is not for an open session"),
                        };
                        tx.send(&Event::Ack { error: Some(error) }).await?;
                        continue;
                    }
                };
                let error = agents.family.order_by_user(&session, command).await.err();

                if cascade {
                    // The session keeps the task, so it ends with it; it is
                    // bounded by the time each machine has to answer.
                    let (agents, family) = (agents.clone(), session.clone());
                    session.keep(tokio::spawn(async move { agents.family.cancel_family(&family).await }));
                }

                tx.send(&Event::Ack { error }).await?;
            }
            changed = live.changed() => {
                let events = watched::read_and_update(&mut live, |live| seen.events(live));
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

/// What a stream has told the caller, to send only what changed.
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

    /// The events that bring the caller up to `live`, state first.
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
                stop_reason: live
                    .info
                    .state
                    .last_stop
                    .unwrap_or(agent::StopReason::Other),
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
