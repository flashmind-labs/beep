//! A session's turns: prompts, steers, stops and compaction.

use super::{
    MAX_RUNNING_TOOLS,
    actor::{Actor, Reply, Stopping},
};
use crate::agents::{
    acp::{self, Prompt},
    compaction::{COMPACT_PROMPT, COMPACTED_ENTRY, TurnEnd},
    image_files,
    live::{TurnFiles, TurnTokens, cut},
};
use agent_client_protocol::{
    Agent, ConnectionTo,
    schema::v1::{CancelNotification, PromptResponse, SessionId, SessionUpdate, ToolCallStatus},
};
use anyhow::Result;
use beep_proto::node::agent::{AlertKind, EntryKind, State, Status, StopReason};
use beep_proto::node::usage::{MAX_MODEL_NAME, MAX_TURN_MODELS, ModelTokens};
use beep_proto::{MAX_AGENT_IMAGES, MAX_AGENT_IMAGES_TOTAL, ModelId};
use futures::FutureExt;
use serde_json::Value;
use std::{collections::VecDeque, time::Instant};
use tokio::task::spawn_blocking;

impl Actor {
    /// Marks the running turn as stopping for the user, at once, and gives
    /// the agent `stop_within` to end it before the node stops the agent.
    pub(super) fn stop_due(&mut self) {
        let Some(turn) = self.turn else {
            return;
        };
        if self.stopping == Some(Stopping::User) {
            return;
        }
        self.stopping = Some(Stopping::User);
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
            let summary = stopping.summary();
            let changed = live.info.state.summary != summary;
            if changed {
                live.info.state.summary = summary.into();
            }
            changed
        });
    }

    /// Follows the running turn's tool calls; true when `update` finished
    /// one and none is left running, where a steer can go in.
    pub(super) fn tool_finished(&mut self, update: &SessionUpdate) -> bool {
        let (id, status) = match update {
            SessionUpdate::ToolCall(call) => (&call.tool_call_id, Some(call.status)),
            SessionUpdate::ToolCallUpdate(update) => (&update.tool_call_id, update.fields.status),
            _ => return false,
        };
        match status {
            Some(ToolCallStatus::Completed | ToolCallStatus::Failed) => {
                self.running.remove(id);
                self.running.is_empty()
            }
            Some(_) if self.running.len() < MAX_RUNNING_TOOLS => {
                self.running.insert(id.clone());
                false
            }
            _ => false,
        }
    }

    /// A tool call finished with a steer waiting: ends the turn there, and
    /// the steer goes as the next prompt once the agent says it ended. Not
    /// while a question waits, which the turn needs answered first.
    pub(super) fn steer_now(&mut self, cx: &ConnectionTo<Agent>) -> Result<()> {
        if self.steers.is_empty()
            || self.turn.is_none()
            || self.stopping.is_some()
            || !self.asked.is_empty()
        {
            return Ok(());
        }
        let Some(session) = self.acp_session.clone() else {
            return Ok(());
        };
        cx.send_notification(CancelNotification::new(session))?;
        self.stopping = Some(Stopping::Steer);
        self.show_stopping(Stopping::Steer);
        Ok(())
    }

    /// Prompts and steers waiting, as the state counts them.
    fn waiting(&self) -> u32 {
        u32::try_from(self.queued.len() + self.steers.len()).unwrap_or(u32::MAX)
    }

    pub(super) fn show_queued(&mut self) {
        let queued = self.waiting();
        self.live.send_if_modified(|live| {
            let changed = live.info.state.queued != queued;
            live.info.state.queued = queued;
            changed
        });
    }

    pub(super) fn turn_done(
        &mut self,
        cx: &ConnectionTo<Agent>,
        stop: StopReason,
        error: Option<String>,
    ) {
        // A turn that ends leaves no question open. A dropped responder
        // sends nothing, so each is answered cancelled.
        while let Some(asked) = self.asked.pop_front() {
            if let Err(e) = asked.waiting.cancel() {
                log::debug!("an open question was not cancelled: {e}");
            }
        }
        // Prompts queued behind the turn run first: one alert for the lot.
        if self.queued.is_empty() && self.steers.is_empty() {
            let last_message = self.live.borrow().info.last_message.clone();
            self.alert(AlertKind::TurnEnded {
                stop_reason: stop,
                last_message,
                error: error.clone(),
            });
        }
        let mut files = None;
        self.live.send_modify(|live| {
            live.end_thought(Instant::now());
            live.info.turns += 1;
            live.last_stop = Some(stop);
            live.info.question = None;
            let state: &mut State = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
            state.error = error;
            files = live.turn_files();
        });
        if let Some(files) = files {
            self.read_files(files);
        }
        let live = self.live.borrow();
        self.compactor.turn_ended(TurnEnd {
            usage: live.info.context,
            commands: &live.commands,
        });
        drop(live);
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

    /// Sends `/compact` ahead of the waiting prompts.
    fn compact(&mut self, cx: &ConnectionTo<Agent>, session: &SessionId) {
        let prompt = Prompt::from(COMPACT_PROMPT.to_owned());
        self.send(cx, acp::prompt(session, &prompt, None), Reply::Compacted);
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
        self.compactor.finished();
        let tokens = result.as_ref().map(turn_tokens).unwrap_or_default();
        self.live.send_modify(|live| live.spent(tokens));
        let line = match result {
            Ok(done) => {
                let stop = StopReason::from_acp(&acp::wire_name(&done.stop_reason));
                (stop == StopReason::EndTurn).then_some(COMPACTED_ENTRY.to_owned())
            }
            Err(e) => {
                log::warn!("the agent did not compact: {e}");
                Some(format!("[compacting the conversation failed: {e}]"))
            }
        };
        self.live.send_modify(|live| {
            if let Some(line) = line
                && !live.take_compacted()
            {
                live.push(EntryKind::Other, &line);
            }
            let state = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
        });
        self.next_prompt(cx);
    }

    /// Sends the next waiting prompt, if any.
    pub(super) fn next_prompt(&mut self, cx: &ConnectionTo<Agent>) {
        let Some(session) = self.acp_session.clone() else {
            return;
        };
        // Held for an update; they go when the hold ends, or after the
        // restart.
        if *self.holding.borrow() || self.moving {
            return;
        }
        if self.queued.is_empty() && self.steers.is_empty() {
            return;
        }
        if self.compactor.start(!self.asked.is_empty()) {
            self.compact(cx, &session);
            return;
        }
        // A steer goes ahead of the prompts queued for after the turn.
        let Some(prompt) = take_steers(&mut self.steers).or_else(|| self.queued.pop_front()) else {
            return;
        };
        let brief = self.brief.take();
        self.send(
            cx,
            acp::prompt(&session, &prompt, brief.as_deref()),
            Reply::Prompt,
        );
        self.sent += 1;
        self.turn = Some(self.sent);
        let queued = self.waiting();
        self.live.send_modify(|live| {
            for picture in prompt.pictures() {
                live.keep(picture);
            }
            live.prompted(&prompt.shown());
            live.info.state.queued = queued;
        });
    }
}

