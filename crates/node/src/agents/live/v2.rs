//! Bounded, identified ACP v2 transcript projections. Lifecycle belongs to
//! the session actor; presentation snapshots reuse the existing tool renderer.

use super::{
    Live,
    content::{cut, push_cut},
    limits::{MAX_ENTRIES, MAX_ENTRY, MAX_LAST_MESSAGE, MAX_PLAN},
    state::Message,
};
use agent_client_protocol::{schema::MaybeUndefined, v1::schema as v1, v2::schema as v2};
use beep_proto::{
    AgentContext,
    node::agent::{EntryKind, PlanStatus},
};
use serde_json::Value;

impl Live {
    pub(crate) fn v2_submitted(&mut self) {
        self.v2.pending = self
            .dropped
            .checked_add(self.transcript.len() as u64)
            .and_then(|at| at.checked_sub(1));
    }

    pub(crate) fn v2_accepted(&mut self, id: v2::MessageId) {
        if let Some(entry) = self.v2.pending.take() {
            self.v2.messages.insert(
                id,
                Message {
                    entry,
                    optimistic: true,
                },
            );
        }
        let deferred = std::mem::take(&mut self.v2.deferred);
        for update in deferred {
            self.apply_v2(&update);
        }
    }

    pub(crate) fn v2_rejected(&mut self) {
        self.v2.pending = None;
        let deferred = std::mem::take(&mut self.v2.deferred);
        for update in deferred {
            self.apply_v2(&update);
        }
    }

