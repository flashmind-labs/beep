//! Reads how much of each coding agent's plan is left from the agent's
//! own CLI, signed in as it is on this machine: Claude Code answers the
//! `get_usage` control request in stream-json mode without a prompt, and
//! Codex's app-server answers `account/rateLimits/read`. Neither sends a
//! model request; the node never reads their token files.

use super::{
    acp::{Line, LineReader},
    usage::claude_window,
};
use crate::process::{GroupLeader, STOP_GRACE, own_group};
use anyhow::{Context, Result, bail};
use beep_proto::{
    AgentSlug,
    node::usage::{LimitWindow, MAX_WINDOW_MINUTES, PlanUsage},
};
use serde_json::{Value, json};
use std::{
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    time::timeout,
};

/// How long one read may take, the CLI's start included.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest line read from the CLI; a longer one is skipped.
const MAX_LINE: usize = 1024 * 1024;
/// The most lines read before giving up on an answer.
const MAX_LINES: usize = 200;

/// Claude Code without hooks, MCP servers, settings files or a saved
/// session: it starts, answers the control request and exits once its
/// input ends.
pub(super) const CLAUDE_ARGS: &[&str] = &[
    "-p",
    "--output-format",
    "stream-json",
    "--verbose",
    "--input-format",
    "stream-json",
    "--no-session-persistence",
    "--strict-mcp-config",
    "--setting-sources",
    "",
];

/// Codex's JSON-RPC server on stdio.
pub(super) const CODEX_ARGS: &[&str] = &["app-server"];

/// What the CLI is answered with next.
enum Step {
    Send(Vec<Value>),
    Done(Value),
    Wait,
}

/// Claude Code's plan usage; `None` when its plan has no rate limits, such
/// as an API key.
pub(super) async fn read_claude(command: Command) -> Result<Option<PlanUsage>> {
    let request = json!({
        "type": "control_request",
        "request_id": "beep-usage",
        "request": {"subtype": "get_usage", "skip_behaviors": true},
    });
    let answer = exchange(command, vec![request], |line| {
        if line["type"] != "control_response" || line["response"]["request_id"] != "beep-usage" {
            return Step::Wait;
        }
        Step::Done(line["response"].clone())
    })
    .await?;
    if answer["subtype"] != "success" {
        bail!("Claude Code refused: {}", answer["error"]);
    }
    Ok(claude_plan(&answer["response"], now()))
}

/// Codex's plan usage; `None` when it is signed in with an API key.
pub(super) async fn read_codex(command: Command) -> Result<Option<PlanUsage>> {
    let initialize = json!({
        "id": 1,
        "method": "initialize",
        "params": {"clientInfo": {"name": "beep-node", "version": env!("CARGO_PKG_VERSION")}},
    });
    let answer = exchange(command, vec![initialize], |line| {
        match line["id"].as_u64() {
            Some(1) => Step::Send(vec![
                json!({"method": "initialized"}),
                json!({"id": 2, "method": "account/rateLimits/read"}),
            ]),
            Some(2) => Step::Done(line.clone()),
            _ => Step::Wait,
        }
    })
    .await?;
    if let Some(error) = answer.get("error") {
        bail!("Codex refused: {}", error["message"]);
    }
    Ok(codex_plan(&answer["result"], now()))
}

/// Starts `command`, sends it `first` as JSON lines and answers each line
/// it writes by `step`, until `step` gives the answer.
async fn exchange(
    mut command: Command,
    first: Vec<Value>,
    mut step: impl FnMut(&Value) -> Step,
) -> Result<Value> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = own_group(&mut command)
        .spawn()
        .context("starting the agent's CLI")?;
    // Stops the CLI and what it started however this ends.
    let mut child = GroupLeader::new(child);
    let talk = async {
        let process = child.child_mut();
        let mut stdin = process.stdin.take().context("the CLI has no stdin")?;
        let stdout = process.stdout.take().context("the CLI has no stdout")?;
        let mut lines = LineReader::new(BufReader::new(stdout), MAX_LINE);
        write(&mut stdin, &first).await?;
        for _ in 0..MAX_LINES {
            let Some(line) = lines.next().await? else {
                bail!("the CLI exited without answering");
            };
            let Line::Text(line) = line else { continue };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match step(&message) {
                Step::Send(more) => write(&mut stdin, &more).await?,
                Step::Done(answer) => return Ok(answer),
                Step::Wait => {}
            }
        }
        bail!("the CLI did not answer in {MAX_LINES} lines")
    };
    let answer = timeout(READ_TIMEOUT, talk)
        .await
        .context("the CLI did not answer in time")?;
    child.stop(STOP_GRACE).await;
    answer
}

