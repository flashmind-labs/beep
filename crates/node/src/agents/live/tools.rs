//! Tool calls, the subagents they start and what they show.

use super::{
    ARTIFACT_TOOL, EDIT_KINDS, Live, MAX_ENTRY, MAX_FILES, MAX_LINE, MAX_STEPS,
    content::{cut, image_picture, parent_tool, picture_of},
};
use crate::agents::mcp::calls::Tool as BeepTool;
use crate::agents::{
    acp::wire_name,
    images::Picture,
    subagent::{Activity, Run, Subagent},
    tool_diff::diff_text,
};
use agent_client_protocol::schema::v1::{
    ContentBlock, EmbeddedResourceResource, ToolCall as AcpToolCall, ToolCallContent, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolKind,
};
use beep_proto::node::agent::EntryKind;
use serde_json::Value;
use std::{borrow::Cow, path::Path};

#[derive(Debug)]
pub(super) struct ToolCall {
    /// Its entry, counted from the start of the session.
    pub(super) entry: u64,
    pub(super) title: String,
    pub(super) kind: ToolKind,
    pub(super) status: ToolCallStatus,
    pub(super) paths: Vec<String>,
    /// What it showed, already cut to an entry's length.
    pub(super) output: String,
    /// The image lines of the pictures it showed.
    pub(super) pictures: Vec<String>,
    /// The subagent's tool call this one ran in, whose entry shows it.
    pub(super) parent: Option<ToolCallId>,
    /// The calls a subagent this tool started ran, in order.
    pub(super) steps: Vec<ToolCallId>,
    /// The subagent it started, when the call says it starts one.
    pub(super) subagent: Option<Subagent>,
    /// A call to the node's own `beep` server, titled from its arguments.
    pub(super) beep: Option<BeepCall>,
}

/// A `beep` server call and its arguments as far as they have come.
#[derive(Debug)]
pub(super) struct BeepCall {
    pub(super) tool: BeepTool,
    pub(super) input: Option<Value>,
}

/// A Codex subagent among a session's entries, whose steps are read from
/// its rollout.
#[derive(Debug, Clone)]
pub(crate) struct CodexChild {
    /// The call that started it.
    pub(crate) call: ToolCallId,
    pub(crate) threads: Vec<String>,
}

impl Live {
    pub(super) fn tool_call(&mut self, call: &AcpToolCall) {
        self.in_message = false;
        // Codex saying a subagent it started began or ended goes on the
        // call that started it.
        if let Some(activity) = call.raw_input.as_ref().and_then(Activity::of)
            && let Some(owner) = self.codex_threads.get(&activity.thread).cloned()
        {
            self.codex_activity(&owner, activity);
            return;
        }
        // A subagent's own call goes on the entry of the call that started
        // it, not in the transcript.
        let parent = parent_tool(call.meta.as_ref()).filter(|id| self.tools.contains_key(id));
        let mut subagent = Subagent::of_call(&call.title, call.raw_input.as_ref());
        let mut tool = ToolCall {
            entry: 0,
            title: call.title.clone(),
            kind: call.kind,
            status: call.status,
            paths: paths(&call.locations, call.raw_input.as_ref()),
            output: String::new(),
            pictures: Vec::new(),
            parent: parent.clone(),
            steps: Vec::new(),
            subagent: None,
            beep: BeepTool::of_call(&call.title, call.meta.as_ref(), call.raw_input.as_ref()).map(
                |tool| BeepCall {
                    tool,
                    input: call.raw_input.clone(),
                },
            ),
        };
        if let Some(subagent) = &mut subagent {
            if let Some(title) = subagent.title.take() {
                tool.title = title;
            }
            for thread in &subagent.threads {
                self.codex_threads
                    .insert(thread.clone(), call.tool_call_id.clone());
            }
        }
        tool.subagent = subagent;
        let pictures = self.tool_output(&mut tool, &call.content);
        self.info.state.summary = cut(&tool.own_title(), MAX_LINE);
        self.finish_tool(&tool);
        match parent {
            Some(parent) => {
                if let Some(owner) = self.tools.get_mut(&parent) {
                    tool.entry = owner.entry;
                    owner.steps.push(call.tool_call_id.clone());
                }
                self.tools.insert(call.tool_call_id.clone(), tool);
                self.write_tool(&parent);
            }
            None => {
                tool.entry = self.push_entry(EntryKind::Tool, tool.entry_text());
                self.tools.insert(call.tool_call_id.clone(), tool);
            }
        }
        self.tool_images(&pictures);
    }

    /// Codex's report that the subagent `owner` started began or ended:
    /// its task names it.
    fn codex_activity(&mut self, owner: &ToolCallId, activity: Activity) {
        if let Some(tool) = self.tools.get_mut(owner) {
            tool.title = activity.task;
            if let Some(subagent) = &mut tool.subagent {
                subagent.ended |= activity.ended;
            }
        }
        self.write_tool(owner);
    }

