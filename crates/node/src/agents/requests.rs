//! The `agents/1` requests: one answer each, or a stream the server follows.

use super::{
    Agents, AlertHub, Images, MAX_IMAGE, MAX_TRANSCRIPT, PLAN_EVERY, PLAN_REFRESH_GAP, SPEND_EVERY,
    custom_agents, harnesses, media_file, reach, unix_now, work_diff, work_folder, worktrees,
};
use crate::agents::claude::Excerpt;
use anyhow::{Context, Result};
use beep_mesh::machine::thumb::{Picture, thumb_of};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{self, SessionsReply, SessionsRequest},
    usage::SessionSpend,
};
use iroh::EndpointId;
use std::{fmt::Display, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    task::{JoinHandle, spawn_blocking},
    time::{Instant, MissedTickBehavior, interval, timeout},
};

/// How long a worktree list may take, the path check and git together.
const WORKTREES_WAIT: Duration = Duration::from_secs(8);

impl Agents {
    /// The answer to a transcript read: the last `last` entries of a
    /// session this node runs, or else of a Claude Code session started
    /// outside Beep, read from its file.
    pub(super) async fn transcript_reply(
        &self,
        session: &str,
        last: u32,
        thoughts: bool,
        before: Option<u64>,
    ) -> SessionsReply {
        let last = usize::try_from(last.min(MAX_TRANSCRIPT)).unwrap_or(usize::MAX);
        let read = match self.get(session) {
            Some(found) => Ok(Some(self.live_transcript(&found, last, before).await)),
            // A session started outside Beep: Claude Code's, else another
            // agent's from its own store.
            None => match self.external.transcript(session, last, before).await {
                Ok(None) => self.stores_transcript(session, last, before).await,
                other => other,
            }
            .map(|read| {
                read.map(|read| {
                    for picture in read.pictures {
                        self.images.keep(session, picture);
                    }
                    (read.entries, read.earlier)
                })
            }),
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

impl Agents {
    /// The transcript of a session in Codex's or pi's files, or why another
    /// agent's cannot be read.
    async fn stores_transcript(
        &self,
        session: &str,
        last: usize,
        before: Option<u64>,
    ) -> Result<Option<Excerpt>> {
        let stores = self.stores.clone();
        let id = session.to_owned();
        spawn_blocking(move || stores.transcript(&id, last, before)).await?
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
        } if agent == "claude" => {
            match agents.external.list(limit, session, agents.owned()).await {
                Ok(sessions) => SessionsReply::External { sessions },
                Err(e) => failed(e),
            }
        }
        SessionsRequest::External {
            agent,
            limit,
            session,
        } => match agents.stores_external(&agent, limit, session).await {
            Ok(sessions) => SessionsReply::External { sessions },
            Err(e) => failed(e),
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
            return send_alerts(tx, rx, &agents.alerts, caller, |alert| {
                SessionsReply::Alert { alert }
            })
            .await;
        }
        SessionsRequest::FollowRuns {} => {
            return send_alerts(tx, rx, &agents.ci.runs, caller, |news| SessionsReply::Run {
                news,
            })
            .await;
        }
        SessionsRequest::FollowUsage { history } => {
            return send_usage(tx, rx, &agents, history).await;
        }
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
        SessionsRequest::Worktrees { folder } => {
            let list = work_request(&agents, &folder, reach::Access::Work, move |real| {
                let worktrees = worktrees::list(&real)?;
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
        SessionsRequest::DeleteSession { agent, session } => {
            match agents.delete_session(&agent, &session).await {
                Ok(()) => SessionsReply::Deleted {},
                Err(e) => failed(e),
            }
        }
        SessionsRequest::Models { agent } => match agents.models(&agent).await {
            Some(models) => SessionsReply::Models { models },
            None => SessionsReply::Error {
                message: format!("this node has no agent {agent}"),
            },
        },
        SessionsRequest::MoveProgress { session } => SessionsReply::Moving {
            progress: agents.moves.progress(&session),
        },
        SessionsRequest::MoveCheck { project, branch } => SessionsReply::MoveChecked {
            ignored: agents.moves.ignores_worktree(&project, &branch).await,
        },
        SessionsRequest::Copies {} => SessionsReply::Copies {
            copies: agents.moves.copies().await,
        },
        SessionsRequest::ContinueCopy {
            agent,
            session,
            lost,
        } => match agents.moves.continue_copy(&agent, &session, lost).await {
            Ok(()) => SessionsReply::CopyDone {},
            Err(e) => failed(e),
        },
        SessionsRequest::DiscardCopy {
            agent,
            session,
            lost,
        } => {
            agents.moves.discard_copy(&agent, &session, lost).await;
            SessionsReply::CopyDone {}
        }
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

/// Sends the server what finished turns spent and each agent's plan
/// usage, reading the plans every `PLAN_EVERY` and when the server asks,
/// until the server closes the stream. With `history`, also what sessions
/// spent as the agents' logs tell it, read every `SPEND_EVERY`.
async fn send_usage(
    mut tx: LinkTx,
    mut rx: LinkRx,
    agents: &Arc<Agents>,
    history: bool,
) -> Result<()> {
    let mut plans = agents.usage.plans();
    // What earlier reads left goes out at once: a desktop opens a stream per
    // look and listens only briefly, so it would miss a CLI that starts slowly.
    let mut sent = plans.borrow_and_update().clone();
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
    let mut every_scan = interval(SPEND_EVERY);
    every_scan.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // A pass reads files; it runs on the blocking pool beside the loop.
    // Dropped with the stream, it still finishes and saves what it read.
    let mut scanning: Option<JoinHandle<Vec<SessionSpend>>> = None;
    // The first pass gives this stream every session: a desktop keeps nothing
    // between reads, so sessions only one earlier stream heard would go missing.
    let mut first_pass = true;
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
            _ = every.tick() => {
                agents.read_plans_soon();
                last_read = Some(Instant::now());
            }
            _ = every_scan.tick(), if history && scanning.is_none() => {
                let spend = agents.spend.clone();
                let all = first_pass;
                scanning = Some(spawn_blocking(move || {
                    if all { spend.pass_all(unix_now()) } else { spend.pass(unix_now()) }
                }));
            }
            Some(read) = async { Some(scanning.as_mut()?.await) }, if scanning.is_some() => {
                scanning = None;
                match read {
                    Ok(reports) => {
                        for report in reports {
                            tx.send(&SessionsReply::SessionSpend { usage: report.clone() }).await?;
                            if !first_pass {
                                agents.spend.sent(&report);
                            }
                        }
                        first_pass = false;
                    }
                    Err(e) => log::warn!("reading the agents' logs failed: {e}"),
                }
            }
            message = rx.recv::<SessionsRequest>() => match message {
                Ok(Some(SessionsRequest::RefreshPlanUsage {})) => {
                    let recent = last_read.is_some_and(|at| at.elapsed() < PLAN_REFRESH_GAP);
                    if !recent {
                        agents.read_plans_soon();
                        last_read = Some(Instant::now());
                    }
                }
                // Anything else is the server's goodbye.
                _ => return Ok(()),
            },
        }
    }
}

/// The picture `key` of `session` as `Image` answers it.
async fn image_reply(agents: &Agents, session: &str, key: &str) -> SessionsReply {
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
mod tests {
    use super::*;
    use crate::agents::{claude::External, found::Stores};
    use beep_proto::node::agent::EntryKind;
    use serde_json::json;
    use std::{fs, path::Path};

    const CODEX_ID: &str = "01a199e3-03f2-7820-a29e-067ae8c4ec85";
    const PI_ID: &str = "pi-session-1";

    fn lines(values: &[serde_json::Value]) -> String {
        values.iter().map(|line| format!("{line}\n")).collect()
    }

    /// A fabricated Codex rollout: a prompt, reasoning, a call and its
    /// output, an answer, then a second prompt and answer.
    fn codex_rollout() -> String {
        let user = |text: &str| json!({"type": "event_msg", "payload": {"type": "user_message", "message": text}});
        let said = |text: &str| {
            json!({"type": "response_item", "payload": {"type": "message",
            "role": "assistant", "content": [{"type": "output_text", "text": text}]}})
        };
        lines(&[
            json!({"type": "session_meta", "payload": {"id": CODEX_ID, "cwd": "/srv/project",
                "timestamp": "2026-11-02T01:32:34.000Z", "source": "cli"}}),
            json!({"type": "response_item", "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "<environment_context>"}]}}),
            user("fix the retry flag"),
            json!({"type": "response_item", "payload": {"type": "reasoning",
                "summary": [{"type": "summary_text", "text": "Looking at the flag"}]}}),
            json!({"type": "response_item", "payload": {"type": "function_call", "name": "exec_command",
                "arguments": "{\"cmd\":\"cargo test\"}", "call_id": "c1"}}),
            json!({"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1", "output": "ok"}}),
            said("The flag is fixed."),
            user("now the docs"),
            said("Docs updated."),
        ])
    }

    fn pi_session() -> String {
        lines(&[
            json!({"type": "session", "id": PI_ID, "cwd": "/srv/project", "timestamp": "2026-11-02T01:32:34.000Z"}),
            json!({"type": "message", "message": {"role": "user", "content": [{"type": "text", "text": "list the files"}]}}),
            json!({"type": "message", "message": {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "I will run ls"},
                {"type": "toolCall", "id": "t1", "name": "bash", "arguments": {"command": "ls"}},
                {"type": "text", "text": "Here they are."}]}}),
            json!({"type": "message", "message": {"role": "toolResult", "toolCallId": "t1", "isError": true,
                "content": [{"type": "text", "text": "boom"}]}}),
        ])
    }

    fn agents(codex: &Path, pi: &Path) -> Agents {
        let nothing = tempfile::tempdir().expect("a temp folder");
        Agents {
            external: External::at(nothing.path()),
            stores: Stores::at(Some(codex), Some(pi), None),
            ..Agents::default()
        }
    }

    async fn read(agents: &Agents, id: &str, last: u32) -> SessionsReply {
        agents.transcript_reply(id, last, true, None).await
    }

    #[tokio::test]
    async fn a_codex_session_started_outside_beep_lists_then_reads() {
        let codex = tempfile::tempdir().expect("a temp folder");
        let pi = tempfile::tempdir().expect("a temp folder");
        let dir = codex.path().join("sessions/2026/11/02");
        fs::create_dir_all(&dir).expect("folders");
        fs::write(
            dir.join(format!("rollout-2026-11-02T02-32-34-{CODEX_ID}.jsonl")),
            codex_rollout(),
        )
        .expect("a rollout");
        let agents = agents(codex.path(), pi.path());

        let listed = agents
            .stores_external("codex", 10, Some(CODEX_ID.into()))
            .await
            .expect("the listing");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, CODEX_ID);
        assert_eq!(listed[0].agent, "codex");

        let SessionsReply::Transcript { entries, earlier } = read(&agents, CODEX_ID, 50).await
        else {
            panic!("a transcript");
        };
        let shown: Vec<(EntryKind, &str)> = entries
            .iter()
            .map(|entry| (entry.kind, entry.text.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                (EntryKind::User, "fix the retry flag"),
                (EntryKind::Thought, "Looking at the flag"),
                (EntryKind::Tool, "exec_command: cargo test"),
                (EntryKind::Agent, "The flag is fixed."),
                (EntryKind::User, "now the docs"),
                (EntryKind::Agent, "Docs updated."),
            ]
        );
        assert_eq!(earlier, 0);

        let SessionsReply::Transcript { entries, earlier } = read(&agents, CODEX_ID, 2).await
        else {
            panic!("a transcript");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].text, "Docs updated.");
        assert_eq!(earlier, 4);
    }

    #[tokio::test]
    async fn pages_before_an_entry_cover_a_found_session_without_a_gap_or_a_repeat() {
        let codex = tempfile::tempdir().expect("a temp folder");
        let pi = tempfile::tempdir().expect("a temp folder");
        let dir = codex.path().join("sessions/2026/11/02");
        fs::create_dir_all(&dir).expect("folders");
        fs::write(
            dir.join(format!("rollout-2026-11-02T02-32-34-{CODEX_ID}.jsonl")),
            codex_rollout(),
        )
        .expect("a rollout");
        let agents = agents(codex.path(), pi.path());
        let SessionsReply::Transcript { entries: all, .. } = read(&agents, CODEX_ID, 50).await
        else {
            panic!("a transcript");
        };

        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let SessionsReply::Transcript { entries, earlier } =
                agents.transcript_reply(CODEX_ID, 4, true, before).await
            else {
                panic!("a transcript");
            };
            if let Some(end) = before {
                assert_eq!(
                    u64::from(earlier) + entries.len() as u64,
                    end,
                    "ends at before"
                );
            }
            if entries.is_empty() {
                break;
            }
            seen.splice(0..0, entries);
            before = Some(u64::from(earlier));
            if earlier == 0 {
                break;
            }
        }
        assert_eq!(seen, all);
    }

    #[tokio::test]
    async fn a_pi_session_reads_and_marks_a_failed_call() {
        let codex = tempfile::tempdir().expect("a temp folder");
        let pi = tempfile::tempdir().expect("a temp folder");
        let dir = pi.path().join("--srv-project--");
        fs::create_dir_all(&dir).expect("folders");
        fs::write(
            dir.join(format!("2026-11-02T01-32-34-000Z_{PI_ID}.jsonl")),
            pi_session(),
        )
        .expect("a session");
        let agents = agents(codex.path(), pi.path());
        let SessionsReply::Transcript { entries, .. } = read(&agents, PI_ID, 50).await else {
            panic!("a transcript");
        };
        let shown: Vec<(EntryKind, &str)> = entries
            .iter()
            .map(|entry| (entry.kind, entry.text.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                (EntryKind::User, "list the files"),
                (EntryKind::Thought, "I will run ls"),
                (EntryKind::Tool, "bash: ls (failed)"),
                (EntryKind::Agent, "Here they are."),
            ]
        );
    }

    #[tokio::test]
    async fn an_unknown_session_is_no_session() {
        let codex = tempfile::tempdir().expect("a temp folder");
        let pi = tempfile::tempdir().expect("a temp folder");
        let agents = agents(codex.path(), pi.path());
        let SessionsReply::Error { message } = read(&agents, "nothing", 5).await else {
            panic!("an error");
        };
        assert_eq!(message, "there is no session nothing");
    }
}
