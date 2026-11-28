//! Antigravity's sessions (`conversations_dir`, `sessions`): a SQLite
//! database per conversation, `conversations/<id>.db` under
//! `~/.gemini/antigravity-cli`, whose `trajectory_metadata_blob` names the
//! folder it ran in and when it started. Titles, and which conversations a
//! session started as subagents, are in `conversation_summaries.db` beside
//! the folder. Each step of a conversation is a row of `steps` (`transcript`).

use super::codex_run;
use super::common::{
    Agent, AgySpawn, AgyStatus, AgyTranscript, Memos, Seen, Summaries, newest_files, open_database,
    prompt_title, unix_secs,
};
use crate::calls::Tool as BeepTool;
use crate::logs::subagent::{Conversation, Run, RunStep};
use crate::logs::window::{COMPACTED_ENTRY, titled_call};
use crate::tags;
use crate::text::{cut, valid_id};
use beep_proto::SessionId;
use beep_proto::node::agent::{Entry, EntryKind};
use prost::Message;
use rusqlite::OptionalExtension;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

const TRAJECTORY: &str = "SELECT data FROM trajectory_metadata_blob LIMIT 1";
const STEPS: &str = "SELECT step_type, status, step_payload FROM steps
     WHERE step_type IN (14, 15, 17, 23, 132) ORDER BY idx";
const HAS_STEPS: &str = "SELECT EXISTS (SELECT 1 FROM steps)";
const TITLES: &str = "SELECT conversation_id, title FROM conversation_summaries WHERE title != ''";
/// A conversation's first stretch of what the user said, for one with no title.
const FIRST_PROMPTS: &str =
    "SELECT conversation_id, substr(user_body, 1, 65536) FROM conversations_fts
     WHERE chunk_index = 0 AND user_body != ''";
const CHILDREN: &str = "SELECT conversation_id FROM conversation_summaries
     WHERE parent_conversation_id != ''
     UNION SELECT conversation_id FROM conversation_parents";
const CHILD_STATES: &str = "SELECT conversation_id, status, COALESCE(killed, 0)
     FROM conversation_summaries WHERE conversation_id IN (
         SELECT conversation_id FROM conversation_summaries WHERE parent_conversation_id != ''
         UNION SELECT conversation_id FROM conversation_parents
     )";

pub(crate) const SPAWN_TOOL: &str = "invoke_subagent";

/// Where Antigravity keeps its conversations: `~/.gemini/antigravity-cli`,
/// which nothing but the home moves. It may not exist yet.
pub(crate) fn conversations_dir() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".gemini")
            .join("antigravity-cli")
            .join("conversations"),
    )
}

pub(crate) fn is_conversation(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "db")
}

/// The conversations in `root` that the user started, newest first.
pub(crate) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let summaries = summaries(root, memos);

    newest_files(root, 0, &is_conversation)
        .into_iter()
        .filter_map(|(path, at)| {
            let mut seen = memos
                .heads
                .read_database(&path, || head(&path, false))
                .inspect_err(|e| {
                    log::debug!(
                        "agy conversation not read; path={} error={e}",
                        path.display()
                    )
                })
                .ok()??;

            if summaries.children.contains(&seen.id) {
                return None;
            }

            // agy opens every conversation when it starts, which touches an
            // empty `-wal`: only one holding writes counts.
            let wal_at = std::fs::metadata(super::wal_path(&path))
                .ok()
                .filter(|meta| meta.len() > 0)
                .and_then(|meta| meta.modified().ok())
                .map_or(0, super::common::unix_secs);
            seen.last_activity = at.max(wal_at).max(seen.started_at);
            seen.title = summaries.titles.get(&seen.id).cloned();
            Some(seen)
        })
        .collect()
}

/// The database of conversation `id` in the conversations folder `root`.
pub(crate) fn database(root: &Path, id: &SessionId) -> PathBuf {
    root.join(format!("{}.db", id.as_ref()))
}

/// The longest entry kept.
const MAX_ENTRY: usize = 4 * 1024;
const MAX_CARD_STEPS: usize = 40;
const MAX_CARD_STEP: usize = 120;

impl AgyTranscript {
    pub(super) fn spawns(&self) -> impl Iterator<Item = (usize, &str)> {
        self.spawns
            .iter()
            .map(|(id, spawn)| (spawn.entry, id.as_str()))
    }

