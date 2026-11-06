//! The user's instructions for every agent, kept between markers in the
//! agent files under a machine's home folder: reading them, and writing the
//! newest text into them, through the machine's shell.

use super::{exec, said};
use crate::{MeshError, PeerClient};
use anyhow::Context;
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{AgentFile, MachineAgentFiles};
use std::time::Duration;

/// The most bytes of one agent file read back; longer is refused, never cut.
const MAX_FILE: usize = 256 * 1024;
/// How long reading or writing a machine's files may take.
pub const LIMIT: Duration = Duration::from_secs(20);

/// Where Beep's text starts and ends in an agent file.
const START: &str = "<!-- beep:start -->";
const END: &str = "<!-- beep:end -->";

/// Beep's own rules for coding agents, written after the user's text.
const BEEP_RULES: &str =
    "- Unless told otherwise, work on a new branch in a git worktree, not in the main checkout.";

/// Each coding agent and its file under the home folder.
const AGENT_FILES: [(&str, &str); 4] = [
    ("claude", ".claude/CLAUDE.md"),
    ("codex", ".codex/AGENTS.md"),
    ("gemini", ".gemini/GEMINI.md"),
    ("pi", ".pi/agent/AGENTS.md"),
];

/// One agent's file as the node said it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFile {
    pub agent: &'static str,
    /// Under the home folder.
    pub rel: &'static str,
    /// `None` when the agent's folder is there but the file is not.
    pub text: Option<String>,
}

/// How one machine's agent files stand, as `ReadAgentFiles` shows them.
pub fn machine_files(
    machine_id: String,
    machine_name: String,
    files: Result<Vec<ReadFile>, MeshError>,
    newest: Option<&str>,
) -> MachineAgentFiles {
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

/// Reads every agent's file on the machine, in a shell of its own.
pub async fn read(client: &PeerClient) -> Result<Vec<ReadFile>, MeshError> {
    let script = if on_windows(client).await? {
        read_script_windows()
    } else {
        read_script()
    };
    let ran = exec(client, &script, LIMIT).await?;
    if ran.exit_code != Some(0) {
        return Err(said("its shell could not read the agent files"));
    }
    parse_read(&ran.output).map_err(|e| said(format!("{e:#}")))
}

/// Writes `text` into every agent file that lacks it.
pub async fn write(client: &PeerClient, files: &[ReadFile], text: &str) -> Result<(), MeshError> {
    if on_windows(client).await? {
        return run_write(client, write_script_windows(files, text)).await;
    }
    run_write(client, write_script(files, text)).await
}

/// Runs a write script; an empty one has nothing to do.
async fn run_write(client: &PeerClient, script: String) -> Result<(), MeshError> {
    if script.is_empty() {
        return Ok(());
    }
    let ran = exec(client, &script, LIMIT).await?;
    if ran.exit_code != Some(0) {
        return Err(said(format!(
            "writing the agent files failed: {}",
            ran.output.trim()
        )));
    }
    Ok(())
}

/// Whether the machine's shell is PowerShell on Windows. A POSIX shell prints
/// `:OS` for this command, PowerShell prints the `OS` variable, `Windows_NT`.
pub async fn on_windows(client: &PeerClient) -> Result<bool, MeshError> {
    let ran = exec(client, "echo $env:OS", LIMIT).await?;
    Ok(ran.output.trim() == "Windows_NT")
}

/// Prints `@@ <file> <state> [base64]`: 1 a file, 0 the folder without it, `-` neither.
fn read_script() -> String {
    let files: Vec<&str> = AGENT_FILES.iter().map(|(_, rel)| *rel).collect();
    let files = files.join(" ");
    // One byte past the limit, so a longer file shows as one.
    let limit = MAX_FILE + 1;
    format!(
        "for f in {files}; do \
           if [ -f \"$HOME/$f\" ]; then printf '@@ %s 1 ' \"$f\"; head -c {limit} \"$HOME/$f\" | base64 | tr -d '\\n'; echo; \
           elif [ -d \"$HOME/$(dirname \"$f\")\" ]; then printf '@@ %s 0\\n' \"$f\"; \
           else printf '@@ %s -\\n' \"$f\"; fi; \
         done"
    )
}

/// The PowerShell form of `read_script`, printing the same lines.
fn read_script_windows() -> String {
    let files: Vec<String> = AGENT_FILES
        .iter()
        .map(|(_, rel)| format!("'{rel}'"))
        .collect();
    let files = files.join(",");
    // One byte past the limit, so a longer file shows as one.
    let limit = MAX_FILE + 1;
    format!(
        "$ErrorActionPreference = 'Stop'; \
         foreach ($f in @({files})) {{ \
           $p = Join-Path $HOME $f; \
           if (Test-Path -LiteralPath $p -PathType Leaf) {{ \
             $s = [IO.File]::OpenRead($p); \
             try {{ \
               $b = New-Object byte[] {limit}; $n = 0; \
               while ($n -lt {limit}) {{ $r = $s.Read($b, $n, {limit} - $n); if ($r -le 0) {{ break }}; $n += $r }}; \
               Write-Output ('@@ ' + $f + ' 1 ' + [Convert]::ToBase64String($b, 0, $n)) \
             }} finally {{ $s.Dispose() }} \
           }} elseif (Test-Path -LiteralPath (Split-Path -Parent $p) -PathType Container) {{ Write-Output ('@@ ' + $f + ' 0') }} \
           else {{ Write-Output ('@@ ' + $f + ' -') }} \
         }}"
    )
}

fn parse_read(output: &str) -> anyhow::Result<Vec<ReadFile>> {
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
                if bytes.len() > MAX_FILE {
                    anyhow::bail!(
                        "~/{rel} is over {} KB, more than Beep reads. Shorten it to read or \
                         write this machine's agent files.",
                        MAX_FILE / 1024
                    );
                }
                Some(String::from_utf8_lossy(&bytes).into_owned())
            }
            "0" => None,
            _ => continue,
        };
        files.push(ReadFile { agent, rel, text });
    }
    if files.is_empty() && !output.contains("@@ ") {
        anyhow::bail!("its shell did not answer as expected");
    }
    Ok(files)
}

