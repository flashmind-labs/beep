//! A session's turns: prompts, steers, stops and compaction.

use super::{
    actor::{Actor, HeldEnd, Reply, Stopping, Turn, stop_reason},
    tokens::turn_tokens,
    types::MAX_RUNNING_TOOLS,
};
use crate::agents::{
    acp::{self, Prompt},
    compaction::{COMPACT_PROMPT, COMPACTED_ENTRY, TurnEnd, summary_note},
    image_files,
    live::{TurnFiles, TurnMade, TurnWorktrees},
    made_files, worktrees,
};
use agent_client_protocol::{
    Agent, ConnectionTo,
    v1::schema::{CancelNotification, PromptResponse, SessionId, SessionUpdate, ToolCallStatus},
};
use anyhow::Result;
use beep_proto::node::agent::{AlertKind, EntryKind, State, Status, StopReason};
use beep_proto::watched;
use futures::FutureExt;
use serde_json::Value;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::task::spawn_blocking;

/// How long a turn's end waits for running subagents that send nothing
/// before it alerts anyway: a Codex subagent may never say it ended.
pub(super) const SUBAGENTS_QUIET: Duration = Duration::from_secs(30 * 60);

impl Actor {
    /// Marks the running turn as stopping for the user, at once, and gives
    /// the agent `stop_within` to end it before the node stops the agent.
    pub(super) fn stop_due(&mut self) {
        let Some(turn) = &mut self.turn else {
            return;
        };

        if turn.stopping == Some(Stopping::User) {
            return;
        }

        turn.stopping = Some(Stopping::User);
        let turn = turn.number;
        self.show_stopping(Stopping::User);
        let within = self.stop_within;
        self.replies.push(
            async move {
                tokio::time::sleep(within).await;
                Reply::StopDue { turn }
            }
            .boxed(),
        );
    }

    pub(super) fn show_stopping(&mut self, stopping: Stopping) {
        self.live.send_if_modified(|live| {
            let summary = Into::<&'static str>::into(stopping);
            let changed = live.info.state.summary != summary;
            if changed {
                live.info.state.summary = summary.into();
            }
            changed
        });
    }

    /// Follows unfinished tools for the cancel fallback. Any completed tool
    /// lets a steer through, even when other commands keep running.
    pub(super) fn tool_finished(&mut self, update: &SessionUpdate) -> bool {
        let (id, status) = match update {
            SessionUpdate::ToolCall(call) => (&call.tool_call_id, Some(call.status)),
            SessionUpdate::ToolCallUpdate(update) => (&update.tool_call_id, update.fields.status),
            _ => return false,
        };

        match status {
            Some(ToolCallStatus::Completed | ToolCallStatus::Failed) => {
                self.running.remove(id);
                true
            }
            Some(_) if self.running.len() < MAX_RUNNING_TOOLS => {
                self.running.insert(id.clone());
                false
            }
            _ => false,
        }
    }

    /// Injects immediately when supported. Older adapters end the turn at
    /// a tool boundary so the steer can go as the next prompt. A turn the
    /// user stopped takes none: injected, the agent works on in that turn
    /// instead of stopping, and the steer goes as the next prompt anyway.
    pub(super) fn steer_now(&mut self, cx: &ConnectionTo<Agent>) -> Result<()> {
        let Some(turn) = &mut self.turn else {
            return Ok(());
        };

        if self.backlog.steers_empty()
            || turn.stopping.is_some()
            || self.backlog.is_steering()
            || turn.steer_after
            || !self.asked.is_empty()
        {
            return Ok(());
        }

        let Some(session) = self.acp_session.clone() else {
            return Ok(());
        };

        if self.offers.takes_steering {
            let Some(prompt) = self.backlog.pop_steer() else {
                return Ok(());
            };
            let request = acp::steering(&session, &prompt)?;
            self.backlog.begin_steer(prompt);
            self.send(cx, request, Reply::Steered);
            return Ok(());
        }

        cx.send_notification(CancelNotification::new(session))?;
        turn.stopping = Some(Stopping::Steer);
        self.show_stopping(Stopping::Steer);
        Ok(())
    }

    pub(super) fn show_steer(&mut self) {
        let Some(steer) = self.backlog.steering_mut().filter(|steer| !steer.shown) else {
            return;
        };
        steer.shown = true;
        self.live.send_modify(|live| {
            for picture in steer.prompt.pictures() {
                live.keep(picture);
            }

            live.steered(&steer.prompt.shown());
        });
    }