    pub(super) fn child_cards<'a>(
        &self,
        parent: &Path,
        memos: &Memos,
        len: usize,
        calls: impl Iterator<Item = (usize, &'a str)>,
    ) -> Vec<(usize, String)> {
        let Some(root) = parent.parent() else {
            return Vec::new();
        };
        let mut matched = calls
            .filter_map(|(index, id)| Some((index, self.spawns.get(id)?)))
            .filter(|(index, _)| *index < len)
            .peekable();
        if matched.peek().is_none() {
            return Vec::new();
        }

        let summaries = summaries(root, memos);
        matched
            .map(|(index, spawn)| {
                let mut run = Run {
                    name: spawn.name.clone(),
                    task: spawn.task.clone(),
                    ..Run::default()
                };
                let mut status = spawn.status;
                if let Some(child) = &spawn.child {
                    status = summaries
                        .child_states
                        .get(child)
                        .copied()
                        .unwrap_or(AgyStatus::InProgress);
                    run.working = status == AgyStatus::InProgress;

                    let path = database(root, child);
                    if let Ok(log) = transcript(&path, memos) {
                        let mut steps = log
                            .entries
                            .iter()
                            .filter(|entry| entry.kind == EntryKind::Tool);
                        run.count = steps.clone().count();
                        run.steps = steps
                            .by_ref()
                            .skip(run.count.saturating_sub(MAX_CARD_STEPS))
                            .map(|entry| RunStep {
                                title: cut(&entry.text, MAX_CARD_STEP),
                                done: true,
                            })
                            .collect();
                        run.result = log
                            .entries
                            .iter()
                            .rev()
                            .find(|entry| entry.kind == EntryKind::Agent)
                            .map(|entry| entry.text.clone());
                        if summaries.children.contains(child) {
                            run.session =
                                Some(Conversation::Agy(child.as_ref()).to_string().into());
                        }
                    }
                }
                (index, run.entry_with_status(status.as_ref()).text)
            })
            .collect()
    }
}

pub(super) fn transcript(database: &Path, memos: &Memos) -> rusqlite::Result<Arc<AgyTranscript>> {
    memos
        .agy_logs
        .read_database(database, || read_transcript(database).map(Arc::new))
}

/// Read the conversation and link invocation results to their initiating calls.
fn read_transcript(database: &Path) -> rusqlite::Result<AgyTranscript> {
    let conn = open_database(database)?;
    let mut statement = conn.prepare(STEPS)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<Vec<u8>>>(2)?,
        ))
    })?;
    let mut log = AgyTranscript::default();
    let entries = &mut log.entries;
    let mut calls: HashMap<String, usize> = HashMap::new();

    for row in rows {
        let (Some(kind), Some(status), Some(data)) = row? else {
            continue;
        };
        let data = data.as_slice();

        match kind {
            USER => {
                let text = UserStep::decode(data).ok().and_then(|step| step.user);
                let text = text
                    .map(|user| tags::untagged(&user.text))
                    .unwrap_or_default();
                entries.extend(entry(EntryKind::User, &text));
            }
            REPLY => {
                let Some(reply) = ReplyStep::decode(data).ok().and_then(|step| step.reply) else {
                    continue;
                };
                entries.extend(entry(EntryKind::Thought, &reply.thinking));
                entries.extend(entry(EntryKind::Agent, &reply.text));

                for call in &reply.calls {
                    let Some(made) = entry(EntryKind::Tool, &call.title()) else {
                        continue;
                    };
                    if !call.id.is_empty() {
                        calls.insert(call.id.clone(), entries.len());
                        if call.name == SPAWN_TOOL {
                            let args: serde_json::Value =
                                serde_json::from_str(&call.args).unwrap_or_default();
                            let first_line = |key: &str| {
                                args[key]
                                    .as_str()
                                    .and_then(|text| text.lines().next())
                                    .and_then(prompt_title)
                            };
                            log.spawns.insert(
                                call.id.clone(),
                                AgySpawn {
                                    entry: entries.len(),
                                    child: None,
                                    status: AgyStatus::InProgress,
                                    name: first_line("TypeName"),
                                    task: first_line("Role").or_else(|| first_line("Prompt")),
                                },
                            );
                        }
                    }
                    entries.push(made);
                }
            }
            CALL => {
                let Some(step) = CallStep::decode(data).ok() else {
                    continue;
                };
                let Some(call) = step.meta.and_then(|meta| meta.call) else {
                    continue;
                };

                if let Some(spawn) = log.spawns.get_mut(&call.id) {
                    if let Some(child) = InvocationStep::decode(data)
                        .ok()
                        .and_then(|step| step.output)
                        .and_then(InvocationResult::child)
                    {
                        spawn.child = Some(child);
                    } else if spawn.child.is_none() {
                        spawn.status = match status {
                            FAILED | CANCELLED => AgyStatus::Failed,
                            SUCCEEDED => AgyStatus::Completed,
                            _ => AgyStatus::InProgress,
                        };
                    }
                }

                if status == FAILED
                    && let Some(entry) = calls.get(&call.id).and_then(|at| entries.get_mut(*at))
                {
                    entry.text.push_str(" (failed)");
                }
            }
            ERROR => {
                let error = ErrorStep::decode(data)
                    .ok()
                    .and_then(|step| step.error?.detail);
                entries.extend(entry(
                    EntryKind::Other,
                    &error.map(|detail| detail.text).unwrap_or_default(),
                ));
            }
            COMPACTED => entries.extend(entry(EntryKind::Compacted, COMPACTED_ENTRY)),
            _ => {}
        }
    }

    Ok(log)
}