    /// The calls the subagent `tool` started ran, as its entry names them.
    pub(super) fn steps_of<'a>(&'a self, tool: &'a ToolCall) -> Vec<Step<'a>> {
        tool.steps
            .iter()
            .filter_map(|step| self.tools.get(step))
            .map(|step| Step {
                status: step.status,
                title: &step.title,
            })
            .collect()
    }

    /// Writes tool `id`'s entry again: its own text, and the steps of the
    /// subagent it started.
    fn write_tool(&mut self, id: &ToolCallId) {
        let Some(tool) = self.tools.get(id) else {
            return;
        };
        let text = tool.entry_with(&self.steps_of(tool), None);
        let entry = tool.entry;
        if let Some(entry) = self.entry_mut(entry) {
            entry.text = text;
        }
    }

    pub(super) fn tool_call_update(&mut self, update: &ToolCallUpdate) {
        let Some(mut call) = self.tools.remove(&update.tool_call_id) else {
            return;
        };
        let fields = &update.fields;
        let codex = call.subagent.as_ref().is_some_and(|sub| sub.codex);
        if let Some(title) = &fields.title {
            // A Codex subagent keeps the title its task or prompt gave it.
            if !codex {
                call.title.clone_from(title);
            }
            self.info.state.summary = cut(title, MAX_LINE);
        }
        // Claude Code sends a call's arguments once they have all come.
        if call.beep.is_none() {
            call.beep =
                BeepTool::of_call(&call.title, update.meta.as_ref(), fields.raw_input.as_ref())
                    .map(|tool| BeepCall { tool, input: None });
        }
        if let Some(beep) = &mut call.beep {
            if let Some(input) = &fields.raw_input {
                beep.input = Some(input.clone());
            }
            self.info.state.summary = cut(&call.own_title(), MAX_LINE);
        }
        if let Some(subagent) = call.subagent.as_mut().filter(|sub| sub.codex) {
            subagent.update(fields.raw_input.as_ref());
            for thread in &subagent.threads {
                self.codex_threads
                    .insert(thread.clone(), update.tool_call_id.clone());
            }
        }
        if let Some(kind) = fields.kind {
            call.kind = kind;
        }
        if let Some(status) = fields.status {
            call.status = status;
        }
        let more = paths(
            fields.locations.as_deref().unwrap_or_default(),
            fields.raw_input.as_ref(),
        );
        if !more.is_empty() {
            call.paths = more;
        }
        let pictures = match &fields.content {
            Some(content) => self.tool_output(&mut call, content),
            None => Vec::new(),
        };
        self.finish_tool(&call);
        let shown = call
            .parent
            .clone()
            .unwrap_or_else(|| update.tool_call_id.clone());
        self.tools.insert(update.tool_call_id.clone(), call);
        self.write_tool(&shown);
        self.tool_images(&pictures);
    }

    /// Sets what `tool` showed: its text, and a line for each picture,
    /// kept for the server. Returns the pictures, for `tool_images`.
    fn tool_output(&mut self, tool: &mut ToolCall, content: &[ToolCallContent]) -> Vec<Picture> {
        let shown = output(content, Path::new(&self.info.cwd));
        tool.output = shown.text;
        tool.pictures = shown.pictures.iter().map(Picture::line).collect();
        for picture in &shown.pictures {
            self.keep(picture.clone());
        }
        for link in &shown.links {
            self.link(link);
        }
        shown.pictures
    }

    /// Notes the files a finished edit changed.
    fn finish_tool(&mut self, tool: &ToolCall) {
        if tool.status != ToolCallStatus::Completed || !EDIT_KINDS.contains(&tool.kind) {
            return;
        }
        let cwd = Path::new(&self.info.cwd);
        let files = &mut self.info.state.files_touched;
        for path in &tool.paths {
            let shown = Path::new(path)
                .strip_prefix(cwd)
                .map_or_else(|_| path.clone(), |rel| rel.display().to_string());
            files.retain(|file| *file != shown);
            files.push(shown);
        }
        let extra = files.len().saturating_sub(MAX_FILES);
        files.drain(..extra);
    }
}

/// One call a subagent ran, as its parent's entry names it.
pub(super) struct Step<'a> {
    pub(super) status: ToolCallStatus,
    pub(super) title: &'a str,
}

impl ToolCall {
    /// Its title and status, what it showed, then its image lines whole.
    fn entry_text(&self) -> String {
        self.entry_with(&[], None)
    }

    /// How the call stands as its entry shows it: a Codex subagent works
    /// until it says it ended, or its rollout says it finished.
    fn shown_status(&self, run: Option<&Run>) -> ToolCallStatus {
        let codex = self.subagent.as_ref().filter(|sub| sub.codex);
        if self.status == ToolCallStatus::Failed || codex.is_none() {
            return self.status;
        }
        let working = match run {
            Some(run) => run.working,
            None => codex.is_some_and(|sub| !sub.ended),
        };
        if working {
            ToolCallStatus::InProgress
        } else {
            ToolCallStatus::Completed
        }
    }

