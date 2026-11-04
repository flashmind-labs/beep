//! The research team's tools: the room, notes, sources and draft.

use super::{Entry, Member};
use crate::store::{NewSource, Store};
use anyhow::{Result, bail};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult, parse_args};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, sync::Arc};
use uuid::Uuid;

/// Room messages `read_room` returns at most, newest kept.
const ROOM_READ: usize = 40;
/// The longest message, note or quote a member may write.
const MAX_TEXT: usize = 4000;
const MAX_DRAFT: usize = 60_000;

/// Who is calling, in which research.
#[derive(Clone)]
pub(crate) struct Seat {
    pub store: Store,
    pub research: Uuid,
    pub member: Member,
}

/// The tools a seat adds, so the runner does not log their calls as steps.
pub(crate) const TEAM_TOOLS: &[&str] = &[
    "post_message",
    "read_room",
    "add_source",
    "add_note",
    "read_notes",
    "read_draft",
    "write_draft",
];

/// A member's tools for its role.
pub(crate) struct TeamTools(pub Vec<Arc<dyn Tool>>);

pub(crate) fn team_tools(seat: Seat) -> TeamTools {
    let seat = Arc::new(seat);
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(PostMessage(seat.clone())),
        Arc::new(ReadRoom(seat.clone())),
        Arc::new(ReadNotes(seat.clone())),
    ];
    match seat.member {
        Member::Scout(_) => {
            tools.push(Arc::new(AddSource(seat.clone())));
            tools.push(Arc::new(AddNote(seat)));
        }
        Member::Critic => tools.push(Arc::new(ReadDraft(seat))),
        Member::Writer => {
            tools.push(Arc::new(ReadDraft(seat.clone())));
            tools.push(Arc::new(WriteDraft(seat)));
        }
        Member::Lead => tools.push(Arc::new(ReadDraft(seat))),
    }
    TeamTools(tools)
}

fn check_text(what: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > MAX_TEXT {
        bail!("{what} must be 1-{MAX_TEXT} bytes");
    }
    Ok(())
}

struct PostMessage(Arc<Seat>);

#[derive(Deserialize)]
struct PostArgs {
    text: String,
    to: Option<String>,
}

#[async_trait]
impl Tool for PostMessage {
    fn name(&self) -> &str {
        "post_message"
    }
    fn description(&self) -> &str {
        "Post a short message to the team's room. Name one agent in `to` (like `scout-2`, \
         `critic`, `writer`) to address them, or leave it out for everyone. Use it for \
         findings others need, questions to another agent and answers to challenges."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "to": { "type": "string", "description": "An agent id, or omit for everyone" }
            },
            "required": ["text"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: PostArgs = parse_args(self.name(), ctx.args)?;
        check_text("text", &args.text)?;
        let to = match args.to.as_deref().map(str::trim) {
            None | Some("" | "everyone" | "all") => None,
            Some(to) => match to.parse::<Member>() {
                Ok(to) => Some(to),
                Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
            },
        };
        let entry = Entry::Said {
            to,
            content: args.text.trim().to_owned(),
        };
        let seat = &self.0;
        seat.store
            .add_research_entry(seat.research, seat.member, &entry)
            .await?;
        Ok(ToolResult::success(ctx.tool_call_id, "Posted."))
    }
}

struct ReadRoom(Arc<Seat>);

#[async_trait]
impl Tool for ReadRoom {
    fn name(&self) -> &str {
        "read_room"
    }
    fn description(&self) -> &str {
        "Read the latest messages in the team's room."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let entries = self.0.store.research_entries(self.0.research, 0).await?;
        let said: Vec<String> = entries
            .iter()
            .filter_map(|row| match &row.entry {
                Entry::Said {
                    to: Some(to),
                    content,
                } => Some(format!("{} to {to}: {content}", row.member)),
                Entry::Said { to: None, content } => Some(format!("{}: {content}", row.member)),
                _ => None,
            })
            .collect();
        if said.is_empty() {
            return Ok(ToolResult::success(ctx.tool_call_id, "The room is empty."));
        }
        let latest = &said[said.len().saturating_sub(ROOM_READ)..];
        Ok(ToolResult::success(ctx.tool_call_id, latest.join("\n")))
    }
}

struct AddSource(Arc<Seat>);

#[derive(Deserialize)]
struct SourceArgs {
    url: String,
    title: String,
    quote: String,
}

