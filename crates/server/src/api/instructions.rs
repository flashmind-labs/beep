//! The user's instructions for every agent Beep runs: their versions, and
//! the agent files on the user's machines that Beep reads them from and
//! writes them into, between its markers.

use super::{Reply, agent_sessions::linked_keys};
use crate::{
    nodes::Online,
    protocol::*,
    router::Cx,
    store::{InstructionsRow, Saved},
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::{StreamExt, stream};
use std::time::Duration;

/// The longest instructions, in bytes.
const MAX_TEXT: usize = 16 * 1024;
/// The longest note.
const MAX_NOTE: usize = 200;
/// The most bytes of one agent file read back.
const MAX_FILE: usize = 256 * 1024;
/// How long reading or writing a machine's files may take.
const NODE_LIMIT: Duration = Duration::from_secs(20);
/// Machines read at once.
const IN_FLIGHT: usize = 4;

/// Where Beep's text starts and ends in an agent file.
const START: &str = "<!-- beep:start -->";
const END: &str = "<!-- beep:end -->";

/// Each coding agent and its file under the home folder.
const AGENT_FILES: [(&str, &str); 4] = [
    ("claude", ".claude/CLAUDE.md"),
    ("codex", ".codex/AGENTS.md"),
    ("gemini", ".gemini/GEMINI.md"),
    ("pi", ".pi/agent/AGENTS.md"),
];

pub(super) async fn get_instructions(
    cx: Cx,
    _: GetInstructions,
) -> Reply<Option<InstructionsVersion>> {
    Ok(newest(&cx).await?.map(view))
}

pub(super) async fn save_instructions(
    cx: Cx,
    request: SaveInstructions,
) -> Reply<InstructionsVersion> {
    if request.text.len() > MAX_TEXT {
        return Err(ApiError::bad_request(
            "Instructions can be at most 16 KB.".to_owned(),
        ));
    }
    let note = request.note.trim();
    if note.len() > MAX_NOTE || note.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "A note can be at most {MAX_NOTE} characters on one line."
        )));
    }
    match cx
        .user
        .save_instructions(request.base, &request.text, note)
        .await?
    {
        Saved::Made(row) => Ok(view(row)),
        Saved::Stale(newest) => Err(ApiError::Conflict {
            message: format!("Version {newest} was saved since. Load it, then edit again."),
        }),
    }
}

pub(super) async fn list_instructions_versions(
    cx: Cx,
    _: ListInstructionsVersions,
) -> Reply<Vec<InstructionsVersion>> {
    Ok(cx
        .user
        .instructions_versions(50)
        .await?
        .into_iter()
        .map(view)
        .collect())
}

pub(super) async fn read_agent_files(cx: Cx, _: ReadAgentFiles) -> Reply<Vec<MachineAgentFiles>> {
    let text = newest(&cx).await?.map(|row| row.text);
    let nodes = machines(&cx).await?;
    let mut read: Vec<MachineAgentFiles> = stream::iter(nodes)
        .map(|node| {
            let text = text.clone();
            async move {
                let files = read_files(&node).await;
                machine_files(&node, files, text.as_deref())
            }
        })
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await;
    read.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    Ok(read)
}

pub(super) async fn sync_agent_files(cx: Cx, request: SyncAgentFiles) -> Reply<MachineAgentFiles> {
    let Some(newest) = newest(&cx).await? else {
        return Err(ApiError::bad_request(
            "Save your instructions before writing them to agent files.".to_owned(),
        ));
    };
    let node = machines(&cx)
        .await?
        .into_iter()
        .find(|node| node.connection.remote_id().to_string() == request.machine_id)
        .ok_or(ApiError::NotFound {})?;
    let fail = |e: anyhow::Error| ApiError::bad_request(format!("{}: {e:#}", node.name));
    let before = read_files(&node).await.map_err(fail)?;
    let script = write_script(&before, &newest.text);
    if !script.is_empty() {
        let mut output = String::new();
        let ran = node
            .exec(&script, NODE_LIMIT, &mut |text| output.push_str(text))
            .await
            .map_err(fail)?;
        if ran.exit_code != Some(0) {
            return Err(fail(anyhow::anyhow!(
                "writing the agent files failed: {}",
                output.trim()
            )));
        }
    }
    let after = read_files(&node).await;
    Ok(machine_files(&node, after, Some(&newest.text)))
}

