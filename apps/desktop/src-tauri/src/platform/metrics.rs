//! Bounded timings from the webviews, without page arguments or user data.

use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Timing {
    kind: String,
    name: String,
    outcome: String,
    ms: f64,
}

const PAGES: &[&str] = &[
    "home",
    "projects",
    "pane",
    "agent",
    "agentnew",
    "agents",
    "chat",
    "chats",
    "group",
    "ginfo",
    "gnew",
    "invite",
    "terminals",
    "research",
    "researchopen",
    "providers",
    "sync",
    "instructions",
    "memories",
    "routines",
    "rnew",
    "grants",
    "skills",
    "apps",
    "appperms",
    "mcp",
    "machines",
    "settings",
    "model",
    "link",
    "login",
    "starting",
    "joincode",
    "joining",
    "invite-phone",
    "revoked",
];
const COMMANDS: &[&str] = &[
    "machine_clocks",
    "project_sync_detail",
    "agent_worktrees",
    "session_copies",
    "session_moves",
    "session_move_progress",
    "agent_view",
    "agent_older",
    "agent_modes",
    "agent_tasks",
    "agent_sessions",
    "agent_hosts",
    "center_home",
    "project_sync",
    "project_overview",
    "center_project",
    "project_git_state",
    "center_project_lists",
    "center_project_issues",
    "center_project_pull_requests",
    "center_project_runs",
];

#[tauri::command]
pub(crate) fn desktop_trace_enabled() -> bool {
    std::env::var_os("BEEP_DESKTOP_TRACE_COMMANDS").is_some_and(|value| value == "1")
}

fn labels(t: &Timing) -> Option<(&'static str, &'static str, &'static str)> {
    if !t.ms.is_finite() || !(0.0..=600_000.0).contains(&t.ms) {
        return None;
    }

    let outcome = match t.outcome.as_str() {
        "ok" => "ok",
        "error" => "error",
        "timeout" => "timeout",
        _ => return None,
    };
    let (kind, names) = match t.kind.as_str() {
        "page" => ("page_mount_painted", PAGES),
        "data" => ("data_ready", &["agent"][..]),
        "command" => ("command", COMMANDS),
        _ => return None,
    };
    let name = *names.iter().find(|&&name| name == t.name)?;
    Some((kind, name, outcome))
}

/// Records a small batch so timing a call never recursively times itself.
#[tauri::command]
pub(crate) fn desktop_timings(samples: Vec<Timing>) {
    for sample in samples.into_iter().take(200) {
        let Some((kind, name, outcome)) = labels(&sample) else {
            continue;
        };
        metrics::counter!("beep_desktop_timings_total", "kind" => kind, "name" => name, "outcome" => outcome).increment(1);
        metrics::histogram!("beep_desktop_duration_seconds", "kind" => kind, "name" => name, "outcome" => outcome).record(sample.ms / 1000.0);
        let slow = if kind == "page_mount_painted" {
            250.0
        } else {
            1000.0
        };

        if sample.ms >= slow {
            log::info!("desktop {kind} {name} took {:.0} ms ({outcome})", sample.ms);
        }
    }
}

#[cfg(test)]
#[path = "../tests/platform_metrics.rs"]
mod tests;
