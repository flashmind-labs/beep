//! The transcript and state of a session: entries, spend and questions.

use super::{
    CLAUDE_RATE_LIMIT, Live, MAX_ENTRIES, MAX_ENTRY, MAX_LINE, TurnTokens,
    content::{cut, detail},
    tools::CodexChild,
};
use crate::agents::mcp::calls::Tool as BeepTool;
use crate::agents::{
    acp::wire_name,
    images::{Images, Picture},
    subagent::Run,
    unix_now,
    usage::{UsageHub, claude_rate_limit},
    window::window,
};
use agent_client_protocol::schema::v1::{RequestPermissionRequest, SessionUpdate, ToolCallId};
use beep_proto::{
    AgentSlug,
    node::{
        agent::{AnswerOption, Entry, EntryKind, OptionKind, Question, SessionInfo, Status},
        usage::TurnUsage,
    },
};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Instant, SystemTime},
};

impl Live {
    pub(crate) fn new(info: SessionInfo, images: Arc<Images>, usage: Arc<UsageHub>) -> Self {
        let folder = PathBuf::from(&info.cwd);
        Self {
            folder: Some(folder.clone()),
            place_wanted: Some(folder),
            info,
            images,
            usage,
            ..Self::default()
        }
    }

    /// Counts what the session spent since the last count, with `tokens`
    /// the turn used, as a finished turn. Nothing when it spent nothing.
    pub(crate) fn spent(&mut self, tokens: TurnTokens) {
        let cost_usd = self.unbilled.take();
        if tokens.total == 0 && tokens.models.is_empty() && cost_usd.is_none() {
            return;
        }
        self.info.tokens = self.info.tokens.saturating_add(tokens.total);
        if let Some(cost) = cost_usd {
            *self.info.cost_usd.get_or_insert(0.0) += cost;
        }
        self.usage.record(TurnUsage {
            session: self.info.id.clone(),
            agent: AgentSlug::new(self.info.agent.as_str()),
            cwd: self.info.cwd.clone(),
            at: unix_now(),
            cost_usd,
            tokens: tokens.total,
            models: tokens.models,
            agent_session: self.agent_session.clone(),
        });
    }

    /// Takes a `usage_update`'s running cost and the rate limits it carries.
    pub(super) fn usage_update(
        &mut self,
        cost: Option<(f64, &str)>,
        meta: Option<&serde_json::Map<String, Value>>,
    ) {
        if let Some((amount, "USD")) = cost
            && amount.is_finite()
            && amount >= 0.0
        {
            // A total lower than before starts again, as after a restart.
            let delta = if amount >= self.cost_seen {
                amount - self.cost_seen
            } else {
                amount
            };
            self.cost_seen = amount;
            if delta > 0.0 {
                *self.unbilled.get_or_insert(0.0) += delta;
            }
        }
        if let Some(info) = meta.and_then(|meta| meta.get(CLAUDE_RATE_LIMIT)) {
            self.usage
                .update_windows(&AgentSlug::CLAUDE, claude_rate_limit(info), unix_now());
        }
    }

    /// Keeps a picture the transcript shows, for the server to ask for,
    /// and has it sent to the server to keep.
    pub(crate) fn keep(&self, picture: Picture) {
        self.images.keep_and_send(&self.info.id, picture);
    }

    /// The kept picture `key` of this session.
    #[cfg(test)]
    pub(crate) fn picture(&self, key: &str) -> Option<Arc<Picture>> {
        self.images.get(&self.info.id, key)
    }

    pub(crate) fn status(&self) -> Status {
        self.info.state.status
    }

    /// The Codex subagents shown at entry `from` or later.
    pub(crate) fn codex_children(&self, from: u64) -> Vec<CodexChild> {
        self.tools
            .iter()
            .filter(|(_, tool)| tool.entry >= from && tool.parent.is_none())
            .filter_map(|(id, tool)| {
                let subagent = tool.subagent.as_ref().filter(|sub| sub.codex)?;
                (!subagent.threads.is_empty()).then(|| CodexChild {
                    call: id.clone(),
                    threads: subagent.threads.clone(),
                })
            })
            .collect()
    }

    /// The entry of call `id` with its subagent's `run` read from disk,
    /// and where that entry is.
    pub(crate) fn with_run(&self, id: &ToolCallId, run: &Run) -> Option<(u64, String)> {
        let tool = self.tools.get(id)?;
        Some((tool.entry, tool.entry_with(&self.steps_of(tool), Some(run))))
    }