    pub(super) fn steered(
        &mut self,
        cx: &ConnectionTo<Agent>,
        result: Result<Value, agent_client_protocol::Error>,
    ) -> Result<()> {
        let outcome = result.and_then(|value| {
            serde_json::from_value::<acp::SteeringOutcome>(value)
                .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))
        });

        if matches!(outcome, Ok(acp::SteeringOutcome::Injected)) {
            self.show_steer();
        }

        let Some(steer) = self.backlog.take_steering() else {
            return Ok(());
        };

        match outcome {
            Ok(acp::SteeringOutcome::Injected) => {}
            Err(e) if steer.shown => {
                self.note(&format!(
                    "The agent echoed the steer but its delivery reply failed: {e}"
                ));
            }
            // Taken back, so it is not sent again, but an adapter that failed
            // it is not asked to steer the next one.
            _ if steer.dropped => {
                if !matches!(outcome, Ok(acp::SteeringOutcome::PromptRequired)) {
                    self.offers.takes_steering = false;
                }
            }
            Ok(acp::SteeringOutcome::PromptRequired) => {
                self.backlog.push_front_steer(steer.prompt);
                if let Some(turn) = &mut self.turn {
                    turn.steer_after = true;
                }
                // Wait for the owning prompt's terminal reply. Sending it
                // again now would race the adapter's end-of-turn handling.
                self.show_queued();

                if self.turn.is_none() {
                    self.next_prompt(cx);
                }

                return Ok(());
            }
            Ok(acp::SteeringOutcome::Failed) | Err(_) => {
                let reason = outcome.err().map(|e| format!(": {e}")).unwrap_or_default();
                self.note(&format!(
                    "The agent could not inject the message{reason}; it will go as the next prompt"
                ));
                self.offers.takes_steering = false;
                self.backlog.push_front_steer(steer.prompt);
            }
        }

        self.show_queued();
        if self.turn.is_none() {
            if self.backlog.waiting() == 0 {
                let ended = watched::read_sent(&self.live, |live| {
                    live.info
                        .state
                        .last_stop
                        .map(|stop_reason| AlertKind::TurnEnded {
                            stop_reason,
                            last_message: live.info.last_message.clone(),
                            error: live.info.state.error.clone(),
                        })
                });

                if let Some(ended) = ended {
                    self.turn_ended(ended);
                }
            }

            self.next_prompt(cx);
        } else if self.offers.takes_steering || self.running.is_empty() {
            self.steer_now(cx)?;
        }

        Ok(())
    }

    /// Alerts that the turn ended, or holds the alert while subagents it
    /// started still run.
    fn turn_ended(&mut self, ended: AlertKind) {
        if watched::read_sent(&self.live, |live| live.info.state.subagents) == 0 {
            self.held_end = None;
            self.alert(ended);
            return;
        }

        self.held_end = Some(HeldEnd {
            ended,
            due: tokio::time::Instant::now() + SUBAGENTS_QUIET,
        });
    }

    /// The agent sent news while a turn's end is held: alert once no
    /// subagent runs, or else wait `SUBAGENTS_QUIET` from now.
    pub(super) fn subagents_moved(&mut self) {
        let Some(held) = &mut self.held_end else {
            return;
        };

        if watched::read_sent(&self.live, |live| live.info.state.subagents) > 0 {
            held.due = tokio::time::Instant::now() + SUBAGENTS_QUIET;
            return;
        }

        self.subagents_quiet();
    }

    /// Alerts the held turn's end: its subagents ended or went quiet.
    pub(super) fn subagents_quiet(&mut self) {
        if let Some(held) = self.held_end.take() {
            self.alert(held.ended);
        }
    }

    pub(super) fn show_queued(&mut self) {
        let queued = self.backlog.waiting();
        let messages = self.backlog.messages();
        self.live.send_if_modified(|live| {
            let changed =
                live.info.state.queued != queued || live.info.state.queued_messages != messages;
            live.info.state.queued = queued;
            live.info.state.queued_messages = messages;
            changed
        });
    }

    pub(super) fn turn_done(
        &mut self,
        cx: &ConnectionTo<Agent>,
        stop: StopReason,
        error: Option<String>,
        by_user: bool,
    ) {
        // A turn that ends leaves no question open. A dropped responder
        // sends nothing, so each is answered cancelled.
        while let Some(asked) = self.asked.pop_front() {
            if let Err(e) = asked.waiting.cancel() {
                log::debug!("an open question was not cancelled: {e}");
            }
        }
        // A stop ends the subagents the turn waited on; Claude Code holds a
        // turn that ends by itself until its background subagents end too.
        self.live.send_if_modified(|live| {
            let before = live.info.state.subagents;

            if self.offers.protocol == acp::Protocol::V1 {
                if stop == StopReason::Cancelled {
                    live.close_interrupted_subagents();
                } else {
                    live.end_background_subagents();
                }
            }

            live.info.state.subagents != before
        });

        // Prompts queued behind the turn run first: one alert for the lot.
        if self.backlog.waiting() == 0 {
            let last_message =
                watched::read_sent(&self.live, |live| live.info.last_message.clone());
            self.turn_ended(AlertKind::TurnEnded {
                stop_reason: stop,
                last_message,
                error: error.clone(),
            });
        }

        let mut files = None;
        let mut made = None;
        let mut worktrees = None;
        let by_user = by_user && stop == StopReason::Cancelled;
        self.live.send_modify(|live| {
            live.end_thought(Instant::now());
            live.stopped(by_user);
            live.info.turns += 1;
            live.info.last_activity = crate::agents::unix_now();
            live.info.question = None;
            let state: &mut State = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
            state.error = error;
            state.last_stop = Some(stop);
            // A prompt folded into a turn the agent started can end first.
            live.show_autonomous_turn();
            files = live.turn_files();
            made = live.turn_made();
            worktrees = live.turn_worktrees();
        });

        if let Some(files) = files {
            self.read_files(files);
        }

        if let Some(made) = made {
            self.find_made(made);
        }

        if let Some(worktrees) = worktrees {
            self.find_worktrees(worktrees);
        }

        self.count_changes();

        let (usage, commands) = watched::read_sent(&self.live, |live| {
            (live.info.context, live.commands.clone())
        });
        self.compactor.turn_ended(TurnEnd {
            usage,
            commands: &commands,
        });
        self.next_prompt(cx);
    }

    /// Reads the image files a turn named off the async workers; they show
    /// once read.
    fn read_files(&mut self, files: TurnFiles) {
        let TurnFiles {
            cwd,
            paths,
            made,
            since,
            entry,
        } = files;
        let read = spawn_blocking(move || {
            let mut pictures = image_files::read(&cwd, &paths);
            pictures.extend(image_files::read_since(&cwd, &made, since));
            pictures
        });
        self.replies.push(
            async move {
                let pictures = read.await.unwrap_or_else(|e| {
                    log::warn!("reading a turn's image files failed: {e}");
                    Vec::new()
                });
                Reply::FilesRead { entry, pictures }
            }
            .boxed(),
        );
    }

    /// Looks for the files a turn's commands made off the async workers;
    /// the turn's note names them once found.
    fn find_made(&mut self, made: TurnMade) {
        let TurnMade {
            cwd,
            since,
            written,
        } = made;
        let found = spawn_blocking(move || made_files::find(&cwd, since, &written));
        self.replies.push(
            async move {
                let paths = found.await.unwrap_or_else(|e| {
                    log::warn!("looking for a turn's made files failed: {e}");
                    Vec::new()
                });
                Reply::Made(paths)
            }
            .boxed(),
        );
    }

    /// Looks for the worktrees a turn's commands added off the async
    /// workers; the session records them once found.
    fn find_worktrees(&mut self, turn: TurnWorktrees) {
        let TurnWorktrees {
            cwd,
            since,
            commands,
        } = turn;
        let found = spawn_blocking(move || worktrees::made_since(&cwd, since, &commands));
        self.replies.push(
            async move {
                let trees = found.await.unwrap_or_else(|e| {
                    log::warn!("looking for a turn's worktrees failed: {e}");
                    Vec::new()
                });
                Reply::Worktrees(trees)
            }
            .boxed(),
        );
    }

    /// Counts the lines changed in the checkout the session works in, for
    /// the lists to show beside it.
    pub(super) fn count_changes(&mut self) {
        let folder = watched::read_sent(&self.live, |live| {
            live.info
                .place
                .as_ref()
                .map_or_else(|| live.info.cwd.clone(), |place| place.folder.clone())
        });
        let checkouts = self.node.checkouts.clone();
        self.replies.push(
            async move { Reply::Changed(checkouts.changes(PathBuf::from(folder)).await) }.boxed(),
        );
    }

    /// Sends `/compact` ahead of the waiting prompts.
    fn compact(&mut self, cx: &ConnectionTo<Agent>, session: &SessionId) {
        let prompt = Prompt::from(COMPACT_PROMPT.to_owned());
        self.send_prompt(cx, acp::prompt(session, &prompt, None), true);
        self.live.send_modify(|live| {
            // What it said before no longer holds once compacted.
            live.info.context = None;
            live.take_compacted();
            let state = &mut live.info.state;
            state.status = Status::Working;
            state.summary = "Compacting the conversation".into();
            state.error = None;
        });
    }

    /// The node's `/compact` ended: the transcript says so, unless the
    /// agent's own update did, then the waiting prompt goes.
    pub(super) fn compacted(
        &mut self,
        cx: &ConnectionTo<Agent>,
        result: Result<PromptResponse, agent_client_protocol::Error>,
    ) {
        let reply = self.compactor.finished();
        let tokens = result.as_ref().map(turn_tokens).unwrap_or_default();
        self.live.send_modify(|live| live.spent(tokens));
        let entry = match result {
            Ok(done) => {
                let stop = stop_reason(&done);
                if stop == StopReason::EndTurn {
                    self.restate();
                }
                let note = if reply.trim().is_empty() {
                    COMPACTED_ENTRY.to_owned()
                } else {
                    summary_note(&reply)
                };
                (stop == StopReason::EndTurn).then_some((EntryKind::Compacted, note))
            }
            Err(e) => {
                log::warn!("the agent did not compact: {e}");
                Some((
                    EntryKind::Other,
                    format!("[compacting the conversation failed: {e}]"),
                ))
            }
        };
        self.live.send_modify(|live| {
            if let Some((kind, line)) = entry
                && !live.take_compacted()
            {
                live.push(kind, &line);
            }

            let state = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
            live.show_autonomous_turn();
        });
        self.next_prompt(cx);
    }

    /// Has the next prompt carry the standing rules again, for an agent with
    /// no system prompt whose conversation was just compacted.
    fn restate(&mut self) {
        if self.brief.is_none() {
            self.brief.clone_from(&self.standing);
        }
    }

    /// Whether a waiting prompt can go out now: the session is ready and no
    /// prompt, compaction, steer or question of its own is open. A turn the
    /// agent started on its own takes a prompt, which joins it.
    pub(super) fn can_prompt(&self) -> bool {
        self.ready_for_prompts
            && self.acp_session.is_some()
            && self.turn.is_none()
            && !self.compactor.running()
            && !self.backlog.is_steering()
            && self.asked.is_empty()
    }

    /// Sends the next waiting prompt, if any.
    pub(super) fn next_prompt(&mut self, cx: &ConnectionTo<Agent>) {
        // A session that moves still runs what waited before the
        // move: nothing new comes in, and it ends once the queue is empty.
        if !self.can_prompt() {
            return;
        }

        let Some(session) = self.acp_session.clone() else {
            return;
        };

        let waiting = self.backlog.has_prompts();

        // A conversation past the threshold compacts as soon as its turn
        // ends, so the next message need not wait for it. A session that
        // moves with nothing waiting leaves it to its new machine.
        if (waiting || !self.moving) && self.compactor.start(false) {
            self.compact(cx, &session);
            return;
        }

        if !waiting {
            return;
        }
        // A steer goes ahead of the prompts queued for after the turn.
        let Some(mut prompt) = self.backlog.next() else {
            return;
        };

        if self.standing.is_some() {
            // The agent compacted on its own since the last prompt.
            let mut compacted = false;
            self.live.send_if_modified(|live| {
                compacted = live.take_compacted();
                false
            });

            if compacted {
                self.restate();
            }
        }

        if self.offers.takes_images == Some(false) {
            prompt.images.clear();
        }

        self.context_turn = crate::tags::said(&prompt.text).is_empty();
        let brief = self.brief.take();
        self.sent += 1;
        self.turn = Some(Turn::new(self.sent));
        self.send_prompt(cx, acp::prompt(&session, &prompt, brief.as_deref()), false);
        // The new turn tells its own end.
        self.held_end = None;
        let queued = self.backlog.waiting();
        let messages = self.backlog.messages();
        self.live.send_modify(|live| {
            for picture in prompt.pictures() {
                live.keep(picture);
            }

            live.prompted(&prompt.shown());
            if self.offers.protocol == acp::Protocol::V2 {
                live.v2_submitted();
            }
            live.info.state.queued = queued;
            live.info.state.queued_messages = messages;
        });
    }
}
