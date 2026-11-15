//! What the Windows node runs through `wsl.exe`: listing the distributions,
//! and inside one, probing it, installing beep-node, running it and joining
//! it to the mesh. Inside a distribution everything runs in its `sh`.

use super::distro::{self, DistroName, Found, Listed};
use crate::agents::locate::{HOME_BINS, NODE_VERSIONS, SYSTEM_BINS};
use anyhow::{Context, Result, bail};
use beep_proto::node::harness::HARNESSES;
use std::{ffi::OsString, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

/// How long listing the distributions may take.
const LIST_LIMIT: Duration = Duration::from_secs(5);
/// How long asking a distribution whether its node runs may take.
const RUNNING_LIMIT: Duration = Duration::from_secs(20);
/// How long probing a distribution may take, starting it included.
const PROBE_LIMIT: Duration = Duration::from_secs(30);
/// How long installing beep-node in a distribution may take.
const INSTALL_LIMIT: Duration = Duration::from_secs(5 * 60);
/// How long joining may take: `mesh join` waits up to 15 seconds for the
/// node to listen, then for the desktop to take the ticket.
const JOIN_LIMIT: Duration = Duration::from_secs(90);
/// The most of a failed command's output kept for the reason.
const REASON_LINES: usize = 3;

/// The Linux installer, as Add a machine shows it.
const INSTALL_URL: &str = "https://releases.flashmind.com/beep/node/install.sh";
/// Where the installer puts beep-node inside a distribution.
const NODE: &str = "\"$HOME/.local/bin/beep-node\"";

/// The distributions and whether each runs, from one `--list --verbose`,
/// or where its states are in another language, two quiet lists.
pub(crate) async fn list() -> Result<Vec<Listed>> {
    let verbose = wsl(list_args(List::Verbose), None, LIST_LIMIT).await?;

    if let Some(listed) = distro::listed(&verbose) {
        return Ok(listed);
    }

    let all = wsl(list_args(List::All), None, LIST_LIMIT).await?;
    // Fails when none runs.
    let running = wsl(list_args(List::Running), None, LIST_LIMIT)
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

/// Whether a node already runs in `distro`, such as one another Beep on
/// this machine keeps. Starts the distribution.
pub(crate) async fn node_running(distro: &DistroName) -> bool {
    wsl(in_distro(distro, RUNNING), None, RUNNING_LIMIT)
        .await
        .is_ok()
}

/// What `distro` has: its coding agents and beep-node. Starts it when it
/// is not running.
pub(crate) async fn probe(distro: &DistroName) -> Result<Found> {
    let printed = wsl(in_distro(distro, &probe_script()), None, PROBE_LIMIT).await?;
    Ok(distro::found(&printed))
}

/// Installs the newest beep-node in `distro`, or updates it.
pub(crate) async fn install(distro: &DistroName) -> Result<()> {
    wsl(in_distro(distro, &install_script()), None, INSTALL_LIMIT).await?;
    Ok(())
}

/// Joins the node running in `distro` to the mesh with `ticket`, given on
/// stdin so it shows in no process list.
pub(crate) async fn join(distro: &DistroName, ticket: &str) -> Result<()> {
    wsl(in_distro(distro, JOIN), Some(ticket), JOIN_LIMIT).await?;
    Ok(())
}

/// `wsl.exe` with what runs the node in `distro` until its stdin closes.
pub(crate) fn run_command(distro: &DistroName) -> Command {
    let mut command = crate::run::process::command("wsl.exe");
    command.args(in_distro(distro, RUN)).env("WSL_UTF8", "1");
    command
}

/// `wsl.exe` running the Linux `program` with `args` in `distro`, in the
/// folder `cwd` of the distribution, with `forwarded` and the variables a
/// coding agent reads (`AGENT_VARS`) carried in from Windows.
pub(crate) fn agent_command(
    distro: &DistroName,
    cwd: &str,
    program: &str,
    args: &[String],
    forwarded: &[&str],
) -> Command {
    let mut command = crate::run::process::command("wsl.exe");
    let distro: &std::ffi::OsStr = distro.as_ref();
    command
        .arg("-d")
        .arg(distro)
        .args(["--cd", cwd, "--exec", program])
        .args(args)
        .env("WSLENV", wslenv(forwarded));
    command
}

/// The home folder of `distro`'s default user, as a Linux path, from its
/// own line among what the login shell's profile may print.
pub(crate) async fn home(distro: &DistroName) -> Result<String> {
    let printed = wsl(in_distro(distro, "printf 'home %s\\n' \"$HOME\""), None, RUNNING_LIMIT).await?;
    printed
        .lines()
        .find_map(|line| line.strip_prefix("home "))
        .map(str::to_owned)
        .context("the shell in WSL printed no home folder")
}

/// The variables set in this process that a coding agent reads: its
/// credentials and settings. `PATH` stays out, as WSL makes its own.
const AGENT_VARS: &[&str] = &["CLAUDE_", "ANTHROPIC_", "DISABLE_AUTO_COMPACT", "GH_TOKEN"];

/// `WSLENV` naming the variables that cross into a distribution: the ones
/// it already names, `forwarded`, and the `AGENT_VARS` this process has.
fn wslenv(forwarded: &[&str]) -> String {
    let set: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| AGENT_VARS.iter().any(|var| name.starts_with(var)))
        .collect();
    let names = forwarded.iter().copied().chain(set.iter().map(String::as_str));

    joined_wslenv(std::env::var("WSLENV").ok().as_deref(), names)
}

/// `existing` with `names` added once each, `:` between them.
fn joined_wslenv<'a>(existing: Option<&'a str>, names: impl Iterator<Item = &'a str>) -> String {
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

/// Prints `agent <name> <path>` for each coding agent found, on `PATH` or
/// in the install folders the node itself looks in (`locate`), and
/// `node beep-node <version>` and `id <id>` when beep-node is installed.
fn probe_script() -> String {
    let agents: Vec<&str> = HARNESSES.iter().map(|harness| harness.agent).collect();
    let home: Vec<String> = HOME_BINS.iter().map(|dir| format!("$HOME/{dir}")).collect();
    let versions: Vec<String> = NODE_VERSIONS
        .iter()
        .map(|(versions, tail)| format!("\"$HOME/{versions}\"/*/{tail}"))
        .collect();
    format!(
        "PATH=\"$PATH:{home}:{system}\"\n\
         for d in {versions}; do [ -d \"$d\" ] && PATH=\"$PATH:$d\"; done\n\
         for a in {agents}; do p=$(command -v \"$a\") && echo \"agent $a $p\"; done\n\
         if [ -x {NODE} ]; then\n\
         \x20 echo \"node $({NODE} version)\"\n\
         \x20 i=$({NODE} id 2>/dev/null) && echo \"id $i\"\n\
         fi\n\
         exit 0\n",
        home = home.join(":"),
        system = SYSTEM_BINS.join(":"),
        versions = versions.join(" "),
        agents = agents.join(" "),
    )
}

/// Runs the Linux installer, with curl or wget.
fn install_script() -> String {
    format!(
        "url={INSTALL_URL}\n\
         if command -v curl >/dev/null 2>&1; then script=$(curl -fsSL \"$url\")\n\
         elif command -v wget >/dev/null 2>&1; then script=$(wget -qO- \"$url\")\n\
         else echo 'installing beep-node needs curl or wget in WSL' >&2; exit 1\n\
         fi || exit 1\n\
         printf '%s\\n' \"$script\" | sh\n"
    )
}

/// Runs the node until its stdin closes (`--with-parent`), without the
/// Windows folders WSL appends to `PATH`: a WSL machine runs Linux tools.
const RUN: &str = "PATH=$(printf %s \"$PATH\" | tr : '\\n' | grep -v '^/mnt/' | paste -sd : -)\n\
                   exec \"$HOME/.local/bin/beep-node\" --with-parent run\n";

/// Succeeds when a node runs from the default folder, which the nodes a
/// Windows node runs use.
const RUNNING: &str = "exec \"$HOME/.local/bin/beep-node\" status --quiet\n";

/// Joins the running node with the ticket on stdin, never starting a
/// service of its own.
const JOIN: &str = "exec \"$HOME/.local/bin/beep-node\" mesh join --no-service -\n";

/// Runs `wsl.exe` with `args`, `input` on its stdin, for at most `limit`;
/// what it printed on success, else the last lines of it as the error.
async fn wsl(args: Vec<OsString>, input: Option<&str>, limit: Duration) -> Result<String> {
    let mut command = crate::run::process::command("wsl.exe");
    command
        .args(args)
        // Its own messages in UTF-8 too.
        .env("WSL_UTF8", "1")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("could not start wsl.exe")?;

    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input.as_bytes()).await?;
    }

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