/// Whether `file` holds `text` between the markers.
fn holds(file: &str, text: &str) -> bool {
    file.contains(&block(text))
}

/// The marked block: the user's `text`, then Beep's rules.
fn block(text: &str) -> String {
    let text = text.trim_end();
    if text.is_empty() {
        format!("{START}\n{BEEP_RULES}\n{END}")
    } else {
        format!("{START}\n{text}\n\n{BEEP_RULES}\n{END}")
    }
}

/// `file` with `text` between the markers, replacing them or added at the end.
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

/// A script writing `text` into every file that lacks it; empty when all have it.
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

/// The PowerShell form of `write_script`: bytes, so no BOM and `\n` stays `\n`.
/// Stops at the first error with a non-zero exit.
fn write_script_windows(files: &[ReadFile], text: &str) -> String {
    let steps: Vec<String> = files
        .iter()
        .filter(|file| !file.text.as_deref().is_some_and(|now| holds(now, text)))
        .map(|file| {
            let data = STANDARD.encode(merged(file.text.as_deref(), text));
            let rel = file.rel;
            format!(
                "$p = Join-Path $HOME '{rel}'; \
                 [IO.File]::WriteAllBytes(($p + '.beep-new'), [Convert]::FromBase64String('{data}')); \
                 Move-Item -LiteralPath ($p + '.beep-new') -Destination $p -Force"
            )
        })
        .collect();
    if steps.is_empty() {
        return String::new();
    }
    format!(
        "$ErrorActionPreference = 'Stop'; try {{ {} }} catch {{ Write-Output $_.Exception.Message; exit 1 }}",
        steps.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_replaces_the_marked_block_and_keeps_the_rest() {
        let file = format!("# Mine\n\n{START}\nold\n{END}\n\n## After\n");
        assert_eq!(
            merged(Some(&file), "- new\n"),
            format!("# Mine\n\n{START}\n- new\n\n{BEEP_RULES}\n{END}\n\n## After\n")
        );
        assert_eq!(
            merged(Some("# Mine\n"), "rules"),
            format!("# Mine\n\n{START}\nrules\n\n{BEEP_RULES}\n{END}\n")
        );
        assert_eq!(
            merged(None, "rules"),
            format!("{START}\nrules\n\n{BEEP_RULES}\n{END}\n")
        );
        assert_eq!(merged(None, ""), format!("{START}\n{BEEP_RULES}\n{END}\n"));
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
    fn a_file_over_the_limit_is_refused_not_cut() {
        let full = format!(
            "@@ .claude/CLAUDE.md 1 {}\n",
            STANDARD.encode("x".repeat(MAX_FILE))
        );
        assert!(parse_read(&full).is_ok());
        let over = format!(
            "@@ .claude/CLAUDE.md 1 {}\n",
            STANDARD.encode("x".repeat(MAX_FILE + 1))
        );
        let error = parse_read(&over).expect_err("it is refused");
        assert!(
            error
                .to_string()
                .contains("~/.claude/CLAUDE.md is over 256 KB")
        );
        assert!(read_script().contains(&format!("head -c {}", MAX_FILE + 1)));
    }

    #[test]
    fn the_windows_read_script_lists_every_file_and_reads_past_the_limit() {
        let script = read_script_windows();
        for (_, rel) in AGENT_FILES {
            assert!(script.contains(&format!("'{rel}'")));
        }
        assert!(script.contains(&format!("byte[] {}", MAX_FILE + 1)));
        assert!(script.contains("Join-Path $HOME"));
        assert!(!script.contains("&&"));
    }

    #[test]
    fn the_windows_write_script_writes_bytes_and_skips_files_that_hold_the_text() {
        let held = merged(None, "rules");
        let files = [
            ReadFile {
                agent: "claude",
                rel: ".claude/CLAUDE.md",
                text: Some(held),
            },
            ReadFile {
                agent: "codex",
                rel: ".codex/AGENTS.md",
                text: None,
            },
        ];
        let script = write_script_windows(&files, "rules");
        assert!(!script.contains(".claude/CLAUDE.md"));
        assert!(script.contains("'.codex/AGENTS.md'"));
        assert!(script.contains(&STANDARD.encode(merged(None, "rules"))));
        assert!(script.contains("WriteAllBytes"));
        assert!(script.contains("Move-Item"));
        assert!(script.contains("exit 1"));
        assert!(!script.contains("&&"));
        assert!(write_script_windows(&files[..1], "rules").is_empty());
    }

    #[test]
    fn the_windows_read_output_parses_like_the_posix_one() {
        let output = format!(
            "#< CLIXML\r\n@@ .claude/CLAUDE.md 1 {}\r\n@@ .codex/AGENTS.md 0\r\n@@ .gemini/GEMINI.md -\r\n",
            STANDARD.encode("# Rules\n")
        );
        let files = parse_read(&output).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].text.as_deref(), Some("# Rules\n"));
        assert_eq!(files[1].text, None);
    }
}