async fn newest(cx: &Cx) -> Result<Option<InstructionsRow>, ApiError> {
    Ok(cx.user.instructions_versions(1).await?.into_iter().next())
}

fn view(row: InstructionsRow) -> InstructionsVersion {
    InstructionsVersion {
        version: row.version,
        text: row.text,
        note: row.note,
        saved_at: row.saved_at.into(),
    }
}

/// The caller's online machines whose keys are still linked.
async fn machines(cx: &Cx) -> Result<Vec<Online>, ApiError> {
    let keys = linked_keys(cx).await?;
    Ok(cx
        .app
        .nodes
        .of_user(cx.user.id())
        .into_iter()
        .filter(|node| keys.contains(&node.connection.remote_id().to_string()))
        .collect())
}

/// One agent's file as the node said it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadFile {
    agent: &'static str,
    /// Under the home folder.
    rel: &'static str,
    /// `None` when the agent's folder is there but the file is not.
    text: Option<String>,
}

fn machine_files(
    node: &Online,
    files: Result<Vec<ReadFile>>,
    newest: Option<&str>,
) -> MachineAgentFiles {
    let machine_id = node.connection.remote_id().to_string();
    let machine_name = node.name.clone();
    match files {
        Ok(files) => MachineAgentFiles {
            machine_id,
            machine_name,
            files: files
                .into_iter()
                .map(|file| AgentFile {
                    agent: file.agent.to_owned(),
                    path: format!("~/{}", file.rel),
                    synced: newest
                        .zip(file.text.as_deref())
                        .is_some_and(|(newest, text)| holds(text, newest)),
                    text: file.text,
                })
                .collect(),
            error: None,
        },
        Err(e) => MachineAgentFiles {
            machine_id,
            machine_name,
            files: Vec::new(),
            error: Some(format!("{e:#}")),
        },
    }
}

/// Reads every agent's file on `node`, in a shell of its own.
async fn read_files(node: &Online) -> Result<Vec<ReadFile>> {
    let mut output = String::new();
    let ran = node
        .exec(&read_script(), NODE_LIMIT, &mut |text| {
            output.push_str(text)
        })
        .await?;
    if ran.exit_code != Some(0) {
        bail!("its shell could not read the agent files");
    }
    parse_read(&output)
}

/// Prints `@@ <file> <state> [base64]` per agent file: state 1 is a file,
/// 0 the agent's folder without it, and `-` neither.
fn read_script() -> String {
    let files: Vec<&str> = AGENT_FILES.iter().map(|(_, rel)| *rel).collect();
    format!(
        "for f in {}; do \
           if [ -f \"$HOME/$f\" ]; then printf '@@ %s 1 ' \"$f\"; head -c {MAX_FILE} \"$HOME/$f\" | base64 | tr -d '\\n'; echo; \
           elif [ -d \"$HOME/$(dirname \"$f\")\" ]; then printf '@@ %s 0\\n' \"$f\"; \
           else printf '@@ %s -\\n' \"$f\"; fi; \
         done",
        files.join(" ")
    )
}