    /// Its title as its entry shows it. Claude Code's `Artifact` call comes
    /// titled by the tool's name alone, so the page it publishes is added
    /// for the apps' Artifacts tab: `Artifact /srv/project/report.html`.
    fn own_title(&self) -> Cow<'_, str> {
        if let Some(beep) = &self.beep {
            return Cow::Owned(beep.tool.title(beep.input.as_ref()));
        }
        match self.paths.first() {
            Some(path) if self.title == ARTIFACT_TOOL => {
                Cow::Owned(format!("{} {path}", self.title))
            }
            _ => Cow::Borrowed(&self.title),
        }
    }

    /// Its entry with the steps of the subagent it started: a line
    /// `[steps] N`, then `[step <status>] <title>` for each of the last
    /// `MAX_STEPS`, then `[agent] <name>` when it has one, before what it
    /// showed. A Codex subagent's steps and answer are `run`'s, read from
    /// its rollout.
    pub(super) fn entry_with(&self, steps: &[Step<'_>], run: Option<&Run>) -> String {
        let mut lines = String::new();
        for line in &self.pictures {
            lines.push('\n');
            lines.push_str(line);
        }
        let status = self.shown_status(run);
        let own = self.own_title();
        let title = run.and_then(|run| run.task.as_deref()).unwrap_or(&own);
        let mut text = format!("{title} [{}]", wire_name(&status));
        let run_steps: Vec<Step<'_>> = run
            .map(|run| {
                run.steps
                    .iter()
                    .map(|step| Step {
                        status: if step.done || status != ToolCallStatus::InProgress {
                            ToolCallStatus::Completed
                        } else {
                            ToolCallStatus::InProgress
                        },
                        title: &step.title,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (steps, count) = match run {
            Some(run) => (run_steps.as_slice(), run.count),
            None => (steps, steps.len()),
        };
        if !steps.is_empty() || self.subagent.is_some() {
            text.push_str(&format!("\n[steps] {count}"));
            for step in &steps[steps.len().saturating_sub(MAX_STEPS)..] {
                let title = step.title.lines().next().unwrap_or_default();
                text.push_str(&format!(
                    "\n[step {}] {}",
                    wire_name(&step.status),
                    cut(title, MAX_LINE)
                ));
            }
            let name = run
                .and_then(|run| run.name.as_deref())
                .or(self.subagent.as_ref().and_then(|sub| sub.name.as_deref()));
            if let Some(name) = name {
                text.push_str(&format!("\n[agent] {name}"));
            }
        }
        let output = run
            .and_then(|run| run.result.as_deref())
            .unwrap_or(&self.output);
        if !output.is_empty() {
            text.push('\n');
            text.push_str(output);
        }
        let mut text = cut(&text, MAX_ENTRY.saturating_sub(lines.len()));
        text.push_str(&lines);
        text
    }
}

/// What a tool call's `content` shows.
pub(super) struct Output {
    /// Its text, cut to an entry's length.
    pub(super) text: String,
    /// The images among it.
    pub(super) pictures: Vec<Picture>,
    /// The files it links to.
    pub(super) links: Vec<String>,
}

/// What a tool call's `content` shows: its text, images and links. An
/// edit's diff shows its changed lines, its path relative to `cwd`.
pub(super) fn output(content: &[ToolCallContent], cwd: &Path) -> Output {
    let mut pictures = Vec::new();
    let mut links = Vec::new();
    let text = content
        .iter()
        .filter_map(|item| match item {
            ToolCallContent::Content(content) => match &content.content {
                ContentBlock::Text(text) => Some(text.text.clone()),
                ContentBlock::Image(image) => {
                    pictures.extend(image_picture(image));
                    None
                }
                ContentBlock::Resource(resource) => {
                    if let EmbeddedResourceResource::BlobResourceContents(blob) = &resource.resource
                    {
                        let mime = blob.mime_type.as_deref().unwrap_or("");
                        pictures.extend(picture_of(mime, &blob.blob));
                    }
                    None
                }
                ContentBlock::ResourceLink(link) => {
                    links.push(link.uri.clone());
                    None
                }
                _ => None,
            },
            ToolCallContent::Diff(diff) => Some(diff_text(
                cwd,
                &diff.path,
                diff.old_text.as_deref(),
                &diff.new_text,
            )),
            ToolCallContent::Terminal(_) => Some("(ran in a terminal)".into()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    Output {
        text: cut(&text, MAX_ENTRY),
        pictures,
        links,
    }
}

/// The files a tool call names, in `locations` or its input.
pub(super) fn paths(locations: &[ToolCallLocation], input: Option<&Value>) -> Vec<String> {
    let mut found: Vec<String> = locations
        .iter()
        .map(|location| location.path.display().to_string())
        .collect();
    if found.is_empty()
        && let Some(input) = input
    {
        found.extend(
            ["file_path", "path", "notebook_path"]
                .iter()
                .find_map(|key| input[key].as_str())
                .map(str::to_owned),
        );
    }
    found
}
