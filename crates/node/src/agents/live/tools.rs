//! Tool calls, the subagents they start and what they show.

use super::{
    ARTIFACT_TOOL, EDIT_KINDS, Live, MAX_ENTRY, MAX_FILES, MAX_LINE, MAX_STEPS, SKILL_TOOL,
    content::{cut, image_picture, parent_tool},
};
use crate::agents::mcp::calls::Tool as BeepTool;
use crate::agents::place::call_folder;
use crate::agents::{
    acp::wire_name,
    picture::{Picture, picture_of},
    subagent::{Activity, Conversation, Run, Subagent},
    tool_details,
    tool_diff::diff_text,
};
use agent_client_protocol::v1::schema::{
    ContentBlock, EmbeddedResourceResource, ToolCall as AcpToolCall, ToolCallContent, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolKind,
};
use beep_proto::{AgentToolCall, node::agent::EntryKind};
use serde_json::{Map, Value};
use std::{borrow::Cow, path::Path};

#[derive(Debug, Default)]
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
    /// Its command went to the background and still runs, though the call
    /// itself completed.
    pub(super) backgrounded: bool,
    /// The skill a Claude Code `Skill` call loads.
    pub(super) skill: Option<String>,
    /// How far the files a `send_files` call sends have gone.
    pub(super) sent: Option<Sent>,
    pub(super) details: AgentToolCall,
}

/// The bytes a `send_files` call has sent, of all it sends. Its entry
/// shows them as `[sent] <done> <total>` while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sent {
    pub(crate) done: u64,
    pub(crate) total: u64,
}