    pub(crate) fn apply_v2(&mut self, update: &v2::SessionUpdate) {
        if self.dropped.saturating_sub(self.v2.pruned) >= 64 {
            self.v2
                .messages
                .retain(|_, message| message.entry >= self.dropped);
            self.v2.tools.retain(|id, _| {
                self.tools
                    .get(&v1::ToolCallId::new(id.to_string()))
                    .is_some_and(|call| call.entry >= self.dropped)
            });
            self.v2
                .terminals
                .retain(|_, (entry, _)| *entry >= self.dropped);
            self.v2.pruned = self.dropped;
        }
        if self.v2.pending.is_some()
            && matches!(
                update,
                v2::SessionUpdate::UserMessage(_) | v2::SessionUpdate::UserMessageChunk(_)
            )
        {
            if self.v2.deferred.len() < MAX_ENTRIES {
                self.v2.deferred.push(update.clone());
            }
            return;
        }
        match update {
            v2::SessionUpdate::AgentMessage(message) => self.v2_message(
                &message.message_id,
                EntryKind::Agent,
                &message.content,
                false,
            ),
            v2::SessionUpdate::AgentThought(message) => self.v2_message(
                &message.message_id,
                EntryKind::Thought,
                &message.content,
                false,
            ),
            v2::SessionUpdate::UserMessage(message) => self.v2_message(
                &message.message_id,
                EntryKind::User,
                &message.content,
                false,
            ),
            v2::SessionUpdate::AgentMessageChunk(chunk) => self.v2_chunk(chunk, EntryKind::Agent),
            v2::SessionUpdate::AgentThoughtChunk(chunk) => self.v2_chunk(chunk, EntryKind::Thought),
            v2::SessionUpdate::UserMessageChunk(chunk) => self.v2_chunk(chunk, EntryKind::User),
            v2::SessionUpdate::ToolCallUpdate(update) => {
                self.v2_tool_room(&update.tool_call_id);
                let snapshot = self
                    .v2
                    .tools
                    .entry(update.tool_call_id.clone())
                    .or_insert_with(|| v2::ToolCallUpdate::new(update.tool_call_id.clone()));
                snapshot.apply_update(update.clone());
                let snapshot = snapshot.clone();
                self.v2_tool(&snapshot);
            }
            v2::SessionUpdate::ToolCallContentChunk(chunk) => {
                self.v2_tool_room(&chunk.tool_call_id);
                let update = self
                    .v2
                    .tools
                    .entry(chunk.tool_call_id.clone())
                    .or_insert_with(|| v2::ToolCallUpdate::new(chunk.tool_call_id.clone()));
                match &mut update.content {
                    MaybeUndefined::Value(content) if content.len() < MAX_PLAN => {
                        content.push(chunk.content.clone())
                    }
                    MaybeUndefined::Value(_) => {}
                    _ => update.content = MaybeUndefined::Value(vec![chunk.content.clone()]),
                }
                let update = update.clone();
                self.v2_tool(&update);
            }
            v2::SessionUpdate::TerminalUpdate(update) => self.v2_terminal(update.clone()),
            v2::SessionUpdate::TerminalOutputChunk(chunk) => {
                let mut update = self
                    .v2
                    .terminals
                    .get(&chunk.terminal_id)
                    .map(|(_, update)| update.clone())
                    .unwrap_or_else(|| v2::TerminalUpdate::new(chunk.terminal_id.clone()));
                use base64::{Engine, engine::general_purpose::STANDARD};
                let Ok(chunk) = STANDARD.decode(&chunk.data) else {
                    return;
                };
                let mut bytes = match update.output {
                    MaybeUndefined::Value(output) => {
                        STANDARD.decode(output.data).unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                bytes.truncate(MAX_ENTRY);
                let remaining = MAX_ENTRY.saturating_sub(bytes.len());
                bytes.extend(chunk.into_iter().take(remaining));
                update.output =
                    MaybeUndefined::Value(v2::TerminalOutput::new(STANDARD.encode(bytes)));
                self.v2_terminal(update);
            }
            v2::SessionUpdate::PlanUpdate(update) => {
                if let v2::PlanUpdateContent::Items(items) = &update.plan {
                    self.set_plan(items.entries.iter().map(|entry| {
                        let status = match entry.status {
                            v2::PlanEntryStatus::InProgress => PlanStatus::InProgress,
                            v2::PlanEntryStatus::Completed => PlanStatus::Completed,
                            _ => PlanStatus::Pending,
                        };
                        (entry.content.as_str(), status)
                    }));
                }
            }
            v2::SessionUpdate::SessionInfoUpdate(_) if self.named => {}
            v2::SessionUpdate::SessionInfoUpdate(update) => match &update.title {
                MaybeUndefined::Value(title) => self.info.title = crate::tags::title(title),
                MaybeUndefined::Null => self.info.title = None,
                _ => {}
            },
            v2::SessionUpdate::UsageUpdate(update) => {
                self.info.context = Some(AgentContext {
                    used: update.used,
                    size: update.size,
                });
                self.usage_update(
                    update
                        .cost
                        .as_opt_ref()
                        .flatten()
                        .map(|cost| (cost.amount, cost.currency.as_str())),
                    update.meta.as_opt_ref().flatten(),
                );
            }
            v2::SessionUpdate::AvailableCommandsUpdate(update) => {
                // Command presentation is shared; unknown v2 command types
                // are omitted rather than represented as executable commands.
                if let Ok(value) = serde_json::to_value(update)
                    && let Ok(update) = serde_json::from_value::<v1::AvailableCommandsUpdate>(value)
                {
                    self.apply(&v1::SessionUpdate::AvailableCommandsUpdate(update));
                }
            }
            v2::SessionUpdate::CompactionUpdate(update) => {
                if let Ok(value) = serde_json::to_value(update)
                    && let Ok(update) = serde_json::from_value::<v1::CompactionUpdate>(value)
                {
                    self.apply(&v1::SessionUpdate::CompactionUpdate(update));
                }
            }
            _ => {}
        }
    }

    fn v2_chunk(&mut self, chunk: &v2::ContentChunk, kind: EntryKind) {
        self.v2_message(
            &chunk.message_id,
            kind,
            &MaybeUndefined::Value(vec![chunk.content.clone()]),
            true,
        );
    }

    fn v2_message(
        &mut self,
        id: &v2::MessageId,
        kind: EntryKind,
        content: &MaybeUndefined<Vec<v2::ContentBlock>>,
        append: bool,
    ) {
        if !self.v2.messages.contains_key(id) {
            let entry = self.push_entry(kind, String::new());
            self.v2.messages.insert(
                id.clone(),
                Message {
                    entry,
                    optimistic: false,
                },
            );
        }
        let Some(message) = self.v2.messages.get_mut(id) else {
            return;
        };
        let entry = message.entry;
        if matches!(content, MaybeUndefined::Undefined) {
            return;
        }
        let optimistic = std::mem::take(&mut message.optimistic);
        let text = match content {
            MaybeUndefined::Undefined => return,
            MaybeUndefined::Null => String::new(),
            MaybeUndefined::Value(content) => {
                let mut text = String::new();
                for block in content {
                    let rendered = match block {
                        v2::ContentBlock::Text(block) => block.text.clone(),
                        v2::ContentBlock::Image(image) => {
                            use base64::{Engine, engine::general_purpose::STANDARD};
                            match STANDARD.decode(&image.data) {
                                Ok(data) => {
                                    let picture = crate::logs::picture::Picture {
                                        mime: image.mime_type.to_string(),
                                        data,
                                    };
                                    let line = picture.line();
                                    self.keep(picture);
                                    format!("\n{line}\n")
                                }
                                Err(_) => String::new(),
                            }
                        }
                        v2::ContentBlock::ResourceLink(link) => {
                            self.link(&link.uri);
                            format!("[{}]({})", link.name, link.uri)
                        }
                        _ => String::new(),
                    };
                    push_cut(&mut text, &rendered, MAX_ENTRY);
                }
                text
            }
        };
        if let Some(item) = self.entry_mut(entry) {
            if append && !optimistic {
                push_cut(&mut item.text, &text, MAX_ENTRY);
            } else {
                item.text = cut(&text, MAX_ENTRY);
            }
            if kind == EntryKind::User {
                item.text = crate::tags::bare_block(&item.text);
            }
        }
        if kind == EntryKind::Agent && self.v2.last_agent.is_none_or(|last| entry >= last) {
            self.v2.last_agent = Some(entry);
            self.info.last_message = self
                .entry_mut(entry)
                .map(|item| cut(&item.text, MAX_LAST_MESSAGE))
                .unwrap_or_default();
        } else if kind == EntryKind::User && self.info.title.is_none() {
            self.info.title = self
                .entry_mut(entry)
                .and_then(|item| crate::tags::title(&item.text));
        }
        self.in_message = false;
    }

    fn v2_tool_room(&mut self, id: &v2::ToolCallId) {
        if self.v2.tools.len() < MAX_ENTRIES || self.v2.tools.contains_key(id) {
            return;
        }
        let oldest = self
            .v2
            .tools
            .keys()
            .min_by_key(|id| {
                self.tools
                    .get(&v1::ToolCallId::new(id.to_string()))
                    .map(|call| call.entry)
            })
            .cloned();
        if let Some(oldest) = oldest {
            self.v2.tools.remove(&oldest);
        }
    }

    fn v2_tool(&mut self, snapshot: &v2::ToolCallUpdate) {
        // Apply v2 patch semantics before rendering a complete tool snapshot.
        // Reusing the renderer does not reinterpret null as an omitted update.
        let Ok(mut value) = serde_json::to_value(snapshot) else {
            return;
        };
        if value.get("title").is_none_or(Value::is_null) {
            value["title"] = Value::String(String::new());
        }
        let Ok(mut call) = serde_json::from_value::<v1::ToolCall>(value) else {
            return;
        };
        if let MaybeUndefined::Value(content) = &snapshot.content {
            call.content.clear();
            for item in content {
                if let v2::ToolCallContent::Diff(diff) = item {
                    for change in &diff.changes {
                        let path = match &change.operation {
                            v2::DiffChangeOperation::Add(change)
                            | v2::DiffChangeOperation::Delete(change)
                            | v2::DiffChangeOperation::Modify(change) => &change.path,
                            v2::DiffChangeOperation::Move(change)
                            | v2::DiffChangeOperation::Copy(change) => &change.path,
                            _ => continue,
                        };
                        call.locations
                            .push(v1::ToolCallLocation::new(path.0.clone()));
                    }
                    if let Some(patch) = &diff.patch {
                        call.content
                            .push(v1::ToolCallContent::Content(v1::Content::new(
                                v1::ContentBlock::Text(v1::TextContent::new(cut(
                                    &patch.text,
                                    MAX_ENTRY,
                                ))),
                            )));
                    }
                } else if let Ok(value) = serde_json::to_value(item)
                    && let Ok(content) = serde_json::from_value(value)
                {
                    call.content.push(content);
                }
            }
        }
        self.tool_call(&call);
        self.count_subagents();
    }

    fn v2_terminal(&mut self, update: v2::TerminalUpdate) {
        let id = update.terminal_id.clone();
        if !self.v2.terminals.contains_key(&id) {
            if self.v2.terminals.len() >= MAX_ENTRIES {
                let oldest = self
                    .v2
                    .terminals
                    .iter()
                    .min_by_key(|(_, (entry, _))| *entry)
                    .map(|(id, _)| id.clone());
                if let Some(oldest) = oldest {
                    self.v2.terminals.remove(&oldest);
                }
            }
            let entry = self.push_entry(EntryKind::Tool, String::new());
            self.v2
                .terminals
                .insert(id.clone(), (entry, v2::TerminalUpdate::new(id.clone())));
        }
        let Some((entry, snapshot)) = self.v2.terminals.get_mut(&id) else {
            return;
        };
        snapshot.apply_update(update);
        let entry = *entry;
        let mut text = String::new();
        if let MaybeUndefined::Value(command) = &snapshot.command {
            push_cut(&mut text, command, MAX_ENTRY);
            text.push('\n');
        }
        if let MaybeUndefined::Value(output) = &snapshot.output {
            use base64::{Engine, engine::general_purpose::STANDARD};
            if let Ok(bytes) = STANDARD.decode(&output.data) {
                push_cut(&mut text, &String::from_utf8_lossy(&bytes), MAX_ENTRY);
            }
        }
        if let Some(entry) = self.entry_mut(entry) {
            entry.text = text;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/agents_live_v2.rs"]
mod tests;