#[async_trait]
impl Tool for AddSource {
    fn name(&self) -> &str {
        "add_source"
    }
    fn description(&self) -> &str {
        "Keep a source you read, with the exact passage that supports a finding. Returns its \
         number; cite that number in add_note. A URL kept before returns its old number."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "title": { "type": "string" },
                "quote": { "type": "string", "description": "The passage, copied from the page" }
            },
            "required": ["url", "title", "quote"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: SourceArgs = parse_args(self.name(), ctx.args)?;
        if !args.url.starts_with("http://") && !args.url.starts_with("https://") {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "url must be a web address",
            ));
        }
        check_text("title", &args.title)?;
        check_text("quote", &args.quote)?;
        let source = NewSource {
            url: &args.url,
            title: &args.title,
            quote: &args.quote,
        };
        let seat = &self.0;
        let number = seat
            .store
            .add_research_source(seat.research, seat.member, &source)
            .await?;
        Ok(ToolResult::success(
            ctx.tool_call_id,
            format!("Kept as source {number}."),
        ))
    }
}

struct AddNote(Arc<Seat>);

#[derive(Deserialize)]
struct NoteArgs {
    text: String,
    #[serde(default)]
    sources: Vec<u32>,
}

#[async_trait]
impl Tool for AddNote {
    fn name(&self) -> &str {
        "add_note"
    }
    fn description(&self) -> &str {
        "Add one finding to the team's notes, citing the source numbers that back it. One \
         claim per note, in a sentence or two. The writer builds the paper from these."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "sources": { "type": "array", "items": { "type": "integer" } }
            },
            "required": ["text"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: NoteArgs = parse_args(self.name(), ctx.args)?;
        check_text("text", &args.text)?;
        let seat = &self.0;
        let known = seat.store.research_sources(seat.research).await?.len();
        if let Some(bad) = args
            .sources
            .iter()
            .find(|&&n| n == 0 || usize::try_from(n).map_or(true, |n| n > known))
        {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("There is no source {bad}. Keep it with add_source first."),
            ));
        }
        let entry = Entry::Note {
            content: args.text.trim().to_owned(),
            sources: args.sources,
        };
        seat.store
            .add_research_entry(seat.research, seat.member, &entry)
            .await?;
        Ok(ToolResult::success(ctx.tool_call_id, "Noted."))
    }
}

struct ReadNotes(Arc<Seat>);

#[async_trait]
impl Tool for ReadNotes {
    fn name(&self) -> &str {
        "read_notes"
    }
    fn description(&self) -> &str {
        "Read every note the scouts made, with the sources they cite."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let seat = &self.0;
        let entries = seat.store.research_entries(seat.research, 0).await?;
        let sources = seat.store.research_sources(seat.research).await?;
        Ok(ToolResult::success(
            ctx.tool_call_id,
            notes_text(&entries, &sources),
        ))
    }
}

/// The notes and sources as a member reads them.
pub(crate) fn notes_text(
    entries: &[crate::store::EntryRow],
    sources: &[crate::store::SourceRow],
) -> String {
    let mut out = String::from("Notes:\n");
    let mut any = false;
    for row in entries {
        if let Entry::Note { content, sources } = &row.entry {
            any = true;
            let cited: Vec<String> = sources.iter().map(u32::to_string).collect();
            let _ = writeln!(out, "- ({}) {content} [{}]", row.member, cited.join(", "));
        }
    }
    if !any {
        out.push_str("(none yet)\n");
    }
    out.push_str("\nSources:\n");
    for source in sources {
        let _ = writeln!(
            out,
            "[{}] {} <{}>\n    \"{}\"",
            source.number, source.title, source.url, source.quote
        );
    }
    out
}

struct ReadDraft(Arc<Seat>);

#[async_trait]
impl Tool for ReadDraft {
    fn name(&self) -> &str {
        "read_draft"
    }
    fn description(&self) -> &str {
        "Read the writer's current draft of the paper."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let row = self.0.store.research(self.0.research).await?;
        let draft = row.map(|row| row.draft).unwrap_or_default();
        Ok(ToolResult::success(
            ctx.tool_call_id,
            if draft.is_empty() {
                "There is no draft yet.".to_owned()
            } else {
                draft
            },
        ))
    }
}

struct WriteDraft(Arc<Seat>);

#[derive(Deserialize)]
struct DraftArgs {
    markdown: String,
}

#[async_trait]
impl Tool for WriteDraft {
    fn name(&self) -> &str {
        "write_draft"
    }
    fn description(&self) -> &str {
        "Replace the paper's draft with this Markdown. Write the whole paper each time."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "markdown": { "type": "string" } },
            "required": ["markdown"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: DraftArgs = parse_args(self.name(), ctx.args)?;
        if args.markdown.trim().is_empty() || args.markdown.len() > MAX_DRAFT {
            bail!("the draft must be 1-{MAX_DRAFT} bytes");
        }
        self.0
            .store
            .set_research_draft(self.0.research, args.markdown.trim())
            .await?;
        Ok(ToolResult::success(ctx.tool_call_id, "Saved the draft."))
    }
}
