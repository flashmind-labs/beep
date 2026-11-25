//! The `agents/1` requests: one answer each, or a stream the caller follows.

use beep_proto::SessionId;
use super::{
    Agents, AlertHub, MAX_IMAGE, MAX_TRANSCRIPT, PLAN_EVERY, PLAN_REFRESH_GAP,
    custom_agents, harnesses, media_file, reach, spend::Reports, work_folder, worktrees,
};
use crate::files::transfer::Transfers;
use anyhow::{Context, Result};
use beep_mesh::machine::thumb::{Picture, thumb_of};
use beep_proto::media_mime;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{self, SessionsReply, SessionsRequest},
};
use beep_proto::watched;
use iroh::EndpointId;
use std::{fmt::Display, io::ErrorKind, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    task::spawn_blocking,
    time::{Instant, MissedTickBehavior, interval, timeout},
};
use tokio_util::task::AbortOnDropHandle;

/// How long a worktree list may take, the path check and git together.
const WORKTREES_WAIT: Duration = Duration::from_secs(8);

/// Read-only session pages for coding agents on other mesh machines.
pub(crate) const TRANSCRIPT_PROTOCOL: &str = "agent-transcripts/1";

/// This endpoint deliberately accepts only transcript reads, never agent controls.
pub(crate) async fn serve_transcripts(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
) -> Result<()> {
    let reply = match rx.recv::<SessionsRequest>().await? {
        Some(SessionsRequest::Transcript {
            session,
            last,
            before,
            ..
        }) if !session.as_ref().trim().is_empty() && session.as_ref().len() <= 512 && (1..=50).contains(&last) => {
            agents.transcript_reply(&session, last, false, before).await
        }
        _ => SessionsReply::Error {
            message: "Only bounded transcript reads are accepted.".into(),
        },
    };
    tx.send(&reply).await
}

impl Agents {
    /// How each move to this node stands, as the `move-progress` topic pushes it.
    pub(crate) fn move_entries(&self) -> Vec<beep_proto::node::agent::MoveEntry> {
        self.moves.entries()
    }

    /// The answer to a transcript read: the last `last` entries of a
    /// session this node runs, or else of a Claude Code session started
    /// outside Beep, read from its file.
    pub(super) async fn transcript_reply(
        &self,
        session: &SessionId,
        last: u32,
        thoughts: bool,
        before: Option<u64>,
    ) -> SessionsReply {
        let read = self.read_transcript(session, last, before, true).await;

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

    /// The last `last` entries of a session before entry `before`, with how
    /// many come earlier: `None` for a session this node does not have.
    /// Search indexing reads without keeping the prompts' pictures.
    pub(crate) async fn read_transcript(
        &self,
        session: &SessionId,
        last: u32,
        before: Option<u64>,
        keep_pictures: bool,
    ) -> anyhow::Result<Option<(Vec<agent::Entry>, u64)>> {
        let last = usize::try_from(last.min(MAX_TRANSCRIPT)).unwrap_or(usize::MAX);
        self.transcript_window(session, last, before, None, keep_pictures)
            .await
    }
}

/// The entries a transcript read answers with: the agent's thoughts only
/// when the caller asked for them, as one that did not would show them as
/// unknown entries.
fn shown_entries(mut entries: Vec<agent::Entry>, thoughts: bool) -> Vec<agent::Entry> {
    if !thoughts {
        entries.retain(|entry| entry.kind != agent::EntryKind::Thought);
    }
    entries
}

/// How long an `agents/1` answer may take before the node logs it as slow.
const SLOW_ANSWER: Duration = Duration::from_secs(20);

/// Logs it when the answer to `request` from `caller` takes over
/// `SLOW_ANSWER`, unless dropped first. A request that streams is not
/// watched (#428).
fn watch_slow(request: &SessionsRequest, caller: EndpointId) -> Option<AbortOnDropHandle<()>> {
    let limit = match request {
        SessionsRequest::List {}
        | SessionsRequest::ListAll {}
        | SessionsRequest::Transcript { .. } => Duration::from_secs(2),
        _ => SLOW_ANSWER,
    };
    let what = match request {
        SessionsRequest::Watch { .. }
        | SessionsRequest::FollowPresence {}
        | SessionsRequest::FollowAlerts {}
        | SessionsRequest::FollowRuns {}
        | SessionsRequest::FollowIndex { .. }
        | SessionsRequest::FollowUsage { .. } => return None,
        // Neither carries its images or bytes into the log.
        SessionsRequest::Steer { session, .. } => format!("Steer for session {session}"),
        SessionsRequest::WriteFile { path, .. } | SessionsRequest::UploadFile { path, .. } => {
            format!("WriteFile {path}")
        }
        SessionsRequest::LspDefinition {
            path, line, column, ..
        } => format!("LspDefinition {path}:{line}:{column}"),
        other => format!("{other:?}").chars().take(300).collect(),
    };
    Some(warn_if_slow_after(
        format!("agents/1 {what} from {}", caller.fmt_short()),
        limit,
    ))
}

/// Logs `what` as unanswered once `SLOW_ANSWER` passes, unless dropped first.
pub(super) fn warn_if_slow(what: String) -> AbortOnDropHandle<()> {
    warn_if_slow_after(what, SLOW_ANSWER)
}

fn warn_if_slow_after(what: String, limit: Duration) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(tokio::spawn(async move {
        tokio::time::sleep(limit).await;
        log::warn!("no answer after {} s to {what}", limit.as_secs());
    }))
}

