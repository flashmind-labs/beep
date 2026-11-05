//! Applying the agent's `session/update` notifications.

use super::{
    EDIT_KINDS, Live, MAX_COMMANDS, MAX_COMPACTIONS, MAX_DESCRIPTION, MAX_ENTRY, MAX_LAST_MESSAGE,
    MAX_LINE, MAX_PLAN, MAX_THOUGHT, OpenThought, TurnFiles,
    content::{cut, image_picture, push_cut, replayed, thought_line},
    tools::ToolCall,
};
use crate::agents::{compaction::COMPACTED_ENTRY, image_files, images::Picture};
use agent_client_protocol::schema::{
    MaybeUndefined,
    v1::{
        AvailableCommandInput, CompactionStatus, CompactionUpdate, ContentBlock, PlanEntryStatus,
        SessionUpdate, ToolCallStatus, ToolKind,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AgentContext,
    node::agent::{AgentCommand, EntryKind, PlanEntry, PlanStatus, Status},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

impl Live {
    pub(super) fn apply_at(&mut self, update: &SessionUpdate, now: Instant) -> bool {
        let before = self.info.state.clone();
        let title = self.info.title.clone();
        // Usage, commands and the like arrive in the middle of a thought.
        let ends_thought = matches!(
            update,
            SessionUpdate::AgentMessageChunk(_)
                | SessionUpdate::UserMessageChunk(_)
                | SessionUpdate::ToolCall(_)
                | SessionUpdate::ToolCallUpdate(_)
                | SessionUpdate::Plan(_)
                | SessionUpdate::CompactionUpdate(_)
        );
        if ends_thought {
            self.end_thought(now);
            self.active_at = Some(now);
        }
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                ContentBlock::Text(text) => self.message_chunk(&text.text),
                ContentBlock::Image(image) => {
                    if let Some(picture) = image_picture(image) {
                        self.agent_image(picture);
                    }
                }
                ContentBlock::ResourceLink(link) => self.link(&link.uri),
                _ => {}
            },
            // Only a loaded session replays its prompts; a live one has
            // them from `prompted` already.
            SessionUpdate::UserMessageChunk(chunk)
                if self.info.state.status == Status::Starting =>
            {
                match &chunk.content {
                    ContentBlock::Text(text) => {
                        let mut found = Vec::new();
                        let shown = replayed(&text.text, &mut found);
                        for picture in found {
                            self.keep(picture);
                        }
                        self.user_chunk(&shown);
                    }
                    ContentBlock::Image(image) => {
                        if let Ok(data) = STANDARD.decode(&image.data) {
                            self.user_image(Picture {
                                mime: image.mime_type.clone(),
                                data,
                            });
                        }
                    }
                    _ => {}
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => self.thought_chunk(&chunk.content, now),
            SessionUpdate::ToolCall(call) => self.tool_call(call),
            SessionUpdate::ToolCallUpdate(update) => self.tool_call_update(update),
            SessionUpdate::Plan(plan) => {
                self.info.state.plan = plan
                    .entries
                    .iter()
                    .take(MAX_PLAN)
                    .map(|entry| PlanEntry {
                        content: cut(&entry.content, MAX_LINE),
                        status: match entry.status {
                            PlanEntryStatus::InProgress => PlanStatus::InProgress,
                            PlanEntryStatus::Completed => PlanStatus::Completed,
                            _ => PlanStatus::Pending,
                        },
                    })
                    .collect();
            }
            SessionUpdate::SessionInfoUpdate(info) => {
                if let MaybeUndefined::Value(new) = &info.title {
                    self.naming = false;
                    self.info.title = Some(cut(new, MAX_LINE));
                }
            }
            SessionUpdate::UsageUpdate(usage) => {
                self.info.context = Some(AgentContext {
                    used: usage.used,
                    size: usage.size,
                });
                let cost = usage
                    .cost
                    .as_ref()
                    .map(|cost| (cost.amount, cost.currency.as_str()));
                self.usage_update(cost, usage.meta.as_ref());
            }
            SessionUpdate::AvailableCommandsUpdate(update) => {
                self.commands = update
                    .available_commands
                    .iter()
                    .take(MAX_COMMANDS)
                    .map(|command| AgentCommand {
                        name: cut(&command.name, MAX_LINE),
                        description: cut(&command.description, MAX_DESCRIPTION),
                        hint: match &command.input {
                            Some(AvailableCommandInput::Unstructured(input)) => {
                                cut(&input.hint, MAX_LINE)
                            }
                            _ => String::new(),
                        },
                    })
                    .collect();
            }
            SessionUpdate::CompactionUpdate(update) => self.compaction(update),
            _ => {}
        }
        self.info.state != before || self.info.title != title
    }

    /// Whether the agent compacted the conversation since the last ask.
    pub(crate) fn take_compacted(&mut self) -> bool {
        std::mem::take(&mut self.compacted)
    }

    /// A compaction started or ended: a finished one shows once in the
    /// transcript, and the usage before it no longer holds.
    fn compaction(&mut self, update: &CompactionUpdate) {
        let failed = match &update.status {
            CompactionStatus::InProgress => {
                self.info.state.summary = "Compacting the conversation".into();
                return;
            }
            CompactionStatus::Completed => false,
            CompactionStatus::Failed => true,
            _ => return,
        };
        if self.compactions.len() >= MAX_COMPACTIONS {
            self.compactions.clear();
        }
        if !self.compactions.insert(update.compaction_id.clone()) {
            return;
        }
        if failed {
            let why = match &update.error {
                MaybeUndefined::Value(error) => format!(": {error}"),
                _ => String::new(),
            };
            self.push(
                EntryKind::Other,
                &format!("[compacting the conversation failed{why}]"),
            );
            return;
        }
        self.push(EntryKind::Other, COMPACTED_ENTRY);
        self.info.context = None;
        self.compacted = true;
    }

    fn user_chunk(&mut self, text: &str) {
        if self.in_message
            && let Some(last) = self.transcript.back_mut()
            && last.kind == EntryKind::User
        {
            push_cut(&mut last.text, text, MAX_ENTRY);
        } else {
            self.push_entry(EntryKind::User, cut(text, MAX_ENTRY));
            self.in_message = true;
            // A replayed prompt starts a turn.
            self.turn_images.clear();
            // Only the first prompt of a session with no title names it.
            self.naming = self.info.title.is_none();
        }
        if self.naming
            && let Some(prompt) = self.transcript.back()
            && let Some(line) = prompt
                .text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
        {
            self.info.title = Some(cut(line, MAX_LINE));
        }
    }

    /// An image the agent replays with a prompt, on its own line.
    fn user_image(&mut self, picture: Picture) {
        let line = picture.line();
        self.keep(picture);
        let joined = match self.transcript.back() {
            Some(last) if self.in_message && last.kind == EntryKind::User => format!("\n{line}"),
            _ => line,
        };
        self.user_chunk(&joined);
    }

    /// An image the agent shows in its message, on its own line.
    fn agent_image(&mut self, picture: Picture) {
        let line = picture.line();
        self.keep(picture);
        // Whole, and not in the last message the server is told.
        if self.in_message
            && let Some(last) = self.transcript.back_mut()
            && last.kind == EntryKind::Agent
        {
            last.text.push('\n');
            last.text.push_str(&line);
            last.text.push('\n');
        } else {
            self.push_entry(EntryKind::Agent, format!("{line}\n"));
            self.in_message = true;
            self.info.last_message.clear();
        }
    }

    /// Pictures a tool call showed, each also on an agent line of its own
    /// so the transcript shows it with the tools folded: once a turn, at
    /// most `image_files::MAX_PER_TURN`, and none too large to send.
    pub(super) fn tool_images(&mut self, pictures: &[Picture]) {
        for picture in pictures {
            if self.turn_images.len() >= image_files::MAX_PER_TURN {
                return;
            }
            let line = picture.line();
            if picture.data.len() > image_files::MAX_FILE || self.turn_images.contains(&line) {
                continue;
            }
            // `tool_output` kept it already.
            self.push_entry(EntryKind::Agent, format!("{line}\n"));
            self.in_message = false;
            self.turn_images.push(line);
        }
    }

    /// A file the agent links to: an image one shows when the turn ends.
    pub(super) fn link(&mut self, uri: &str) {
        if self.turn_links.len() < image_files::MAX_PER_TURN {
            self.turn_links
                .extend(image_files::named_in(uri).map(str::to_owned));
        }
    }

    /// The image files the turn that just ended named: in the agent's
    /// messages, its links, the files its finished edits wrote, the ones
    /// its finished reads looked at without showing them, and those its
    /// finished commands named, such as a plot one saved. Files a read
    /// showed as pictures are left out, and so is the room those pictures
    /// took. `None` when there are none.
    pub(crate) fn turn_files(&mut self) -> Option<TurnFiles> {
        let cwd = PathBuf::from(&self.info.cwd);
        let start = self.turn_start;
        let mut entry = None;
        let mut named: Vec<String> = Vec::new();
        for (at, item) in (self.dropped..).zip(&self.transcript) {
            if at > start && item.kind == EntryKind::Agent {
                entry = Some(at);
                named.extend(image_files::named_in(&item.text).map(str::to_owned));
            }
        }
        named.append(&mut self.turn_links);
        let mut done: Vec<&ToolCall> = self
            .tools
            .values()
            .filter(|tool| tool.entry > start && tool.status == ToolCallStatus::Completed)
            .collect();
        done.sort_by_key(|tool| tool.entry);
        let mut shown: Vec<PathBuf> = Vec::new();
        let mut ran: Vec<&str> = Vec::new();
        for tool in done {
            let read = tool.kind == ToolKind::Read;
            if read && !tool.pictures.is_empty() {
                shown.extend(
                    tool.paths
                        .iter()
                        .filter_map(|name| image_files::resolve(&cwd, name)),
                );
            } else if read || EDIT_KINDS.contains(&tool.kind) {
                named.extend(tool.paths.iter().cloned());
            } else if tool.kind == ToolKind::Execute {
                ran.extend(image_files::named_in(&tool.title));
                ran.extend(image_files::named_in(&tool.output));
            }
        }
        let room = image_files::MAX_PER_TURN.saturating_sub(self.turn_images.len());
        let mut paths: Vec<PathBuf> = Vec::new();
        for path in named
            .iter()
            .filter_map(|name| image_files::resolve(&cwd, name))
        {
            if paths.len() >= room {
                break;
            }
            if !paths.contains(&path) && !shown.contains(&path) {
                paths.push(path);
            }
        }
        let mut made: Vec<PathBuf> = Vec::new();
        for path in ran
            .into_iter()
            .filter_map(|name| image_files::resolve(&cwd, name))
        {
            if paths.len() + made.len() >= room {
                break;
            }
            if !paths.contains(&path) && !made.contains(&path) && !shown.contains(&path) {
                made.push(path);
            }
        }
        // Some file systems stamp to the second.
        let began = self.turn_began.unwrap_or_else(SystemTime::now);
        let since = began.checked_sub(Duration::from_secs(2)).unwrap_or(began);
        (!paths.is_empty() || !made.is_empty()).then_some(TurnFiles {
            cwd,
            paths,
            made,
            since,
            entry,
        })
    }

    /// Shows `pictures`, read from the files `turn_files` gave, after the
    /// agent message `entry`: each on its own line, once.
    pub(crate) fn show_files(&mut self, entry: Option<u64>, pictures: Vec<Picture>) {
        let mut lines = String::new();
        for picture in pictures {
            let line = picture.line();
            self.keep(picture);
            let shown = self.turn_images.contains(&line)
                || entry
                    .and_then(|at| self.entry_mut(at))
                    .is_some_and(|item| item.text.contains(&line));
            if !shown && !lines.contains(&line) {
                lines.push('\n');
                lines.push_str(&line);
            }
        }
        if lines.is_empty() {
            return;
        }
        // The lines go whole, past an entry's length if need be.
        match entry.and_then(|at| self.entry_mut(at)) {
            Some(item) => item.text.push_str(&lines),
            None => {
                self.push_entry(EntryKind::Agent, lines.trim_start().to_owned());
                self.in_message = false;
            }
        }
    }

    /// Part of a thought: the summary says "Thinking" until the thought has
    /// a line to show, then that line. The thought is a transcript entry of
    /// its own once it has text, so a redacted one shows nothing.
    fn thought_chunk(&mut self, content: &ContentBlock, now: Instant) {
        if !self.in_thought {
            self.in_thought = true;
            self.thought.clear();
            self.info.state.summary = "Thinking".into();
            // A loaded session replays its thoughts all at once.
            let started = (self.info.state.status != Status::Starting)
                .then(|| self.active_at.unwrap_or(now).min(now));
            self.open_thought = Some(OpenThought {
                entry: None,
                started,
            });
        }
        if let ContentBlock::Text(text) = content {
            if self.thought.len() < MAX_THOUGHT {
                self.thought.push_str(&text.text);
            }
            self.thought_text(&text.text);
        }
        if let Some(line) = thought_line(&self.thought) {
            self.info.state.summary = cut(&line, MAX_LINE);
        }
    }

    /// Adds `text` to the open thought's entry, made at its first text.
    fn thought_text(&mut self, text: &str) {
        let Some(open) = &self.open_thought else {
            return;
        };
        match open.entry {
            Some(at) => {
                if let Some(entry) = self.entry_mut(at) {
                    push_cut(&mut entry.text, text, MAX_ENTRY);
                }
            }
            None if !text.trim().is_empty() => {
                self.in_message = false;
                let at = self.push_entry(EntryKind::Thought, cut(text.trim_start(), MAX_ENTRY));
                if let Some(open) = &mut self.open_thought {
                    open.entry = Some(at);
                }
            }
            None => {}
        }
    }

    /// The thought arriving ended at `now`: its entry says how long it took.
    pub(crate) fn end_thought(&mut self, now: Instant) {
        self.in_thought = false;
        let Some(open) = self.open_thought.take() else {
            return;
        };
        let Some(entry) = open.entry.and_then(|at| self.entry_mut(at)) else {
            return;
        };
        entry.text.truncate(entry.text.trim_end().len());
        if let Some(started) = open.started {
            let took = now.saturating_duration_since(started).as_millis();
            entry.ms = Some(u64::try_from(took).unwrap_or(u64::MAX));
        }
    }

    fn message_chunk(&mut self, text: &str) {
        if self.in_message
            && let Some(last) = self.transcript.back_mut()
            && last.kind == EntryKind::Agent
        {
            push_cut(&mut last.text, text, MAX_ENTRY);
        } else {
            self.push_entry(EntryKind::Agent, cut(text, MAX_ENTRY));
            self.in_message = true;
            self.info.last_message.clear();
        }
        push_cut(&mut self.info.last_message, text, MAX_LAST_MESSAGE);
    }
}
