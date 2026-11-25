//! The transcript and state of a session: entries, spend and questions.

use super::{
    CLAUDE_RATE_LIMIT, Live, MAX_ENTRIES, MAX_ENTRY, MAX_LINE, TurnTokens,
    content::{cut, detail},
    tools::CodexChild,
};
use crate::agents::calls::Tool as BeepTool;
use crate::agents::{
    acp::wire_name,
    images::Images,
    picture::Picture,
    subagent::Run,
    unix_now,
    usage::{UsageHub, claude_rate_limit, flsh_credits},
    window::window,
};
use agent_client_protocol::v1::schema::{RequestPermissionRequest, SessionUpdate, ToolCallId};
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
    pub(crate) fn new(mut info: SessionInfo, images: Arc<Images>, usage: Arc<UsageHub>) -> Self {
        info.title = info.title.as_deref().and_then(crate::agents::tags::title);
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
    pub(crate) fn spent(&mut self, mut tokens: TurnTokens) {
        if let Some(total) = self.flsh_tokens {
            tokens.total = total.checked_sub(self.info.tokens).unwrap_or(total);
        }

        let cost_usd = self.unbilled.take();

        if tokens.total == 0 && tokens.models.is_empty() && cost_usd.is_none() {
            return;
        }

        self.info.tokens = self
            .flsh_tokens
            .unwrap_or_else(|| self.info.tokens.saturating_add(tokens.total));

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
    pub(crate) fn usage_update(
        &mut self,
        cost: Option<(f64, &str)>,
        meta: Option<&serde_json::Map<String, Value>>,
    ) {
        let flsh_usage = (self.info.agent == "flsh")
            .then(|| meta.and_then(|meta| meta.get("flsh/sessionUsage")))
            .flatten();
        let amount = cost
            .filter(|(amount, currency)| *currency == "USD" && amount.is_finite() && *amount >= 0.0)
            .map(|(amount, _)| amount)
            .or_else(|| {
                // flsh omits the ACP cost if any request was unpriced.
                // Its metadata still carries the subtotal of priced requests.
                flsh_usage?
                    .get("total_cost")?
                    .as_str()?
                    .parse::<f64>()
                    .ok()
                    .filter(|amount| amount.is_finite() && *amount > 0.0)
            });

        if let Some(amount) = amount {
            // A total lower than before starts again, as after a restart.
            let delta = if amount >= self.cost_seen {
                amount - self.cost_seen
            } else {
                amount
            };
            self.cost_seen = amount;

            if delta > 0.0 {
                *self.info.cost_usd.get_or_insert(0.0) += delta;
                *self.unbilled.get_or_insert(0.0) += delta;
            }
        }

        if let Some(total) = flsh_usage
            && total.get("tokens_unknown") == Some(&Value::Bool(false))
            && let Some(tokens) = total
                .get("input_tokens")
                .and_then(Value::as_u64)
                .zip(total.get("output_tokens").and_then(Value::as_u64))
                .and_then(|(input, output)| input.checked_add(output))
        {
            self.flsh_tokens = Some(tokens);
        }

        if self.info.agent == "flsh"
            && let Some(credits) = meta
                .and_then(|meta| meta.get("flsh/credits"))
                .and_then(flsh_credits)
        {
            match self
                .info
                .credits
                .iter_mut()
                .find(|held| held.provider == credits.provider)
            {
                Some(held) => *held = credits.clone(),
                None => self.info.credits.push(credits.clone()),
            }

            self.usage.update_credits(
                &(AgentSlug::new(self.info.agent.as_str()), self.info.account.clone()),
                credits,
                unix_now(),
            );
        }

        if let Some(info) = meta.and_then(|meta| meta.get(CLAUDE_RATE_LIMIT)) {
            self.usage.update_windows(
                &(AgentSlug::CLAUDE, self.info.account.clone()),
                claude_rate_limit(info),
                unix_now(),
            );
        }
    }

    /// Keeps a picture the transcript shows, for the apps to ask for.
    pub(crate) fn keep(&self, picture: Picture) {
        self.images.keep(&self.info.id, picture);
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

    pub(crate) fn claude_children(&self, from: u64, len: usize) -> Vec<(usize, String)> {
        self.tools
            .iter()
            .filter_map(|(id, tool)| {
                if tool.parent.is_some() || tool.subagent.as_ref().is_none_or(|sub| sub.codex) {
                    return None;
                }

                let at = usize::try_from(tool.entry.checked_sub(from)?).ok()?;
                (at < len).then(|| (at, id.to_string()))
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

    /// Notes whether the user stopped the turn that ended, or closed the
    /// session: a child's log says so, since its parent may have too.
    pub(crate) fn stopped(&mut self, by_user: bool) {
        self.stopped_by_user = by_user;
        if by_user && self.info.parent.is_some() {
            self.push(EntryKind::Other, "Stopped by you");
        }
    }

    pub(crate) fn push(&mut self, kind: EntryKind, text: &str) {
        self.end_thought(Instant::now());
        self.in_message = false;
        self.push_entry(kind, cut(text, MAX_ENTRY));
    }

    pub(super) fn push_entry(&mut self, kind: EntryKind, text: String) -> u64 {
        let mut entry = Entry::new(kind, text);

        if self.info.state.status != Status::Starting {
            entry.sent_at = Some(unix_now());
        }

        self.push_whole(entry)
    }

    fn push_whole(&mut self, entry: Entry) -> u64 {
        if self.transcript.len() >= MAX_ENTRIES {
            self.transcript.pop_front();
            self.dropped += 1;
            // Long turns also evict entries. Batch the scan so pruning does
            // not walk every retained tool for every incoming entry.
            if self.dropped.is_multiple_of(64) {
                self.prune_tools();
            }
        }

        self.transcript.push_back(entry);
        self.dropped + self.transcript.len() as u64 - 1
    }

    /// Shows again `earlier`, the transcript the session had before the
    /// node restarted, for an agent that resumes without replaying it.
    pub(crate) fn restore(&mut self, earlier: Vec<Entry>) {
        self.end_thought(Instant::now());
        self.in_message = false;

        for entry in earlier {
            self.push_whole(entry);
        }
    }

    pub(super) fn entry_mut(&mut self, at: u64) -> Option<&mut Entry> {
        let index = usize::try_from(at.checked_sub(self.dropped)?).ok()?;
        self.transcript.get_mut(index)
    }

    /// Older Claude adapters send a compaction summary as ordinary user chunks.
    /// Wait for all chunks before removing the adapter's context wrapper.
    pub(crate) fn finish_replay(&mut self) {
        use crate::agents::compaction::{CLAUDE_CONTINUATION, summary_note};
        if self.info.agent != "claude" {
            return;
        }
        for entry in &mut self.transcript {
            if entry.kind != EntryKind::User {
                continue;
            }

            if let Some((context, rest)) = entry.text.split_once(CLAUDE_CONTINUATION)
                && rest.starts_with(" that ran out of context.")
                && crate::agents::tags::parts(context)
                    .iter()
                    .all(|part| matches!(part, crate::agents::tags::Part::Context(_)))
            {
                entry.kind = EntryKind::Compacted;
                entry.text = summary_note(&entry.text[context.len()..]);
            }
        }
    }

    pub(in crate::agents) fn retained_range(&self) -> (u64, usize) {
        (self.dropped, self.transcript.len())
    }

    /// Copies known dates onto replayed entries in transcript order.
    pub(crate) fn restore_times(&mut self, earlier: &[Entry]) {
        let mut saved = earlier.iter().rev();

        for entry in self.transcript.iter_mut().rev() {
            if let Some(index) = saved.clone().position(|old| old.kind == entry.kind && old.text == entry.text) {
                entry.sent_at = saved.nth(index).and_then(|old| old.sent_at);
            }
        }
    }

    /// A prompt went to the agent.
    pub(crate) fn prompted(&mut self, text: &str) {
        // Until the agent names the session, its first prompt does.
        if self.info.title.is_none() {
            self.info.title = crate::agents::tags::title(text);
        }

        self.end_thought(Instant::now());
        self.active_at = Some(Instant::now());
        self.info.last_activity = unix_now();
        self.in_message = false;
        self.turn_start = self.push_entry(EntryKind::User, cut(text, MAX_ENTRY));
        if let Some(entry) = self.entry_mut(self.turn_start) {
            entry.sent_at = Some(unix_now());
        }
        self.turn_links.clear();
        self.turn_images.clear();
        self.turn_began = Some(SystemTime::now());
        // A tool whose entry left the transcript has nothing left to show.
        self.prune_tools();
        // The prompt's turn tells the status now.
        if let Some(turn) = &mut self.autonomous_turn {
            turn.shown = false;
        }
        let state = &mut self.info.state;
        state.status = Status::Working;
        state.summary = "Thinking".into();
        state.error = None;
    }

    /// A message joined the running turn, keeping its files and timing.
    pub(crate) fn steered(&mut self, text: &str) {
        self.end_thought(Instant::now());
        self.active_at = Some(Instant::now());
        self.info.last_activity = unix_now();
        self.in_message = false;
        self.push_entry(EntryKind::User, cut(text, MAX_ENTRY));
    }

    fn prune_tools(&mut self) {
        let dropped = self.dropped;
        self.tools.retain(|_, tool| tool.entry >= dropped);
        self.codex_threads
            .retain(|_, call| self.tools.contains_key(call));
    }

    /// The question the agent asked in `session/request_permission`.
    pub(crate) fn question(&self, request: &RequestPermissionRequest) -> Question {
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
                kind: wire_name(&option.kind).parse().unwrap_or(OptionKind::Other),
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
            id: 0,
            text,
            detail,
            options,
        }
    }

    /// Applies one `session/update`. True when the compact state changed;
    /// transcript and metadata changes can still need publishing.
    pub(crate) fn apply(&mut self, update: &SessionUpdate) -> bool {
        self.apply_at(update, Instant::now())
    }
}