fn parse_read(output: &str) -> Result<Vec<ReadFile>> {
    let mut files = Vec::new();
    for line in output.lines() {
        let Some(rest) = line.trim_end().strip_prefix("@@ ") else {
            continue;
        };
        let mut parts = rest.splitn(3, ' ');
        let (Some(rel), Some(state)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(&(agent, rel)) = AGENT_FILES.iter().find(|(_, known)| *known == rel) else {
            continue;
        };
        let text = match state {
            "1" => {
                let bytes = STANDARD
                    .decode(parts.next().unwrap_or_default())
                    .with_context(|| format!("~/{rel} came back garbled"))?;
                Some(String::from_utf8_lossy(&bytes).into_owned())
            }
            "0" => None,
            _ => continue,
        };
        files.push(ReadFile { agent, rel, text });
    }
    if files.is_empty() && !output.contains("@@ ") {
        bail!("its shell did not answer as expected");
    }
    Ok(files)
}

/// Whether `file` holds `text` between the markers.
fn holds(file: &str, text: &str) -> bool {
    file.contains(&block(text))
}

fn block(text: &str) -> String {
    format!("{START}\n{}\n{END}", text.trim_end())
}

/// `file` with `text` between the markers: in place of what they held, or
/// added at the end. Text outside them stays as it is.
fn merged(file: Option<&str>, text: &str) -> String {
    let block = block(text);
    let file = file.unwrap_or_default();
    if let Some(start) = file.find(START)
        && let Some(len) = file[start..].find(END)
    {
        let end = start + len + END.len();
        return format!("{}{block}{}", &file[..start], &file[end..]);
    }
    let kept = file.trim_end();
    if kept.is_empty() {
        format!("{block}\n")
    } else {
        format!("{kept}\n\n{block}\n")
    }
}

/// A shell script writing `text` into every file that does not hold it;
/// empty when all do.
fn write_script(files: &[ReadFile], text: &str) -> String {
    files
        .iter()
        .filter(|file| !file.text.as_deref().is_some_and(|now| holds(now, text)))
        .map(|file| {
            let data = STANDARD.encode(merged(file.text.as_deref(), text));
            let rel = file.rel;
            format!(
                "printf '%s' '{data}' | base64 -d > \"$HOME/{rel}.beep-new\" && mv \"$HOME/{rel}.beep-new\" \"$HOME/{rel}\""
            )
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::slice;

    #[test]
    fn merging_replaces_the_marked_block_and_keeps_the_rest() {
        let file = format!("# Mine\n\n{START}\nold\n{END}\n\n## After\n");
        assert_eq!(
            merged(Some(&file), "- new\n"),
            format!("# Mine\n\n{START}\n- new\n{END}\n\n## After\n")
        );
        assert_eq!(
            merged(Some("# Mine\n"), "rules"),
            format!("# Mine\n\n{START}\nrules\n{END}\n")
        );
        assert_eq!(merged(None, "rules"), format!("{START}\nrules\n{END}\n"));
        assert!(holds(&merged(Some(&file), "x"), "x\n"));
        assert!(!holds(&file, "x"));
    }

    #[test]
    fn the_read_output_names_each_agent_file() {
        let output = format!(
            "noise\n@@ .claude/CLAUDE.md 1 {}\n@@ .codex/AGENTS.md 0\n@@ .gemini/GEMINI.md -\n@@ .other/X.md 1 eA==\n",
            STANDARD.encode("# Rules\n")
        );
        let files = parse_read(&output).unwrap();
        assert_eq!(
            files,
            [
                ReadFile {
                    agent: "claude",
                    rel: ".claude/CLAUDE.md",
                    text: Some("# Rules\n".into()),
                },
                ReadFile {
                    agent: "codex",
                    rel: ".codex/AGENTS.md",
                    text: None,
                },
            ]
        );
        assert!(parse_read("command not found").is_err());
        assert!(parse_read("@@ .claude/CLAUDE.md 1 !!!").is_err());
    }

    #[test]
    fn only_files_without_the_text_are_written() {
        let text = "rules";
        let done = ReadFile {
            agent: "claude",
            rel: ".claude/CLAUDE.md",
            text: Some(merged(None, text)),
        };
        assert!(write_script(slice::from_ref(&done), text).is_empty());
        let missing = ReadFile {
            agent: "codex",
            rel: ".codex/AGENTS.md",
            text: None,
        };
        let script = write_script(&[done, missing], text);
        assert!(script.contains("$HOME/.codex/AGENTS.md"));
        assert!(!script.contains("CLAUDE.md"));
        assert!(read_script().contains(".pi/agent/AGENTS.md"));
    }
}
