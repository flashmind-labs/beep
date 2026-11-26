//! How much of each coding agent's plan is left (`read_claude`,
//! `read_codex`, `read_agy`), from the agent's own CLI as signed in here.
//! None sends a model request, and the node never reads their token files.

use super::{

    unix_now,
    usage::{WEEK_MINUTES, claude_window},
};
use crate::process::{GroupLeader, STOP_GRACE, line_reader::{Line, LineReader}, own_group};
use anyhow::{Context, Result, bail};
use beep_proto::{
    AgentSlug,
    node::usage::{LimitWindow, MAX_WINDOW_MINUTES, PlanUsage},
};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
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

/// Antigravity's `/usage` command in print mode, which answers from the
/// quota service without a model turn or a saved conversation.
pub(super) const AGY_ARGS: &[&str] = &["-p", "/usage", "--output-format", "stream-json"];

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

    Ok(claude_plan(&answer["response"], unix_now()))
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

    Ok(codex_plan(&answer["result"], unix_now()))
}

/// Antigravity's plan usage; `None` when it reports no limits, such as on
/// an API key.
pub(super) async fn read_agy(command: Command) -> Result<Option<PlanUsage>> {
    let answer = exchange(command, Vec::new(), |line| {
        if line["event"] != "command_result" || line["command"]["name"] != "usage" {
            return Step::Wait;
        }

        Step::Done(line["command"]["data"].clone())
    })
    .await?;

    Ok(agy_plan(&answer, unix_now()))
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

/// The plan in Claude Code's answer to `get_usage`: `rate_limits` holds
/// each window it has, `utilization` from 0 to 100 and `resets_at` an
/// RFC 3339 time, and `model_scoped` the weekly windows of single models,
/// such as Fable, each with its `display_name`. A window not opened yet is
/// null.
fn claude_plan(usage: &Value, at: u64) -> Option<PlanUsage> {
    if usage["rate_limits_available"] != true {
        return None;
    }

    let limits = usage["rate_limits"].as_object()?;
    let window = |minutes, scope: Option<&str>, value: &Value| {
        Some(LimitWindow {
            minutes,
            used_percent: value.get("utilization")?.as_f64()?.clamp(0.0, 100.0),
            resets_at: value["resets_at"]
                .as_str()
                .and_then(crate::agents::found::unix_seconds),
            scope: scope.map(str::to_owned),
        })
    };
    let mut windows: Vec<LimitWindow> = limits
        .iter()
        .filter_map(|(kind, value)| {
            let (minutes, scope) = claude_window(kind)?;
            window(minutes, scope, value)
        })
        .collect();
    let scoped = limits
        .get("model_scoped")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let name = value["display_name"].as_str()?;
            window(WEEK_MINUTES, Some(name), value)
        })
        .collect::<Vec<_>>();

    for one in scoped {
        if !windows.iter().any(|known| known.scope == one.scope) {
            windows.push(one);
        }
    }

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
        credits: Vec::new(),
        account: None,
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
        credits: Vec::new(),
        account: None,
        at,
    })
}

/// The plan in Antigravity's `/usage`: `groups` of models that share
/// limits, each with `buckets` holding a `window` such as `weekly`,
/// `remaining_fraction` from 0 to 1 and `reset_time` in RFC 3339. A bucket
/// not used yet resets a full window from the moment it is asked, so its
/// time is left out.
fn agy_plan(data: &Value, at: u64) -> Option<PlanUsage> {
    let mut windows: Vec<LimitWindow> = data["groups"]
        .as_array()?
        .iter()
        .flat_map(|group| {
            let scope = group["name"].as_str();
            group["buckets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |bucket| {
                    let minutes = match bucket["window"].as_str()? {
                        "5h" => 5 * 60,
                        "weekly" => WEEK_MINUTES,
                        "daily" => 24 * 60,
                        _ => return None,
                    };
                    let left = bucket["remaining_fraction"].as_f64()?.clamp(0.0, 1.0);
                    Some(LimitWindow {
                        minutes,
                        used_percent: (1.0 - left) * 100.0,
                        resets_at: (left < 1.0)
                            .then(|| bucket["reset_time"].as_str())
                            .flatten()
                            .and_then(crate::agents::found::unix_seconds),
                        scope: scope.map(str::to_owned),
                    })
                })
        })
        .collect();
    if windows.is_empty() {
        return None;
    }

    windows.sort_by_key(|window| window.minutes);
    Some(PlanUsage {
        agent: AgentSlug::AGY,
        plan: None,
        windows,
        credits: Vec::new(),
        account: None,
        at,
    })
}

#[cfg(test)]
#[path = "../tests/agents_plans.rs"]
mod tests;