/// The tokens a turn used: `_meta.quota` as claude-agent-acp and
/// codex-acp give it, `token_count.totalTokens` with `model_usage` rows by
/// model, else the response's own `usage`, which any agent may send.
/// Nothing from an agent that says neither.
pub(super) fn turn_tokens(done: &PromptResponse) -> TurnTokens {
    let total = |count: &Value| count.get("totalTokens").and_then(Value::as_u64);
    let quota = done.meta.as_ref().and_then(|meta| meta.get("quota"));
    let models = quota
        .and_then(|quota| quota.get("model_usage"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(ModelTokens {
                model: ModelId::new(cut(row.get("model")?.as_str()?, MAX_MODEL_NAME)),
                tokens: total(row.get("token_count")?)?,
            })
        })
        .filter(|row| row.tokens > 0)
        .take(MAX_TURN_MODELS)
        .collect();
    let total = quota
        .and_then(|quota| quota.get("token_count"))
        .and_then(total)
        .or_else(|| done.usage.as_ref().map(|usage| usage.total_tokens))
        .unwrap_or(0);
    TurnTokens { total, models }
}

/// The steers waiting, as one prompt: their words in order and their
/// images, as many as one prompt takes. The rest wait for the next.
pub(super) fn take_steers(steers: &mut VecDeque<Prompt>) -> Option<Prompt> {
    let mut merged = steers.pop_front()?;
    while let Some(next) = steers.front() {
        let images = merged.images.len() + next.images.len();
        let bytes = merged
            .images
            .iter()
            .chain(&next.images)
            .fold(0usize, |n, image| n.saturating_add(image.data.len()));
        if images > MAX_AGENT_IMAGES || bytes > MAX_AGENT_IMAGES_TOTAL {
            break;
        }
        let Some(next) = steers.pop_front() else {
            break;
        };
        if merged.text.is_empty() {
            merged.text = next.text;
        } else if !next.text.is_empty() {
            merged.text = format!("{}\n\n{}", merged.text, next.text);
        }
        merged.images.extend(next.images);
    }
    Some(merged)
}
