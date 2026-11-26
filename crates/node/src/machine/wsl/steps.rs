//! What the Windows node runs through `wsl.exe`: listing the distributions,
//! and inside one, probing it and running a coding agent.
//! Inside a distribution everything runs in its `sh`.

use super::distro::{self, DistroName, Found, LinuxPath, Listed};
use crate::machine::env::locate::{HOME_BINS, NODE_VERSIONS, SYSTEM_BINS};
use anyhow::{Context, Result, bail};
use beep_proto::node::harness::HARNESSES;
use std::{ffi::OsString, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

/// How long listing the distributions may take.
const LIST_LIMIT: Duration = Duration::from_secs(5);
/// How long probing a distribution may take, starting it included.
const PROBE_LIMIT: Duration = Duration::from_secs(30);
/// The most of a failed command's output kept for the reason.
const REASON_LINES: usize = 3;

/// The distributions and whether each runs, from one `--list --verbose`,
/// or where its states are in another language, two quiet lists.
pub(crate) async fn list() -> Result<Vec<Listed>> {
    let verbose = wsl(list_args(List::Verbose), LIST_LIMIT).await?;

    if let Some(listed) = distro::listed(&verbose) {
        return Ok(listed);
    }

    let all = wsl(list_args(List::All), LIST_LIMIT).await?;
    // Fails when none runs.
    let running = wsl(list_args(List::Running), LIST_LIMIT)
        .await
        .map(|printed| distro::distros(&printed))
        .unwrap_or_default();
    Ok(distro::distros(&all)
        .into_iter()
        .map(|name| Listed {
            running: running.contains(&name),
            default: false,
            name,
        })
        .collect())
}

/// The coding agents `distro` has. Starts it when it is not running.
pub(crate) async fn probe(distro: &DistroName) -> Result<Found> {
    let printed = wsl(in_distro(distro, &probe_script()), PROBE_LIMIT).await?;
    Ok(distro::found(&printed))
}

/// `wsl.exe` running the Linux `program` with `args` in `distro`, in the
/// folder `cwd` of the distribution or its home, with the `PATH` the probe found it on
/// and the variables a coding agent reads (`AGENT_VARS`) carried in from
/// Windows.
pub(crate) fn agent_command(
    distro: &DistroName,
    cwd: Option<&LinuxPath>,
    program: &LinuxPath,
    args: &[String],
) -> Command {
    let mut command = crate::run::process::command("wsl.exe");
    let distro: &std::ffi::OsStr = distro.as_ref();
    let script = format!("{}exec \"$0\" \"$@\"\n", path_prelude());
    command
        .arg("-d")
        .arg(distro)
        .args(["--cd", cwd.map_or("~", AsRef::as_ref), "--exec", "sh", "-lc", &script, program.as_ref()])
        .args(args)
        .env("WSLENV", wslenv());
    command
}

/// The variables set in this process that a coding agent reads: its
/// credentials and settings. `PATH` stays out, as WSL makes its own.
const AGENT_VARS: &[&str] = &["CLAUDE_", "ANTHROPIC_", "DISABLE_AUTO_COMPACT", "GH_TOKEN"];

/// `WSLENV` naming the variables that cross into a distribution: the ones
/// it already names and the `AGENT_VARS` this process has.
fn wslenv() -> String {
    let set: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| AGENT_VARS.iter().any(|var| name.starts_with(var)))
        // An agent's home set on Windows is a Windows path.
        .filter(|name| !HARNESSES.iter().any(|harness| harness.home_var == name))
        .collect();

    joined_wslenv(std::env::var("WSLENV").ok().as_deref(), set.iter().map(String::as_str))
}

/// `existing` with `names` added once each, `:` between them.
pub(crate) fn joined_wslenv<'a>(existing: Option<&'a str>, names: impl Iterator<Item = &'a str>) -> String {
    let mut all: Vec<&str> = existing
        .into_iter()
        .flat_map(|existing| existing.split(':'))
        .filter(|name| !name.is_empty())
        .collect();

    for name in names {
        if !all.contains(&name) {
            all.push(name);
        }
    }

    all.join(":")
}

/// Which list `wsl.exe` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum List {
    Verbose,
    All,
    Running,
}

fn list_args(list: List) -> Vec<OsString> {
    let args: &[&str] = match list {
        List::Verbose => &["--list", "--verbose"],
        List::All => &["--list", "--quiet"],
        List::Running => &["--list", "--quiet", "--running"],
    };
    args.iter().map(OsString::from).collect()
}

/// The arguments that run `script` in `distro`'s login `sh`, in its home.
fn in_distro(distro: &DistroName, script: &str) -> Vec<OsString> {
    let distro: &std::ffi::OsStr = distro.as_ref();
    [
        "-d".into(),
        distro.to_owned(),
        "--cd".into(),
        "~".into(),
        "--exec".into(),
        "sh".into(),
        "-lc".into(),
        script.into(),
    ]
    .into()
}

/// Prints `home <path>`, then `agent <name> <path>` for each coding agent
/// and `tool <name> <path>` for each ACP adapter and `npx` found, on `PATH`
/// or in the install folders the node itself looks in (`locate`).
fn probe_script() -> String {
    let agents: Vec<&str> = HARNESSES.iter().map(|harness| harness.agent).collect();
    let tools: Vec<&str> = crate::agents::adapter_bins().chain(["npx"]).collect();
    format!(
        "{prelude}\
         echo \"home $HOME\"\n\
         for a in {agents}; do p=$(command -v \"$a\") && echo \"agent $a $p\"; done\n\
         for t in {tools}; do p=$(command -v \"$t\") && echo \"tool $t $p\"; done\n\
         exit 0\n",
        prelude = path_prelude(),
        agents = agents.join(" "),
        tools = tools.join(" "),
    )
}

/// Adds the install folders the node itself looks in (`locate`) to `PATH`,
/// so an agent and the `node` it runs on are found.
fn path_prelude() -> String {
    let home: Vec<String> = HOME_BINS.iter().map(|dir| format!("$HOME/{dir}")).collect();
    let versions: Vec<String> = NODE_VERSIONS
        .iter()
        .map(|(versions, tail)| format!("\"$HOME/{versions}\"/*/{tail}"))
        .collect();
    format!(
        "PATH=\"$PATH:{home}:{system}\"\n\
         for d in {versions}; do [ -d \"$d\" ] && PATH=\"$PATH:$d\"; done\n",
        home = home.join(":"),
        system = SYSTEM_BINS.join(":"),
        versions = versions.join(" "),
    )
}

/// Runs `wsl.exe` with `args` for at most `limit`; what it printed on
/// success, else the last lines of it as the error.
async fn wsl(args: Vec<OsString>, limit: Duration) -> Result<String> {
    let mut command = crate::run::process::command("wsl.exe");
    command
        .args(args)
        // Its own messages in UTF-8 too.
        .env("WSL_UTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().context("could not start wsl.exe")?;

    let output = timeout(limit, child.wait_with_output())
        .await
        .with_context(|| format!("wsl.exe took over {} s", limit.as_secs()))??;
    let printed = distro::decode(&output.stdout);

    if !output.status.success() {
        let said = format!("{printed}\n{}", distro::decode(&output.stderr));
        bail!("{}", reason(&said));
    }

    Ok(printed)
}

/// The last lines of a failed command's output, as one line.
fn reason(said: &str) -> String {
    let lines: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return "wsl.exe failed and said nothing".to_owned();
    }

    lines[lines.len().saturating_sub(REASON_LINES)..].join(" ")
}

#[cfg(test)]
#[path = "../../tests/machine_wsl_steps.rs"]
mod tests;
