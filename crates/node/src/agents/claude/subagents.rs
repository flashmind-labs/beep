//! The subagents a Claude Code session started (`attach`), read from
//! `<session>/subagents/agent-<id>.jsonl` and its `.meta.json`, each shown on
//! the entry of the call that started it.

use beep_proto::SessionId;
use super::transcript::{Page, Spawn, is_native, read_tail, tool_line};
use super::{discover::Store, transcript};
use crate::agents::{cut, window::Excerpt};
use crate::agents::found::memo::Memo;
use crate::agents::subagent::Conversation;
use beep_proto::node::agent::{Entry, ExternalSession, Presence};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::{Duration, Instant, SystemTime},
};

fn conversation(session: &Path, agent: &str) -> Option<String> {
    let parent = if is_native(session) {
        session.parent()?.parent()?.file_name()?.to_str()?
    } else {
        session.file_stem()?.to_str()?
    };
    let id = Conversation::Claude { parent, agent }.to_string();
    Conversation::try_from(id.as_str()).is_ok().then_some(id)
}

/// The native child reached through its parent's conversation, never a root listing.
pub(super) fn session(store: &Store, id: &SessionId) -> std::io::Result<Option<ExternalSession>> {
    let Some(Conversation::Claude { parent, agent }) = Conversation::try_from(id.as_ref()).ok() else {
        return Ok(None);
    };
    let Some(path) = store.find(id) else {
        return Ok(None);
    };
    let found = Found {
        id: agent.to_owned(),
        meta: meta(&path.with_extension("meta.json")),
        transcript: path.clone(),
    };
    let digest = transcript::digest_tail(&path, 100)?;
    let head = transcript::head(&path)?;
    let parent_cwd = store
        .find(&Into::<SessionId>::into(parent))
        .and_then(|path| transcript::head(&path).ok())
        .and_then(|head| head.said.cwd);
    let cwd = digest.said.cwd.or(head.said.cwd).or(parent_cwd).unwrap_or_default();
    let presence = match status(&Spawn::default(), &found) {
        Status::InProgress => Presence::Working,
        _ => Presence::Stopped,
    };
    Ok(Some(ExternalSession {
        id: id.to_owned(),
        agent: "claude".to_owned(),
        title: found.meta.description.or(head.said.title).or(head.said.last_prompt),
        place: crate::agents::found::folder_place::of_folder(Path::new(&cwd)),
        cwd,
        started_at: head.first_at.unwrap_or(0),
        last_activity: digest.said.last_at.unwrap_or(0),
        last_prompt: digest.said.last_prompt.unwrap_or_default(),
        last_message: digest.said.last_message.unwrap_or_default(),
        presence,
        ..Default::default()
    }))
}

/// The most subagent files one session's folder is read for.
const MAX_AGENTS: usize = 1_000;
/// The longest meta file read.
const MAX_META: u64 = 64 * 1024;
/// How much of the end of a subagent's file is read.
const MAX_READ: u64 = 4 * 1024 * 1024;
/// The most steps an entry names; it counts them all.
const MAX_STEPS: usize = 30;
/// The longest step line.
const MAX_STEP: usize = 120;
/// The longest subagent entry.
const MAX_ENTRY: usize = 12 * 1024;
/// Legacy subagents without lifecycle metadata use recent file activity.
const RUNNING: Duration = Duration::from_secs(2 * 60);
/// How many subagent files' reads are kept. A session with many subagents
/// otherwise reads each one's tail again on every page and update.
const RUNS_KEPT: usize = 2 * MAX_AGENTS;

/// Each subagent file's run, kept while the file is unchanged.
static RUNS: LazyLock<Memo<Arc<Run>>> = LazyLock::new(|| Memo::new(RUNS_KEPT));

/// What `agent-<id>.meta.json` says.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Meta {
    agent_type: Option<String>,
    /// A name other than its type, such as the skill that started it.
    name: Option<String>,
    description: Option<String>,
    tool_use_id: Option<String>,
    finished: Option<bool>,
    stopped_by_user: Option<bool>,
}

/// One subagent's files.
#[derive(Debug)]
struct Found {
    /// The id its files are named by.
    id: String,
    transcript: PathBuf,
    meta: Meta,
}

/// How a subagent or one of its steps stands, by its wire name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "snake_case")]
enum Status {
    InProgress,
    Completed,
    Failed,
}

/// A subagent's own lines: the calls it made and its last text.
#[derive(Debug, Default)]
struct Run {
    steps: Vec<Step>,
    /// Each call's step, by its id.
    calls: HashMap<String, usize>,
    result: Option<String>,
    model: Option<String>,
}

#[derive(Debug)]
struct Step {
    status: Status,
    title: String,
}