/// The name a call carries: its own, else the one in its metadata.
fn tool_name(name: Option<&str>, meta: Option<&Map<String, Value>>) -> Option<String> {
    name.filter(|name| !name.is_empty())
        .map(|name| cut(name, 240))
        .or_else(|| tool_details::name(meta))
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
    /// Recover omitted spawns once, retaining their transcript positions on later reads.
    pub(crate) fn recover_codex_children(&mut self, runs: &[Run]) -> bool {
        // A synthetic tool inserted between message chunks would split the reply.
        // The next tool or completed turn allows discovery to catch up.
        if self.in_message && self.info.state.status == beep_proto::node::agent::Status::Working {
            return false;
        }

        if self.tools.values().any(|tool| {
            tool.subagent
                .as_ref()
                .is_some_and(|sub| sub.codex && sub.threads.is_empty())
                && matches!(
                    tool.status,
                    ToolCallStatus::Pending | ToolCallStatus::InProgress
                )
        }) {
            return false;
        }

        let mut changed = false;
        for run in runs {
            let Some(session) = run.session.as_ref() else {
                continue;
            };
            let Ok(Conversation::Codex(thread)) = Conversation::try_from(session.as_ref()) else {
                continue;
            };
            if self.codex_threads.contains_key(thread) {
                continue;
            }
            let saved = self.transcript.iter().position(|entry| {
                entry.kind == EntryKind::Tool
                    && entry
                        .text
                        .lines()
                        .any(|line| line == format!("[session] {session}"))
            });
            let entry = match saved {
                Some(at) => self.dropped + at as u64,
                None => self.push_entry(EntryKind::Tool, run.entry().text),
            };
            let id = ToolCallId::new(format!("recovered:{session}"));
            self.tools.insert(
                id.clone(),
                ToolCall {
                    entry,
                    title: run.task.clone().unwrap_or_else(|| "Subagent".to_owned()),
                    status: ToolCallStatus::Completed,
                    subagent: Some(Subagent {
                        threads: vec![thread.to_owned()],
                        ended: !run.working,
                        codex: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            );
            self.codex_threads.insert(thread.to_owned(), id);
            changed = true;
        }

        if changed {
            self.count_subagents();
        }

        changed
    }

    /// Counts the subagents its calls started that still run, as their
    /// entries show them.
    pub(super) fn count_subagents(&mut self) {
        let running = self
            .tools
            .values()
            .filter(|tool| {
                tool.parent.is_none()
                    && tool.subagent.is_some()
                    && matches!(
                        tool.shown_status(None),
                        ToolCallStatus::Pending | ToolCallStatus::InProgress
                    )
            })
            .count();
        self.info.state.subagents = u32::try_from(running).unwrap_or(u32::MAX);
    }

    /// The turn was cancelled: the adapter never closes the subagent calls
    /// it interrupted, so they end here and stop counting as running. Codex
    /// subagents report their own end, and background ones follow their
    /// task.
    pub(crate) fn close_interrupted_subagents(&mut self) {
        let open: Vec<ToolCallId> = self
            .tools
            .iter()
            .filter(|(_, tool)| {
                tool.parent.is_none()
                    && tool.subagent.as_ref().is_some_and(|sub| !sub.codex)
                    && matches!(
                        tool.status,
                        ToolCallStatus::Pending | ToolCallStatus::InProgress
                    )
            })
            .map(|(id, _)| id.clone())
            .collect();

        self.orphaned_subagents |= !open.is_empty();

        for id in &open {
            if let Some(tool) = self.tools.get_mut(id) {
                tool.status = ToolCallStatus::Failed;
            }
            self.write_tool(id);
        }

        self.count_subagents();
    }

    pub(super) fn tool_call(&mut self, call: &AcpToolCall) {
        // Codex saying a subagent it started began or ended goes on the
        // call that started it.
        if let Some(activity) = call
            .raw_input
            .as_ref()
            .and_then(|input| Activity::try_from(input).ok())
            && let Some(owner) = self.codex_threads.get(&activity.thread).cloned()
        {
            self.codex_activity(&owner, activity);
            return;
        }
        // A subagent's own call goes on the entry of the call that started
        // it, not in the transcript.
        let parent = parent_tool(call.meta.as_ref()).filter(|id| self.tools.contains_key(id));
        self.note_folder(call.kind, &call.locations, call.raw_input.as_ref());
        let name = tool_name(call.name.as_deref(), call.meta.as_ref());
        let mut subagent = Subagent::of_call(name.as_deref(), &call.title, call.raw_input.as_ref());
        let mut tool = ToolCall {
            title: call.title.clone(),
            kind: call.kind,
            status: call.status,
            paths: paths(&call.locations, call.raw_input.as_ref()),
            parent: parent.clone(),
            beep: BeepTool::of_call(&call.title, call.meta.as_ref(), call.raw_input.as_ref()).map(
                |tool| BeepCall {
                    tool,
                    input: call.raw_input.clone(),
                },
            ),
            details: AgentToolCall {
                name,
                ..AgentToolCall::default()
            },
            ..Default::default()
        };
        update_details(
            &mut tool.details,
            call.raw_input.as_ref(),
            call.raw_output.as_ref(),
        );

        tool.skill = skill_of(&tool.details, call.raw_input.as_ref());

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
                    if !owner.steps.contains(&call.tool_call_id) {
                        owner.steps.push(call.tool_call_id.clone());
                    }
                }
                self.tools.insert(call.tool_call_id.clone(), tool);
                self.write_tool(&parent);
            }
            None => {
                if let Some(previous) = self.tools.get(&call.tool_call_id) {
                    tool.entry = previous.entry;
                    tool.steps.clone_from(&previous.steps);
                } else {
                    // Only a call with an entry of its own ends the message
                    // being written; a subagent's call arrives in the middle.
                    self.in_message = false;
                    tool.entry = self.push_entry(EntryKind::Tool, tool.entry_with(&[], None));
                }
                if let Some(entry) = self.entry_mut(tool.entry) {
                    entry.tool = Some(tool.details.clone());
                }
                self.tools.insert(call.tool_call_id.clone(), tool);
                self.write_tool(&call.tool_call_id);
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

    /// Shows how far the running `send_files` call has sent its files: the
    /// latest one, as one session sends one batch at a time. True when its
    /// entry changed.
    pub(crate) fn sending(&mut self, sent: Sent) -> bool {
        let call = self
            .tools
            .iter()
            .filter(|(_, tool)| tool.status == ToolCallStatus::InProgress)
            .filter(|(_, tool)| {
                tool.beep.as_ref().is_some_and(|beep| {
                    beep.tool.resolved(beep.input.as_ref()) == BeepTool::SendFiles
                })
            })
            .max_by_key(|(_, tool)| tool.entry)
            .map(|(id, _)| id.clone());
        let Some(id) = call else {
            return false;
        };
        let Some(tool) = self.tools.get_mut(&id) else {
            return false;
        };

        if tool.sent == Some(sent) {
            return false;
        }

        tool.sent = Some(sent);
        self.write_tool(&id);
        true
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
    pub(super) fn write_tool(&mut self, id: &ToolCallId) {
        let Some(tool) = self.tools.get(id) else {
            return;
        };
        let text = tool.entry_with(&self.steps_of(tool), None);
        let entry = tool.entry;
        let details = tool.details.clone();
        if let Some(entry) = self.entry_mut(entry) {
            entry.text = text;
            entry.tool = Some(details);
        }
    }

    /// Notes the folder a tool call works in, when it names one and it is
    /// not the last: the session then looks up its checkout.
    fn note_folder(
        &mut self,
        kind: ToolKind,
        locations: &[ToolCallLocation],
        input: Option<&Value>,
    ) {
        let edited = locations
            .first()
            .filter(|_| EDIT_KINDS.contains(&kind))
            .map(|location| location.path.as_path());
        let base = Path::new(&self.info.cwd);
        let Some(dir) = call_folder(base, input, edited) else {
            return;
        };
        if self.folder.as_deref() != Some(dir.as_path()) {
            self.folder = Some(dir.clone());
            self.place_wanted = Some(dir);
        }
    }

    pub(super) fn tool_call_update(&mut self, update: &ToolCallUpdate) {
        let Some(mut call) = self.tools.remove(&update.tool_call_id) else {
            return;
        };
        let fields = &update.fields;

        if let Some(name) = tool_name(fields.name.as_deref(), update.meta.as_ref()) {
            call.details.name = Some(name);
        }

        update_details(
            &mut call.details,
            fields.raw_input.as_ref(),
            fields.raw_output.as_ref(),
        );
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

        if call.skill.is_none() {
            call.skill = skill_of(&call.details, fields.raw_input.as_ref());
        }

        if call.subagent.is_none() {
            call.subagent = Subagent::of_call(
                call.details.name.as_deref(),
                &call.title,
                fields.raw_input.as_ref(),
            );
        }

        if let Some(subagent) = call.subagent.as_mut()
            && subagent.codex
        {
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
        self.mark_backgrounded(&mut call, &update.tool_call_id, update.meta.as_ref());
        self.note_folder(
            call.kind,
            fields.locations.as_deref().unwrap_or_default(),
            fields.raw_input.as_ref(),
        );
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
    /// kept for the caller. Returns the pictures, for `tool_images`.
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

/// What the result of a skill that forked to the background says.
const FORKED_SKILL: &str = "forked execution";

/// The skill a call loads, when it is Claude Code's `Skill` call.
fn skill_of(details: &AgentToolCall, input: Option<&Value>) -> Option<String> {
    if details.name.as_deref() != Some(SKILL_TOOL) {
        return None;
    }

    let skill = input?.get("skill")?.as_str()?.trim();
    (!skill.is_empty()).then(|| cut(skill, MAX_LINE))
}

/// One call a subagent ran, as its parent's entry names it.
pub(super) struct Step<'a> {
    pub(super) status: ToolCallStatus,
    pub(super) title: &'a str,
}

impl ToolCall {
    /// How the call stands as its entry shows it: a Codex subagent works
    /// until it says it ended, or its rollout says it finished.
    fn shown_status(&self, run: Option<&Run>) -> ToolCallStatus {
        if self.backgrounded && self.status == ToolCallStatus::Completed {
            return ToolCallStatus::InProgress;
        }

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

        // A skill that forks runs as a subagent: its calls name the Skill
        // call as their parent, or its result says it went to the background.
        if let Some(skill) = &self.skill
            && (!self.steps.is_empty() || self.output.contains(FORKED_SKILL))
        {
            return Cow::Owned(format!("Skill: {skill} (subagent)"));
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

        if let Some(Sent { done, total }) = self.sent
            && status == ToolCallStatus::InProgress
        {
            text.push_str(&format!("\n[sent] {done} {total}"));
        }
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

            if let Some(session) = run.and_then(|run| run.session.as_ref()) {
                text.push_str(&format!("\n[session] {session}"));
            }

            let model = run
                .and_then(|run| run.model.as_deref())
                .or(self.subagent.as_ref().and_then(|sub| sub.model.as_deref()));
            if let Some(model) = model {
                text.push_str(&format!("\n[model] {model}"));
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

/// Updates can carry the arguments after the initial call announcement.
fn update_details(details: &mut AgentToolCall, input: Option<&Value>, result: Option<&Value>) {
    if input.is_some_and(|value| !value.is_null()) {
        (details.input, details.input_truncated) = tool_details::json(input);
    }

    if result.is_some_and(|value| !value.is_null()) {
        (details.result, details.result_truncated) = tool_details::json(result);
    }
}