fn entry(kind: EntryKind, text: &str) -> Option<Entry> {
    let text = text.trim();
    (!text.is_empty()).then(|| Entry::new(kind, cut(text, MAX_ENTRY)))
}

/// The summaries database beside the conversations folder `root`.
pub(super) fn summaries_at(root: &Path) -> Option<PathBuf> {
    Some(root.parent()?.join("conversation_summaries.db"))
}

fn summaries(root: &Path, memos: &Memos) -> Arc<Summaries> {
    summaries_at(root)
        .filter(|path| path.is_file())
        .and_then(|path| {
            memos
                .agy
                .read_database(&path, || read_summaries(&path).map(Arc::new))
                .inspect_err(|e| log::debug!("agy summaries not read; error={e}"))
                .ok()
        })
        .unwrap_or_default()
}

pub(super) fn native(database: &Path, id: &SessionId, memos: &Memos) -> Option<Seen> {
    let summary = summaries(database.parent()?, memos);
    let raw_id = SessionId::from(database.file_stem()?.to_str()?);
    if !summary.children.contains(&raw_id) {
        return None;
    }

    let mut seen = memos
        .agy_child_heads
        .read_database(database, || head(database, true))
        .ok()??;
    seen.id = id.clone();
    seen.title = summary
        .titles
        .get(&raw_id)
        .cloned()
        .or_else(|| Some("Antigravity subagent".into()));
    seen.file = Some(database.to_owned());
    seen.last_activity = database
        .metadata()
        .and_then(|meta| meta.modified())
        .map_or(seen.started_at, unix_secs);
    Some(seen)
}

pub(super) fn child_state(database: &Path, memos: &Memos) -> Option<AgyStatus> {
    let id = SessionId::from(database.file_stem()?.to_str()?);
    summaries(database.parent()?, memos)
        .child_states
        .get(&id)
        .copied()
}

fn read_summaries(path: &Path) -> rusqlite::Result<Summaries> {
    let conn = open_database(path)?;
    let mut summaries = Summaries::default();

    // Titles are read last, so they win over first prompts.
    for query in [FIRST_PROMPTS, TITLES] {
        let mut statement = conn.prepare(query)?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (id, text) = row?;
            if let Some(title) = prompt_title(&text) {
                summaries.titles.insert(Into::<SessionId>::into(id), title);
            }
        }
    }

    let mut statement = conn.prepare(CHILDREN)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;

    for id in rows {
        summaries.children.insert(Into::<SessionId>::into(id?));
    }

    // Older databases can still supply titles and child identities.
    if let Ok(mut statement) = conn.prepare(CHILD_STATES) {
        let rows = statement.query_map([], |row| {
            let id: String = row.get(0)?;
            let status: Option<String> = row.get(1)?;
            let killed: bool = row.get(2)?;
            let state = if killed {
                AgyStatus::Failed
            } else if status.as_deref() == Some("CASCADE_RUN_STATUS_RUNNING") {
                AgyStatus::InProgress
            } else {
                AgyStatus::Completed
            };
            Ok((SessionId::from(id), state))
        })?;

        for row in rows {
            let (id, state) = row?;
            summaries.child_states.insert(id, state);
        }
    }

    Ok(summaries)
}

/// The conversation at `database`: its id, folder and start; `None` for
/// one with no folder or no step yet.
fn head(database: &Path, allow_empty: bool) -> rusqlite::Result<Option<Seen>> {
    let conn = open_database(database)?;
    if !allow_empty && !conn.query_row(HAS_STEPS, [], |row| row.get::<_, bool>(0))? {
        return Ok(None);
    }

    let Some(trajectory) = conn
        .query_row(TRAJECTORY, [], |row| row.get::<_, Vec<u8>>(0))
        .optional()?
        .and_then(|data| Trajectory::decode(data.as_slice()).ok())
    else {
        return Ok(None);
    };
    let cwd = trajectory
        .workspaces
        .first()
        .and_then(|workspace| url::Url::parse(&workspace.uri).ok()?.to_file_path().ok());
    let id = database
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|id| !id.is_empty());
    let (Some(cwd), Some(id)) = (cwd, id) else {
        return Ok(None);
    };
    let started = trajectory
        .created
        .and_then(|time| u64::try_from(time.seconds).ok())
        .unwrap_or(0);

    Ok(Some(Seen {
        agent: Agent::Agy,
        id: Into::<SessionId>::into(id),
        cwd,
        started_at: started,
        last_activity: started,
        title: None,
        file: None,
    }))
}