/// The entries of `page` with the subagents its calls started.
pub(super) fn attached(session: &Path, page: Page) -> Excerpt {
    let Page {
        mut excerpt,
        spawns,
        skip,
    } = page;
    let started = Instant::now();
    attach(session, &spawns, &mut excerpt.entries, skip);
    crate::agents::transcript_stage("subagents", started);
    excerpt
}

/// Writes the subagent each of `spawns` started on its entry among
/// `entries`, which start at the digest's entry `skip`. A call whose
/// subagent's file is not found stays as it was.
pub(super) fn attach(
    session: &Path,
    spawns: &HashMap<String, Spawn>,
    entries: &mut [Entry],
    skip: usize,
) {
    let mut shown: Vec<(&String, &Spawn)> = spawns
        .iter()
        .filter(|(_, spawn)| (skip..skip + entries.len()).contains(&spawn.entry))
        .collect();
    if shown.is_empty() {
        return;
    }

    shown.sort_by_key(|(_, spawn)| spawn.entry);
    let folder = if is_native(session) {
        session.parent().map(Path::to_owned).unwrap_or_default()
    } else {
        session.with_extension("").join("subagents")
    };
    let found = agents(&folder);
    let items = shown
        .into_iter()
        .map(|(call, spawn)| (spawn.entry - skip, call.as_str(), spawn));

    attach_each(session, &found, items, entries, true);
}

/// Writes the subagent each call started on the entry at its index. A
/// subagent without a conversation id leaves its entry as it was unless
/// `keep_text`, which writes its text anyway.
fn attach_each<'a>(
    session: &Path,
    found: &[Found],
    calls: impl Iterator<Item = (usize, &'a str, &'a Spawn)>,
    entries: &mut [Entry],
    keep_text: bool,
) {
    let mut taken = HashSet::new();

    for (at, call, spawn) in calls {
        let Some(agent) = pick(found, call, spawn, &taken) else {
            continue;
        };
        taken.insert(agent.id.as_str());
        let Some(entry) = entries.get_mut(at) else {
            continue;
        };
        let id = conversation(session, &agent.id);

        if id.is_none() && !keep_text {
            continue;
        }
        entry.text = text(spawn, agent, &read(&agent.transcript), id.as_deref());
    }
}

/// Refresh ACP spawn calls from the harness's child files.
pub(super) fn attach_live(
    session: &Path,
    calls: &[(usize, String)],
    entries: &mut [Entry],
    kept: &transcript::Transcripts,
) {
    let started = Instant::now();
    let attached = kept.with(session, |digest| {
        attach_calls(session, calls, entries, &digest.spawns)
    });

    if attached.is_err() {
        attach_calls(session, calls, entries, &HashMap::new());
    }

    crate::agents::transcript_stage("subagents", started);
}

fn attach_calls(
    session: &Path,
    calls: &[(usize, String)],
    entries: &mut [Entry],
    spawns: &HashMap<String, Spawn>,
) {
    let found = agents(&session.with_extension("").join("subagents"));
    let fallback = Spawn::default();
    let items = calls
        .iter()
        .map(|(at, call)| (*at, call.as_str(), spawns.get(call).unwrap_or(&fallback)));

    attach_each(session, &found, items, entries, false);
}

/// The subagent `call` started: the one whose meta names the call, else
/// the one its result named, else one not taken with its description.
fn pick<'a>(
    found: &'a [Found],
    call: &str,
    spawn: &Spawn,
    taken: &HashSet<&str>,
) -> Option<&'a Found> {
    let free = |agent: &&Found| !taken.contains(agent.id.as_str());
    found
        .iter()
        .find(|agent| agent.meta.tool_use_id.as_deref() == Some(call))
        .or_else(|| {
            let id = spawn.agent_id.as_deref()?;
            found.iter().find(|agent| agent.id == id)
        })
        .or_else(|| {
            let description = spawn.description.as_deref()?;
            found.iter().filter(free).find(|agent| {
                agent.meta.tool_use_id.is_none()
                    && agent.meta.description.as_deref().map(str::trim) == Some(description)
            })
        })
}

/// The subagents in `dir`, each with its meta when it has one.
fn agents(dir: &Path) -> Vec<Found> {
    let Ok(files) = fs::read_dir(dir) else {
        return Vec::new();
    };
    files
        .flatten()
        .filter_map(|file| {
            let name = file.file_name();
            let id = name
                .to_str()?
                .strip_suffix(".jsonl")?
                .strip_prefix("agent-")?;
            Some(Found {
                id: id.to_owned(),
                transcript: file.path(),
                meta: meta(&dir.join(format!("agent-{id}.meta.json"))),
            })
        })
        .take(MAX_AGENTS)
        .collect()
}