    /// The last `last` entries and how many came before them.
    #[cfg(test)]
    pub(crate) fn transcript(&self, last: usize) -> (Vec<Entry>, u64) {
        self.transcript_before(last, None)
    }

    /// The last `last` entries before the one with index `before` (the end
    /// when `None`) and how many came before them. Entries the session
    /// dropped are gone: past them the answer is empty.
    pub(crate) fn transcript_before(&self, last: usize, before: Option<u64>) -> (Vec<Entry>, u64) {
        let range = window(self.dropped, self.transcript.len(), last, before);
        let earlier = self.dropped + range.start as u64;
        let entries = self.transcript.range(range).cloned().collect();
        (entries, earlier)
    }

    pub(crate) fn push(&mut self, kind: EntryKind, text: &str) {
        self.end_thought(Instant::now());
        self.in_message = false;
        self.push_entry(kind, cut(text, MAX_ENTRY));
    }

    pub(super) fn push_entry(&mut self, kind: EntryKind, text: String) -> u64 {
        if self.transcript.len() >= MAX_ENTRIES {
            self.transcript.pop_front();
            self.dropped += 1;
        }
        self.transcript.push_back(Entry::new(kind, text));
        self.dropped + self.transcript.len() as u64 - 1
    }

    pub(super) fn entry_mut(&mut self, at: u64) -> Option<&mut Entry> {
        let index = usize::try_from(at.checked_sub(self.dropped)?).ok()?;
        self.transcript.get_mut(index)
    }

    /// A prompt went to the agent.
    pub(crate) fn prompted(&mut self, text: &str) {
        // Until the agent names the session, its first prompt does.
        if self.info.title.is_none()
            && let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty())
        {
            self.info.title = Some(cut(line, MAX_LINE));
        }
        self.end_thought(Instant::now());
        self.active_at = Some(Instant::now());
        self.in_message = false;
        self.turn_start = self.push_entry(EntryKind::User, cut(text, MAX_ENTRY));
        self.turn_links.clear();
        self.turn_images.clear();
        self.turn_began = Some(SystemTime::now());
        // A tool whose entry left the transcript has nothing left to show.
        let dropped = self.dropped;
        self.tools.retain(|_, tool| tool.entry >= dropped);
        let state = &mut self.info.state;
        state.status = Status::Working;
        state.summary = "Thinking".into();
        state.error = None;
    }

    /// The question the agent asked in `session/request_permission`.
    pub(crate) fn question(&self, id: u32, request: &RequestPermissionRequest) -> Question {
        let call = &request.tool_call;
        let known = self.tools.get(&call.tool_call_id);
        let title = call
            .fields
            .title
            .clone()
            .or_else(|| known.map(|tool| tool.title.clone()))
            .unwrap_or_else(|| "Use a tool".into());
        // A call to Beep's own tools asks in its words, not by the tool's name.
        let beep = BeepTool::of_call(&title, call.meta.as_ref(), call.fields.raw_input.as_ref())
            .or_else(|| known.and_then(|tool| tool.beep.as_ref().map(|beep| beep.tool)));
        let text = match beep {
            Some(tool) => {
                let input = call
                    .fields
                    .raw_input
                    .as_ref()
                    .or_else(|| known.and_then(|tool| tool.beep.as_ref()?.input.as_ref()));
                tool.title(input)
            }
            None => title,
        };
        let options = request
            .options
            .iter()
            .map(|option| AnswerOption {
                id: option.option_id.0.to_string(),
                name: option.name.clone(),
                kind: OptionKind::from_acp(&wire_name(&option.kind)),
            })
            .collect();
        let text = cut(&text, MAX_LINE);
        // Some agents title the question with the command itself.
        let detail = call
            .fields
            .raw_input
            .as_ref()
            .and_then(detail)
            .filter(|detail| *detail != text);
        Question {
            id,
            text,
            detail,
            options,
        }
    }

    /// Applies one `session/update`. True when the compact state changed,
    /// so the server should hear of it; transcript changes alone are not
    /// sent.
    pub(crate) fn apply(&mut self, update: &SessionUpdate) -> bool {
        self.apply_at(update, Instant::now())
    }
}