// Antigravity's records are protobuf without a published schema; only the
// fields read here are named, by their numbers as the CLI writes them.

/// `steps.step_type`: what the user said.
const USER: i64 = 14;
/// The model's turn: its thinking, reply and calls.
const REPLY: i64 = 15;
/// An error the model's service gave.
const ERROR: i64 = 17;
/// The conversation compacted.
const COMPACTED: i64 = 23;
/// A call's result.
const CALL: i64 = 132;
/// `steps.status` of a call that failed.
const FAILED: i64 = 7;
const CANCELLED: i64 = 6;
const SUCCEEDED: i64 = 3;

// A row's `step_payload`, read as its `step_type` has it, so a field
// another type holds differently does not lose the step.

#[derive(Clone, PartialEq, Message)]
pub(crate) struct UserStep {
    #[prost(message, optional, tag = "19")]
    pub(crate) user: Option<UserInput>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ReplyStep {
    #[prost(message, optional, tag = "20")]
    pub(crate) reply: Option<Reply>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct CallStep {
    #[prost(message, optional, tag = "5")]
    pub(crate) meta: Option<StepMeta>,
}

#[derive(Clone, PartialEq, Message)]
struct InvocationStep {
    #[prost(message, optional, tag = "140")]
    output: Option<InvocationResult>,
}

#[derive(Clone, PartialEq, Message)]
struct InvocationResult {
    #[prost(message, optional, tag = "2")]
    response: Option<InvocationResponse>,
}

impl InvocationResult {
    fn child(self) -> Option<SessionId> {
        let id = self.response?.result?.invocation?.reference?.id;
        valid_id(&id).then(|| SessionId::from(id))
    }
}

#[derive(Clone, PartialEq, Message)]
struct InvocationResponse {
    #[prost(message, optional, tag = "3")]
    result: Option<InvocationPayload>,
}

#[derive(Clone, PartialEq, Message)]
struct InvocationPayload {
    #[prost(message, optional, tag = "9")]
    invocation: Option<Invocation>,
}

#[derive(Clone, PartialEq, Message)]
struct Invocation {
    #[prost(message, optional, tag = "5")]
    reference: Option<ConversationReference>,
}

#[derive(Clone, PartialEq, Message)]
struct ConversationReference {
    #[prost(string, tag = "1")]
    id: String,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ErrorStep {
    #[prost(message, optional, tag = "24")]
    pub(crate) error: Option<StepError>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct StepMeta {
    /// The call a result answers.
    #[prost(message, optional, tag = "4")]
    pub(crate) call: Option<Call>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct UserInput {
    #[prost(string, tag = "2")]
    pub(crate) text: String,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Reply {
    #[prost(string, tag = "1")]
    pub(crate) text: String,
    #[prost(string, tag = "3")]
    pub(crate) thinking: String,
    #[prost(message, repeated, tag = "7")]
    pub(crate) calls: Vec<Call>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Call {
    #[prost(string, tag = "1")]
    pub(crate) id: String,
    #[prost(string, tag = "2")]
    pub(crate) name: String,
    /// The arguments, as JSON.
    #[prost(string, tag = "3")]
    pub(crate) args: String,
}

impl Call {
    /// The call's name and its command, path or summary.
    fn title(&self) -> String {
        let args: serde_json::Value = serde_json::from_str(&self.args).unwrap_or_default();

        if let Some(beep) = BeepTool::of_name(&self.name) {
            return beep.title(Some(&args));
        }

        let detail = ["CommandLine", "Path"]
            .iter()
            .find_map(|key| {
                args[key]
                    .as_str()
                    .and_then(|text| text.lines().next())
                    .map(str::to_owned)
            })
            .or_else(|| codex_run::detail(&args))
            .or_else(|| args["toolSummary"].as_str().map(str::to_owned));
        titled_call(&self.name, detail.as_deref())
    }
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct StepError {
    #[prost(message, optional, tag = "3")]
    pub(crate) detail: Option<ErrorDetail>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ErrorDetail {
    #[prost(string, tag = "2")]
    pub(crate) text: String,
}

/// The conversation's `trajectory_metadata_blob`.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct Trajectory {
    #[prost(message, repeated, tag = "1")]
    pub(crate) workspaces: Vec<Workspace>,
    #[prost(message, optional, tag = "2")]
    pub(crate) created: Option<Time>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Workspace {
    /// A `file://` URI.
    #[prost(string, tag = "1")]
    pub(crate) uri: String,
}

/// A protobuf `Timestamp`, to the second.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct Time {
    #[prost(int64, tag = "1")]
    pub(crate) seconds: i64,
}

#[cfg(test)]
#[path = "../../tests/logs_found_agy.rs"]
mod tests;