async fn write(stdin: &mut ChildStdin, messages: &[Value]) -> Result<()> {
    for message in messages {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        stdin.write_all(&line).await?;
    }
    stdin.flush().await?;
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The plan in Claude Code's answer to `get_usage`: `rate_limits` holds
/// each window it has, `utilization` from 0 to 100 and `resets_at` an
/// RFC 3339 time. A window not opened yet is null.
fn claude_plan(usage: &Value, at: u64) -> Option<PlanUsage> {
    if usage["rate_limits_available"] != true {
        return None;
    }
    let limits = usage["rate_limits"].as_object()?;
    let mut windows: Vec<LimitWindow> = limits
        .iter()
        .filter_map(|(kind, value)| {
            let (minutes, scope) = claude_window(kind)?;
            let used = value.get("utilization")?.as_f64()?;
            Some(LimitWindow {
                minutes,
                used_percent: used.clamp(0.0, 100.0),
                resets_at: value["resets_at"]
                    .as_str()
                    .and_then(crate::claude::unix_seconds),
                scope: scope.map(str::to_owned),
            })
        })
        .collect();
    // A five-hour window not opened yet is all left.
    if !windows.iter().any(|window| window.minutes == 300) {
        windows.push(LimitWindow {
            minutes: 300,
            used_percent: 0.0,
            resets_at: None,
            scope: None,
        });
    }
    windows.sort_by_key(|window| window.minutes);
    Some(PlanUsage {
        agent: AgentSlug::CLAUDE,
        plan: usage["subscription_type"].as_str().map(str::to_owned),
        windows,
        at,
    })
}

/// The plan in Codex's answer to `account/rateLimits/read`: `primary` and
/// `secondary` windows, each `usedPercent` with `windowDurationMins` and
/// `resetsAt` in Unix seconds.
fn codex_plan(result: &Value, at: u64) -> Option<PlanUsage> {
    let limits = &result["rateLimits"];
    let windows: Vec<LimitWindow> = ["primary", "secondary"]
        .iter()
        .filter_map(|which| {
            let window = &limits[*which];
            let minutes = u32::try_from(window["windowDurationMins"].as_u64()?).ok()?;
            Some(LimitWindow {
                minutes: minutes.min(MAX_WINDOW_MINUTES),
                used_percent: window["usedPercent"].as_f64()?.clamp(0.0, 100.0),
                resets_at: window["resetsAt"].as_u64(),
                scope: None,
            })
        })
        .collect();
    if windows.is_empty() {
        return None;
    }
    let mut windows = windows;
    windows.sort_by_key(|window| window.minutes);
    Some(PlanUsage {
        agent: AgentSlug::CODEX,
        plan: limits["planType"].as_str().map(str::to_owned),
        windows,
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_answer_gives_its_windows() {
        let answer = json!({
            "subscription_type": "max",
            "rate_limits_available": true,
            "rate_limits": {
                "five_hour": null,
                "seven_day": {"utilization": 2, "resets_at": "2026-11-10T01:00:00.586471+00:00"},
                "seven_day_opus": {"utilization": 40.5, "resets_at": null},
                "extra_usage": {"is_enabled": false},
            },
        });
        let plan = claude_plan(&answer, 7).unwrap();
        assert_eq!(plan.plan.as_deref(), Some("max"));
        assert_eq!(plan.at, 7);
        let shown: Vec<(u32, f64, Option<&str>)> = plan
            .windows
            .iter()
            .map(|w| (w.minutes, w.used_percent, w.scope.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                (300, 0.0, None),
                (10080, 2.0, None),
                (10080, 40.5, Some("Opus"))
            ]
        );
        assert_eq!(plan.windows[1].resets_at, Some(1_794_272_400));
        let api_key = json!({"rate_limits_available": false, "rate_limits": null});
        assert_eq!(claude_plan(&api_key, 7), None);
    }

    #[test]
    fn codex_answer_gives_its_windows() {
        let answer = json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": {"usedPercent": 74, "windowDurationMins": 10080, "resetsAt": 1_793_775_136},
                "secondary": null,
                "planType": "pro",
            },
        });
        let plan = codex_plan(&answer, 9).unwrap();
        assert_eq!(plan.plan.as_deref(), Some("pro"));
        assert_eq!(plan.windows.len(), 1);
        assert_eq!(plan.windows[0].minutes, 10080);
        assert_eq!(plan.windows[0].used_percent, 74.0);
        assert_eq!(plan.windows[0].resets_at, Some(1_793_775_136));
        assert_eq!(codex_plan(&json!({"rateLimits": {}}), 9), None);
    }

    /// A CLI that answers a control request as Claude Code does.
    #[cfg(unix)]
    #[tokio::test]
    async fn reads_a_cli_that_answers() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            r#"read line; echo '{"type":"system"}'; echo '{"type":"control_response","response":{"subtype":"success","request_id":"beep-usage","response":{"subscription_type":"pro","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":38,"resets_at":null}}}}}'; cat >/dev/null"#,
        ]);
        let plan = read_claude(command).await.unwrap().unwrap();
        assert_eq!(plan.plan.as_deref(), Some("pro"));
        assert_eq!(plan.windows[0].used_percent, 38.0);
    }
}