/// The meta at `path`; empty when there is none or it does not read.
fn meta(path: &Path) -> Meta {
    let Ok(file) = fs::File::open(path) else {
        return Meta::default();
    };
    let reader = std::io::Read::take(file, MAX_META);

    match serde_json::from_reader(reader) {
        Ok(meta) => meta,
        Err(e) => {
            log::debug!("subagent meta not read; path={} error={e}", path.display());
            Meta::default()
        }
    }
}

/// The end of the subagent's file at `path`: the calls it made and what
/// it said last, read again only once the file changes. Empty when it does
/// not read.
fn read(path: &Path) -> Arc<Run> {
    let Ok(run) = RUNS.read(path, || {
        Ok::<_, std::convert::Infallible>(Arc::new(read_run(path)))
    });
    run
}

fn read_run(path: &Path) -> Run {
    let mut run = Run::default();
    let tail = match read_tail(path, MAX_READ) {
        Ok(tail) => tail,
        Err(e) => {
            log::debug!("subagent not read; path={} error={e}", path.display());
            return run;
        }
    };

    for line in tail.text.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
            run.line(&value);
        }
    }

    run
}

impl Run {
    fn line(&mut self, value: &Value) {
        let Value::Array(blocks) = &value["message"]["content"] else {
            return;
        };

        match value["type"].as_str() {
            Some("assistant") => {
                if let Some(model) = value["message"]["model"]
                    .as_str()
                    .map(str::trim)
                    && !model.is_empty() && model != "<synthetic>"
                {
                    self.model = Some(cut(model, MAX_STEP));
                }

                for block in blocks {
                    match block["type"].as_str() {
                        Some("text") => {
                            let text = block["text"].as_str().unwrap_or("").trim();
                            if !text.is_empty() {
                                self.result = Some(text.to_owned());
                            }
                        }
                        Some("tool_use") => {
                            if let Some(id) = block["id"].as_str() {
                                self.calls.insert(id.to_owned(), self.steps.len());
                            }
                            self.steps.push(Step {
                                status: Status::InProgress,
                                title: tool_line(block),
                            });
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for block in blocks.iter().filter(|block| block["type"] == "tool_result") {
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    let Some(step) = self.calls.get(id).and_then(|&at| self.steps.get_mut(at))
                    else {
                        continue;
                    };
                    step.status = if block["is_error"].as_bool() == Some(true) {
                        Status::Failed
                    } else {
                        Status::Completed
                    };
                }
            }
            _ => {}
        }
    }
}

/// Correlated metadata owns completion. File age is only a fallback for
/// older subagents without it: a long tool call can leave the file quiet.
fn status(spawn: &Spawn, agent: &Found) -> Status {
    if spawn.failed || agent.meta.stopped_by_user == Some(true) {
        return Status::Failed;
    }

    if agent.meta.finished == Some(true) {
        return Status::Completed;
    }

    if agent.meta.tool_use_id.is_some() || agent.meta.finished == Some(false) {
        return Status::InProgress;
    }

    let changed = fs::metadata(&agent.transcript).and_then(|meta| meta.modified());
    let recent = changed
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age < RUNNING);
    if recent {
        Status::InProgress
    } else {
        Status::Completed
    }
}

/// The entry of the call that started `agent`, as a live session writes it.
fn text(spawn: &Spawn, agent: &Found, run: &Run, session: Option<&str>) -> String {
    let status = status(spawn, agent);
    let title = spawn
        .description
        .as_deref()
        .or(agent.meta.description.as_deref())
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Subagent");
    let title = title.lines().next().unwrap_or_default();
    let mut text = format!("{title} [{status}]\n[steps] {}", run.steps.len());

    for step in &run.steps[run.steps.len().saturating_sub(MAX_STEPS)..] {
        // A step left running in a finished subagent did not fail.
        let shown = match step.status {
            Status::InProgress if status != Status::InProgress => Status::Completed,
            other => other,
        };
        let title = step.title.lines().next().unwrap_or_default();
        text.push_str(&format!("\n[step {}] {}", shown, cut(title, MAX_STEP)));
    }

    let name = agent
        .meta
        .name
        .as_deref()
        .or(spawn.agent.as_deref())
        .or(agent.meta.agent_type.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(name) = name {
        text.push_str(&format!("\n[agent] {}", cut(name, MAX_STEP)));
    }

    if let Some(id) = session {
        text.push_str(&format!("\n[session] {id}"));
    }

    if let Some(model) = run.model.as_deref().or(spawn.model.as_deref()) {
        text.push_str(&format!("\n[model] {}", cut(model, MAX_STEP)));
    }

    if let Some(result) = &run.result {
        text.push('\n');
        text.push_str(result);
    }
    cut(&text, MAX_ENTRY)
}

#[cfg(test)]
#[path = "../../tests/agents_claude_subagents.rs"]
mod tests;
