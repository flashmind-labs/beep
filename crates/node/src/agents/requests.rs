//! The `agents/1` requests: one answer each, or a stream the server follows.

use super::{
    Agents, AlertHub, Images, MAX_IMAGE, MAX_TRANSCRIPT, PLAN_EVERY, PLAN_REFRESH_GAP, Plans,
    SPEND_EVERY, custom_agents, harnesses, media_file, reach, unix_now, work_diff, work_folder,
};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{self, SessionsReply, SessionsRequest},
    usage::{PlanUsage, SessionSpend},
};
use futures::future::BoxFuture;
use iroh::EndpointId;
use std::{fmt::Display, path::PathBuf, sync::Arc};
use tokio::{
    task::{JoinHandle, spawn_blocking},
    time::{Instant, MissedTickBehavior, interval},
};

impl Agents {
    /// The answer to a transcript read: the last `last` entries of a
    /// session this node runs, or else of a Claude Code session started
    /// outside Beep, read from its file.
    pub(super) async fn transcript_reply(
        &self,
        session: &str,
        last: u32,
        thoughts: bool,
    ) -> SessionsReply {
        let last = usize::try_from(last.min(MAX_TRANSCRIPT)).unwrap_or(usize::MAX);
        let read = match self.get(session) {
            Some(found) => Ok(Some(self.live_transcript(&found, last).await)),
            // A Claude Code session started outside Beep.
            None => match self.external.transcript(session, last).await {
                Ok(Some(read)) => {
                    for picture in read.pictures {
                        self.images.keep(session, picture);
                    }
                    Ok(Some((read.entries, read.earlier)))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(e),
            },
        };
        match read {
            Ok(Some((entries, earlier))) => SessionsReply::Transcript {
                entries: shown_entries(entries, thoughts),
                earlier: u32::try_from(earlier).unwrap_or(u32::MAX),
            },
            Ok(None) => SessionsReply::Error {
                message: format!("there is no session {session}"),
            },
            Err(e) => SessionsReply::Error {
                message: format!("could not read session {session}: {e:#}"),
            },
        }
    }
}

/// The entries a transcript read answers with: the agent's thoughts only
/// when the server asked for them, as one that did not would show them as
/// unknown entries.
fn shown_entries(mut entries: Vec<agent::Entry>, thoughts: bool) -> Vec<agent::Entry> {
    if !thoughts {
        entries.retain(|entry| entry.kind != agent::EntryKind::Thought);
    }
    entries
}

/// Serves one `agents/1` request.
/// `caller` is the key of whoever opened the stream.
pub async fn serve_sessions(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
    caller: EndpointId,
) -> Result<()> {
    let Some(request) = rx.recv::<SessionsRequest>().await? else {
        return Ok(());
    };
    let reply = match request {
        SessionsRequest::List {} => SessionsReply::Sessions {
            sessions: agents.list(),
        },
        SessionsRequest::Transcript {
            session,
            last,
            thoughts,
        } => agents.transcript_reply(&session, last, thoughts).await,
        SessionsRequest::External {
            agent,
            limit,
            session,
        } if agent == "claude" => {
            match agents.external.list(limit, session, agents.owned()).await {
                Ok(sessions) => SessionsReply::External { sessions },
                Err(e) => failed(e),
            }
        }
        SessionsRequest::External { agent, .. } => SessionsReply::Error {
            message: format!("this node lists no {agent} sessions started outside Beep"),
        },
        SessionsRequest::Folders { limit } => match agents.found_folders(limit).await {
            Ok(folders) => SessionsReply::Folders { folders },
            Err(e) => failed(e),
        },
        SessionsRequest::InFolders { folders, limit } => {
            match agents.sessions_in(folders, limit).await {
                Ok(sessions) => SessionsReply::External { sessions },
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Live { session } => live_reply(&agents, &session, None).await,
        SessionsRequest::Steer { session, steer } => {
            live_reply(&agents, &session, Some(steer)).await
        }
        SessionsRequest::Watch { sessions } => {
            return agents.external.watch(tx, rx, sessions).await;
        }
        SessionsRequest::FollowImages {} => return send_images(tx, rx, &agents.images).await,
        SessionsRequest::FollowAlerts {} => {
            return send_alerts(tx, rx, &agents.alerts, caller).await;
        }
        SessionsRequest::FollowUsage { history } => {
            return send_usage(tx, rx, &agents, history).await;
        }
        SessionsRequest::Image { session, key } => match agents.image(&session, &key).await {
            Ok(Some(picture)) if picture.data.len() <= MAX_IMAGE => SessionsReply::Image {
                mime: picture.mime.clone(),
                data: picture.data.clone(),
            },
            Ok(Some(_)) => SessionsReply::Error {
                message: format!("image {key} is too large to send"),
            },
            Ok(None) => SessionsReply::Error {
                message: format!("session {session} has no image {key}"),
            },
            Err(e) => SessionsReply::Error {
                message: format!("could not read session {session}: {e:#}"),
            },
        },
        SessionsRequest::File { path, offset } => {
            match agents.reachable(&path, reach::Access::Read).await {
                Ok(real) => {
                    let read = spawn_blocking(move || {
                        let real = real.to_str().context("the file's path is not UTF-8")?;
                        media_file::read_chunk(real, offset, None)
                    })
                    .await;
                    match read {
                        Ok(Ok(Some(chunk))) => file_reply(chunk),
                        Ok(Ok(None)) => SessionsReply::NoFile {},
                        Ok(Err(e)) => failed(e),
                        Err(e) => SessionsReply::Error {
                            message: format!("the file read stopped: {e}"),
                        },
                    }
                }
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Folder {
            root,
            path,
            ignored,
            depth,
        } => {
            work_request(&agents, &root, reach::Access::Read, move |real| {
                let root = work_folder::WorkRoot::at(&real)?;
                Ok(if depth == 0 {
                    let folder = work_folder::list(&root, &path, ignored)?;
                    SessionsReply::Folder { folder }
                } else {
                    let tree = work_folder::tree(&root, &path, ignored, depth)?;
                    SessionsReply::Tree { tree }
                })
            })
            .await
        }
        SessionsRequest::ReadFile {
            root,
            path,
            offset,
            len,
        } => {
            work_request(&agents, &root, reach::Access::Read, move |real| {
                let root = work_folder::WorkRoot::at(&real)?;
                Ok(match work_folder::read(&root, &path, offset, len)? {
                    Some(chunk) => file_reply(chunk),
                    None => SessionsReply::NoFile {},
                })
            })
            .await
        }
        SessionsRequest::WriteFile {
            root,
            path,
            offset,
            size,
            data,
        } => {
            work_request(&agents, &root, reach::Access::Write, move |real| {
                let root = work_folder::WorkRoot::at(&real)?;
                let path = work_folder::write(&root, &path, offset, size, &data)?;
                Ok(SessionsReply::Written { path })
            })
            .await
        }
        SessionsRequest::CustomAgents { folder } => {
            let home = dirs::home_dir();
            work_request(&agents, &folder, reach::Access::Work, move |real| {
                let agents = custom_agents::list(&real, home.as_deref())?;
                Ok(SessionsReply::CustomAgents { agents })
            })
            .await
        }
        SessionsRequest::Diff { ask } => {
            let root = ask.root.clone();
            work_request(&agents, &root, reach::Access::Diff, move |real| {
                let diff = work_diff::read(&real, &ask)?;
                Ok(SessionsReply::Diff { diff })
            })
            .await
        }
        SessionsRequest::Models { agent } => match agents.models(&agent).await {
            Some(models) => SessionsReply::Models { models },
            None => SessionsReply::Error {
                message: format!("this node has no agent {agent}"),
            },
        },
        SessionsRequest::Harnesses { refresh } => SessionsReply::Harnesses {
            report: agents.harnesses(refresh).await,
        },
        SessionsRequest::UpdateHarness { agent } => {
            match harnesses::update(&agents.kinds, &agents.latest, &agent).await {
                Ok(()) => SessionsReply::Harnesses {
                    report: agents.harnesses(false).await,
                },
                Err(e) => failed(e),
            }
        }
        other => SessionsReply::Error {
            message: format!("this node does not serve {other:?}"),
        },
    };
    tx.send(&reply).await
}

async fn live_reply(agents: &Agents, session: &str, steer: Option<agent::Steer>) -> SessionsReply {
    match agents.external.live(session, steer, agents.owned()).await {
        Ok(status) => SessionsReply::Live {
            status: Box::new(status),
        },
        Err(e) => failed(e),
    }
}

/// The reply saying `e`, with its causes.
fn failed(e: impl Display) -> SessionsReply {
    SessionsReply::Error {
        message: format!("{e:#}"),
    }
}

/// The reply carrying a chunk of a file.
fn file_reply(chunk: media_file::Chunk) -> SessionsReply {
    SessionsReply::File {
        size: chunk.size,
        mime: chunk.mime.to_owned(),
        data: chunk.data,
    }
}

/// Runs `job` on the real folder `path` names, once `access` reaches it,
/// off the async workers; its error becomes the `Error` reply.
async fn work_request(
    agents: &Agents,
    path: &str,
    access: reach::Access,
    job: impl FnOnce(PathBuf) -> Result<SessionsReply> + Send + 'static,
) -> SessionsReply {
    let real = match agents.reachable(path, access).await {
        Ok(real) => real,
        Err(e) => return failed(e),
    };
    match spawn_blocking(move || job(real)).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(e)) => failed(e),
        Err(e) => SessionsReply::Error {
            message: format!("the folder work stopped: {e}"),
        },
    }
}

/// Sends the server each picture a running session newly shows, until it
/// closes the stream. One too large for a frame is left out: the server
/// cannot ask for it either.
async fn send_images(mut tx: LinkTx, mut rx: LinkRx, images: &Images) -> Result<()> {
    loop {
        tokio::select! {
            unsent = images.next_unsent() => {
                if unsent.picture.data.len() > MAX_IMAGE {
                    continue;
                }
                tx.send(&SessionsReply::SessionImage {
                    session: unsent.session,
                    mime: unsent.picture.mime.clone(),
                    data: unsent.picture.data.clone(),
                })
                .await?;
            }
            // The caller sends nothing more; anything is its goodbye.
            _ = rx.recv::<SessionsRequest>() => return Ok(()),
        }
    }
}

/// Sends each alert as it comes, until the caller closes the stream.
async fn send_alerts(
    mut tx: LinkTx,
    mut rx: LinkRx,
    alerts: &AlertHub,
    caller: EndpointId,
) -> Result<()> {
    let follow = alerts.follow(caller);
    loop {
        tokio::select! {
            alert = follow.next() => {
                tx.send(&SessionsReply::Alert { alert }).await?;
                follow.sent();
            }
            // The caller sends nothing more; anything is its goodbye.
            _ = rx.recv::<SessionsRequest>() => return Ok(()),
        }
    }
}

/// Sends the server what finished turns spent and each agent's plan
/// usage, reading the plans every `PLAN_EVERY` and when the server asks,
/// until the server closes the stream. With `history`, also what sessions
/// spent as the agents' logs tell it, read every `SPEND_EVERY`.
async fn send_usage(mut tx: LinkTx, mut rx: LinkRx, agents: &Agents, history: bool) -> Result<()> {
    let mut plans = agents.usage.plans();
    let mut sent = Plans::new();
    let mut every = interval(PLAN_EVERY);
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // A read runs a CLI or two; it runs beside the loop, not in it.
    let mut reading: Option<BoxFuture<'_, Vec<PlanUsage>>> = None;
    let mut last_read: Option<Instant> = None;
    let mut every_scan = interval(SPEND_EVERY);
    every_scan.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // A pass reads files; it runs on the blocking pool beside the loop.
    // Dropped with the stream, it still finishes and saves what it read.
    let mut scanning: Option<JoinHandle<Vec<SessionSpend>>> = None;
    loop {
        tokio::select! {
            turn = agents.usage.next_turn() => {
                tx.send(&SessionsReply::TurnUsage { usage: turn }).await?;
            }
            Ok(()) = plans.changed() => {
                let now = plans.borrow_and_update().clone();
                for plan in now.values().filter(|plan| sent.get(&plan.agent) != Some(plan)) {
                    tx.send(&SessionsReply::PlanUsage { usage: plan.clone() }).await?;
                }
                sent = now;
            }
            _ = every.tick(), if reading.is_none() => {
                reading = Some(Box::pin(agents.read_plans()));
            }
            Some(read) = async { Some(reading.as_mut()?.await) }, if reading.is_some() => {
                reading = None;
                last_read = Some(Instant::now());
                for plan in read {
                    agents.usage.set_plan(plan);
                }
            }
            _ = every_scan.tick(), if history && scanning.is_none() => {
                let spend = agents.spend.clone();
                scanning = Some(spawn_blocking(move || spend.pass(unix_now())));
            }
            Some(read) = async { Some(scanning.as_mut()?.await) }, if scanning.is_some() => {
                scanning = None;
                match read {
                    Ok(reports) => {
                        for report in reports {
                            tx.send(&SessionsReply::SessionSpend { usage: report.clone() }).await?;
                            agents.spend.sent(&report);
                        }
                    }
                    Err(e) => log::warn!("reading the agents' logs failed: {e}"),
                }
            }
            message = rx.recv::<SessionsRequest>() => match message {
                Ok(Some(SessionsRequest::RefreshPlanUsage {})) => {
                    let recent = last_read.is_some_and(|at| at.elapsed() < PLAN_REFRESH_GAP);
                    if reading.is_none() && !recent {
                        reading = Some(Box::pin(agents.read_plans()));
                    }
                }
                // Anything else is the server's goodbye.
                _ => return Ok(()),
            },
        }
    }
}