/// Serves one `agents/1` request.
/// `caller` is the key of whoever opened the stream. `Share` needs the
/// node's `transfers`.
pub async fn serve_sessions(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: Arc<Agents>,
    caller: EndpointId,
    transfers: Option<Transfers>,
) -> Result<()> {
    let Some(request) = rx.recv::<SessionsRequest>().await? else {
        return Ok(());
    };
    let mut trace = beep_proto::logging::RequestTrace::new(<&'static str>::from(&request));
    trace.phase("handling");
    let operation = match &request {
        SessionsRequest::List {} | SessionsRequest::ListAll {} => Some("list"),
        SessionsRequest::Transcript { .. } => Some("transcript"),
        _ => None,
    };
    let started = std::time::Instant::now();
    let _slow = watch_slow(&request, caller);
    let mut reply = match request {
        SessionsRequest::List {} => SessionsReply::Sessions {
            sessions: agents.listed_with_places(false).await,
        },
        SessionsRequest::ListAll {} => SessionsReply::Sessions {
            sessions: agents.listed_with_places(true).await,
        },
        SessionsRequest::Transcript {
            session,
            last,
            thoughts,
            before,
        } => {
            agents
                .transcript_reply(&session, last, thoughts, before)
                .await
        }
        SessionsRequest::External {
            agent,
            limit,
            session,
        } => agents
            .external_of(&agent, limit, session)
            .await
            .map_or_else(failed, |sessions| SessionsReply::External { sessions }),
        SessionsRequest::ExternalNamed { sessions } => SessionsReply::External {
            sessions: agents.external_named(sessions).await,
        },
        SessionsRequest::Folders { limit } => agents
            .found_folders(limit)
            .await
            .map_or_else(failed, |folders| SessionsReply::Folders { folders }),
        SessionsRequest::InFolders { folders, limit } => agents
            .sessions_in(folders, limit)
            .await
            .map_or_else(failed, |sessions| SessionsReply::External { sessions }),
        SessionsRequest::InFoldersPage {
            folders,
            limit,
            before,
        } => agents
            .sessions_page(folders, limit, before)
            .await
            .map_or_else(failed, |page| SessionsReply::SessionPage { page }),
        SessionsRequest::Live { session } => live_reply(&agents, &session, None).await,
        SessionsRequest::Steer { session, steer } => {
            live_reply(&agents, &session, Some(steer)).await
        }
        SessionsRequest::Watch { sessions } => {
            return trace.stream(agents.external.watch(tx, rx, sessions)).await;
        }
        SessionsRequest::FollowImages {} => failed("this node does not send pictures"),
        SessionsRequest::FollowPresence {} => {
            return trace.stream(super::presence::follow(tx, rx, &agents)).await;
        }
        SessionsRequest::FollowAlerts {} => {
            return trace
                .stream(send_alerts(tx, rx, &agents.alerts, caller, |alert| {
                    SessionsReply::Alert { alert }
                }))
                .await;
        }
        SessionsRequest::FollowRuns {} => {
            return trace
                .stream(send_alerts(tx, rx, &agents.ci.runs, caller, |news| {
                    SessionsReply::Run { news }
                }))
                .await;
        }
        SessionsRequest::FollowUsage { history } => {
            return trace
                .stream(send_usage(tx, rx, &agents, history, caller))
                .await;
        }
        SessionsRequest::FollowIndex { epoch, after } => {
            return trace
                .stream(send_index(tx, rx, &agents, &epoch, after))
                .await;
        }
        SessionsRequest::Search { query, sessions } => match agents.search.get().cloned() {
            Some(index) => index
                .find(query, sessions)
                .await
                .map_or_else(failed, |hits| SessionsReply::SearchHits { hits }),
            None => failed("this node's search index is not open yet"),
        },
        SessionsRequest::Image { session, key } => image_reply(&agents, &session, &key).await,
        SessionsRequest::Thumb { session, key } => match image_reply(&agents, &session, &key).await
        {
            SessionsReply::Image { mime, data } => {
                let small = thumb_of(Picture { mime, data }).await;
                SessionsReply::Image {
                    mime: small.mime,
                    data: small.data,
                }
            }
            other => other,
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
        SessionsRequest::Share { path } => share_reply(&agents, transfers, caller, &path).await,
        SessionsRequest::Folder {
            root,
            path,
            ignored,
            depth,
        } => {
            folder_reply(&agents, &root, path, ignored, depth).await
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
        SessionsRequest::UploadFile {
            root,
            agent,
            session,
            path,
            offset,
            size,
            data,
        } => {
            let running = agents.get(&session);
            let draft = root.is_empty() && super::uploads::is_draft(&agent, &session);
            let write = move || {
                let path = super::uploads::write(
                    &agent,
                    &session,
                    &path,
                    offset,
                    size,
                    &data,
                    running.as_deref(),
                )?;
                Ok(SessionsReply::Written { path })
            };

            if draft {
                file_job(write).await
            } else {
                work_request(&agents, &root, reach::Access::Write, move |_| write()).await
            }
        }
        SessionsRequest::DiscardDraftAttachment { draft, path } => {
            file_job(move || {
                super::uploads::discard_draft(&draft, &path)?;
                Ok(SessionsReply::FilesEdited {})
            })
            .await
        }
        SessionsRequest::EditFiles { root, action } => {
            work_request(&agents, &root, reach::Access::Write, move |real| {
                let root = work_folder::WorkRoot::at(&real)?;
                work_folder::edit(&root, &action)?;
                Ok(SessionsReply::FilesEdited {})
            })
            .await
        }
        SessionsRequest::LspDefinition {
            root,
            path,
            line,
            column,
            text,
        } => {
            let lookup = async {
                let real = agents.reachable(&root, reach::Access::Work).await?;
                agents.lsp.definition(&real, &path, line, column, text).await
            };

            match lookup.await {
                Ok(result) => SessionsReply::LspLocations { result },
                Err(e) => failed(e),
            }
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
            let read = async {
                let real = agents.reachable(&ask.root, reach::Access::Diff).await?;
                agents.checkouts.diff(real, ask).await
            };

            match read.await {
                Ok(diff) => SessionsReply::Diff {
                    diff: Arc::unwrap_or_clone(diff),
                },
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Worktrees { folder, session } => {
            let created = session
                .map(|id| agents.created_worktrees(&id))
                .unwrap_or_default();
            let list = work_request(&agents, &folder, reach::Access::Work, move |real| {
                let worktrees = worktrees::list(&real, &created)?;
                Ok(SessionsReply::Worktrees { worktrees })
            });
            timeout(WORKTREES_WAIT, list)
                .await
                .unwrap_or_else(|_| SessionsReply::Error {
                    message: "git took too long to list the worktrees".into(),
                })
        }
        // A session that is not running offers nothing.
        SessionsRequest::Commands { session } => SessionsReply::Commands {
            commands: agents
                .get(&session)
                .map(|found| found.commands())
                .unwrap_or_default(),
        },
        SessionsRequest::Processes { session } => match agents.get(&session) {
            Some(found) if found.info().state.status != agent::Status::Ended => found
                .processes
                .list()
                .await
                .map_or_else(failed, |running| SessionsReply::Processes {
                    processes: running.processes,
                    containers: running.containers,
                }),
            _ => SessionsReply::Processes {
                processes: Vec::new(),
                containers: Vec::new(),
            },
        },
        SessionsRequest::StopContainer { session, id } => match agents.get(&session) {
            Some(found) if found.info().state.status != agent::Status::Ended => found
                .processes
                .stop_container(&id)
                .await
                .map_or_else(failed, |()| SessionsReply::ProcessStopped {}),
            _ => failed("the session has ended"),
        },
        SessionsRequest::StopProcess { session, pid } => match agents.get(&session) {
            Some(found) if found.info().state.status != agent::Status::Ended => found
                .processes
                .stop(pid)
                .await
                .map_or_else(failed, |()| SessionsReply::ProcessStopped {}),
            _ => failed("the session has ended"),
        },
        SessionsRequest::CloseExternal { agent, session } => agents
            .close_external(&agent, &session)
            .await
            .map_or_else(failed, |()| SessionsReply::ProcessStopped {}),
        SessionsRequest::RelocateSession {
            agent,
            session,
            folder,
        } => agents
            .relocate_session(&agent, &session, &folder)
            .await
            .map_or_else(failed, |()| SessionsReply::Deleted {}),
        SessionsRequest::DeleteSession { agent, session } => agents
            .delete_session(&agent, &session)
            .await
            .map_or_else(failed, |()| SessionsReply::Deleted {}),
        SessionsRequest::Models { agent } => match agents.models(&agent).await {
            Some(models) => SessionsReply::Models { models },
            None => SessionsReply::Error {
                message: format!("this node has no agent {agent}"),
            },
        },
        SessionsRequest::MoveProgress { session } => SessionsReply::Moving {
            progress: agents.moves.progress(&session),
        },
        // Asked by apps from before worktree moves were removed, before they
        // send one: this node takes it as a move to the project folder.
        SessionsRequest::MoveCheck { .. } => SessionsReply::MoveChecked { ignored: false },
        // Asked by apps from before background copies were removed: this
        // node keeps no copies.
        SessionsRequest::Copies {} => SessionsReply::Copies { copies: Vec::new() },
        SessionsRequest::ContinueCopy { .. } => SessionsReply::Error {
            message: "this machine no longer keeps copies of other machines' sessions".into(),
        },
        SessionsRequest::DiscardCopy { .. } => SessionsReply::CopyDone {},
        SessionsRequest::Harnesses { refresh } => SessionsReply::Harnesses {
            report: agents.harnesses(refresh).await,
        },
        SessionsRequest::UpdateHarness { agent } => {
            match harnesses::update(&agents.kinds(), &agents.latest, &agent).await {
                Ok(()) => SessionsReply::Harnesses {
                    report: agents.harnesses(false).await,
                },
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Present { mut paths } => {
            paths.truncate(agent::MAX_PRESENT_PATHS);

            spawn_blocking(move || media_file::present(&paths, dirs::home_dir().as_deref()))
                .await
                .map_or_else(failed, |present| SessionsReply::Present { present })
        }
        SessionsRequest::HashFiles { root, mut paths } => {
            paths.truncate(agent::MAX_HASH_PATHS);

            work_request(&agents, &root, reach::Access::Read, move |real| {
                let root = work_folder::WorkRoot::at(&real)?;
                let hashes = work_folder::hashes(&root, &paths);
                Ok(SessionsReply::Hashes {
                    hashes: hashes
                        .into_iter()
                        .map(|hash| hash.map(serde_bytes::ByteArray::new))
                        .collect(),
                })
            })
            .await
        }
        SessionsRequest::StartReview { order } => match agents.start_review(order).await {
            Ok(run) => SessionsReply::Review { run },
            Err(message) => SessionsReply::Error { message },
        },
        SessionsRequest::Review { id } => match agents.review(&id) {
            Some(run) => SessionsReply::Review { run },
            None => SessionsReply::Error {
                message: format!("there is no review {id}"),
            },
        },
        SessionsRequest::Reviews { session } => SessionsReply::Reviews {
            runs: agents.reviews_of(&session),
        },
        SessionsRequest::InstallAdapters {} => {
            agents.install_adapters_now();
            SessionsReply::AdaptersInstalling {}
        }
        other => SessionsReply::Error {
            message: format!("this node does not serve {other:?}"),
        },
    };

    let metadata = match &mut reply {
        SessionsReply::External { sessions } => Some(sessions),
        SessionsReply::SessionPage { page } => Some(&mut page.sessions),
        _ => None,
    };
    if let Some(sessions) = metadata {
        agents.settle_closed(sessions);
        agents.observe_external(sessions);
    }

    let computed = started.elapsed();
    trace.phase("sending");
    let result = tx.send(&reply).await;
    let outcome = end_trace(&mut trace, &result, &reply);

    if let Some(operation) = operation {
        let elapsed = started.elapsed();
        metrics::counter!("beep_agent_requests_total", "operation" => operation, "outcome" => outcome).increment(1);
        metrics::histogram!("beep_agent_request_seconds", "operation" => operation)
            .record(elapsed.as_secs_f64());
        if elapsed >= Duration::from_secs(1) {
            log::warn!(
                "slow agents/1 {operation} from {}: total={}ms handling={}ms send={}ms outcome={outcome}",
                caller.fmt_short(),
                elapsed.as_millis(),
                computed.as_millis(),
                elapsed.saturating_sub(computed).as_millis()
            );
        }
    }

    result
}

/// Records how the request ended and returns its metric label: `error` when
/// the answer could not be sent, `refused` when the handler answered an error.
fn end_trace(
    trace: &mut beep_proto::logging::RequestTrace,
    sent: &Result<()>,
    reply: &SessionsReply,
) -> &'static str {
    match (sent, reply) {
        (Err(e), _) => {
            trace.fail(format!("{e:#}"));
            "error"
        }
        (Ok(()), SessionsReply::Error { message }) => {
            trace.refuse(message);
            "refused"
        }
        (Ok(()), _) => {
            trace.finish(true);
            "ok"
        }
    }
}

async fn live_reply(agents: &Agents, session: &SessionId, steer: Option<agent::Steer>) -> SessionsReply {
    agents
        .external
        .live(session, steer, agents.owned())
        .await
        .map_or_else(failed, |status| SessionsReply::Live {
            status: Box::new(status),
        })
}

/// The reply saying `e`, with its causes.
fn failed(e: impl Display) -> SessionsReply {
    SessionsReply::Error {
        message: format!("{e:#}"),
    }
}

/// Lets `caller` fetch the media file `path` from the node's blob store.
async fn share_reply(
    agents: &Agents,
    transfers: Option<Transfers>,
    caller: EndpointId,
    path: &str,
) -> SessionsReply {
    let Some(transfers) = transfers else {
        return failed("this machine's file transfers did not start");
    };

    if media_mime(path).is_none() {
        return failed(format!(
            "{path} is not a picture, video, audio file, 3D model or PDF"
        ));
    }

    let real = match agents.reachable(path, reach::Access::Read).await {
        Ok(real) => real,
        Err(e) => return failed(e),
    };

    match tokio::fs::metadata(&real).await {
        Err(e) if e.kind() == ErrorKind::NotFound => return SessionsReply::NoFile {},
        Err(e) => return failed(format!("could not read {path}: {e}")),
        Ok(meta) if !meta.is_file() => return failed(format!("{path} is not a file")),
        Ok(_) => {}
    }

    match transfers.share(&real, caller).await {
        Ok(shared) => SessionsReply::Shared {
            hash: shared.hash,
            size: shared.size,
        },
        Err(e) => failed(e),
    }
}

/// The folder `path` in `root` as a `Folder` request asks for it. Its git
/// marks come from the checkout's kept status when the checkout has one.
async fn folder_reply(
    agents: &Agents,
    root: &str,
    path: String,
    ignored: bool,
    depth: u32,
) -> SessionsReply {
    let real = match agents.reachable(root, reach::Access::Read).await {
        Ok(real) => real,
        Err(e) => return failed(e),
    };
    let checkouts = agents.checkouts.clone();
    let runtime = tokio::runtime::Handle::current();

    file_job(move || {
        let root = work_folder::WorkRoot::at(&real)?;
        // A path that is no folder is refused before any status is asked.
        work_folder::open_folder(&root, &path)?;
        let kept = runtime.block_on(checkouts.status(real));
        let kept = kept.as_deref();

        Ok(if depth == 0 {
            let folder = work_folder::list(&root, &path, ignored, kept)?;
            SessionsReply::Folder { folder }
        } else {
            let tree = work_folder::tree(&root, &path, ignored, depth, kept)?;
            SessionsReply::Tree { tree }
        })
    })
    .await
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

    file_job(move || job(real)).await
}

async fn file_job(
    job: impl FnOnce() -> Result<SessionsReply> + Send + 'static,
) -> SessionsReply {
    match spawn_blocking(job).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(e)) => failed(e),
        Err(e) => SessionsReply::Error {
            message: format!("the folder work stopped: {e}"),
        },
    }
}

/// Sends each alert as it comes, until the caller closes the stream.
async fn send_alerts<T: Clone>(
    mut tx: LinkTx,
    mut rx: LinkRx,
    alerts: &AlertHub<T>,
    caller: EndpointId,
    reply: impl Fn(T) -> SessionsReply,
) -> Result<()> {
    let follow = alerts.follow(caller);

    loop {
        tokio::select! {
            alert = follow.next() => {
                tx.send(&reply(alert)).await?;
                follow.sent();
            }
            // The caller sends nothing more; anything is its goodbye.
            _ = rx.recv::<SessionsRequest>() => return Ok(()),
        }
    }
}

/// Sends the caller what finished turns spent and each agent's plan
/// usage, reading the plans every `PLAN_EVERY` and when the caller asks,
/// until the caller closes the stream. With `history`, also what sessions
/// spent as the agents' logs tell it, read for all such streams together.
async fn send_usage(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: &Arc<Agents>,
    history: bool,
    caller: EndpointId,
) -> Result<()> {
    let turns = agents.usage.turns.follow(caller);
    let mut plans = agents.usage.plans();
    // What earlier reads left goes out at once: a desktop opens a stream per
    // look and listens only briefly, so it would miss a CLI that starts slowly.
    let mut sent = watched::read_and_update(&mut plans, Clone::clone);

    for plan in sent.values() {
        tx.send(&SessionsReply::PlanUsage {
            usage: plan.clone(),
        })
        .await?;
    }

    let mut every = interval(PLAN_EVERY);
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // When this stream last asked for a read, which runs beside it.
    let mut last_read: Option<Instant> = None;
    // The node's one scan of the logs runs while this stream holds it. The
    // reports so far go out at once, the later ones as the scan changes them.
    let (_scan, mut spend) = match history {
        true => {
            let (scan, mut spend) = agents.spend.follow(agents.log_changes().await?);
            spend.mark_changed();
            (Some(scan), Some(spend))
        }
        false => (None, None),
    };
    let mut told = Reports::new();

    loop {
        tokio::select! {
            turn = turns.next() => {
                tx.send(&SessionsReply::TurnUsage { usage: turn }).await?;
                turns.sent();
            }
            Ok(()) = plans.changed() => {
                let now = watched::read_and_update(&mut plans, Clone::clone);

                for (_, plan) in now.iter().filter(|(of, plan)| sent.get(*of) != Some(*plan)) {
                    tx.send(&SessionsReply::PlanUsage { usage: plan.clone() }).await?;
                }
                sent = now;
            }
            _ = every.tick() => {
                agents.read_plans_soon();
                last_read = Some(Instant::now());
            }
            Some(now) = async {
                match spend.as_mut() {
                    Some(spend) => {
                        spend.changed().await.ok()?;
                        Some(watched::read_and_update(spend, Clone::clone))
                    }
                    None => std::future::pending().await,
                }
            } => {
                for (_, report) in now.iter().filter(|(of, report)| told.get(*of) != Some(*report)) {
                    tx.send(&SessionsReply::SessionSpend { usage: report.clone() }).await?;
                }
                told = now;
            }
            message = rx.recv::<SessionsRequest>() => match message {
                Ok(Some(SessionsRequest::RefreshPlanUsage {})) => {
                    let recent = last_read.is_some_and(|at| at.elapsed() < PLAN_REFRESH_GAP);

                    if !recent {
                        agents.read_plans_soon();
                        last_read = Some(Instant::now());
                    }
                }
                // Anything else is the caller's goodbye.
                _ => return Ok(()),
            },
        }
    }
}

/// The most rows one `IndexRows` batch carries.
const INDEX_BATCH: u32 = 500;

/// Sends the search index's rows after `after` (all of them when `epoch`
/// is not this index's), then each new row as it is added.
async fn send_index(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: &Arc<Agents>,
    epoch: &str,
    mut after: u64,
) -> Result<()> {
    let Some(index) = agents.search.get().cloned() else {
        return tx
            .send(&failed("this node's search index is not open yet"))
            .await;
    };

    if epoch != index.epoch() {
        after = 0;
    }

    let mut newest = index.newest();

    loop {
        // Marked seen before reading, so a row added meanwhile wakes the
        // next round.
        let top = beep_proto::watched::read_and_update(&mut newest, |seq| *seq);

        loop {
            let rows = index.rows_after(after, INDEX_BATCH).await?;
            after = rows.last().map_or(after, |row| row.seq);
            let current = rows.is_empty() || after >= top;
            tx.send(&SessionsReply::IndexRows {
                epoch: index.epoch().to_owned(),
                rows,
                current,
            })
            .await?;

            if current {
                break;
            }
        }

        tokio::select! {
            changed = newest.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
            // Anything from the caller is its goodbye.
            _ = rx.recv::<SessionsRequest>() => return Ok(()),
        }
    }
}

/// The picture `key` of `session` as `Image` answers it.
async fn image_reply(agents: &Agents, session: &SessionId, key: &str) -> SessionsReply {
    match agents.image(session, key).await {
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
    }
}

#[cfg(test)]
#[path = "../tests/agents_requests.rs"]
mod tests;
