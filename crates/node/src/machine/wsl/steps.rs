//! What the Windows node runs through `wsl.exe`: listing the distributions,
//! and inside one, probing it, installing beep-node, running it and joining
//! it to the mesh. Inside a distribution everything runs in its `sh`.

use super::distro::{self, DistroName, Found};
use anyhow::{Context, Result, bail};
use beep_proto::node::harness::HARNESSES;
use std::{ffi::OsString, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

/// How long listing the distributions may take.
const LIST_LIMIT: Duration = Duration::from_secs(10);
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

/// The distributions, or only the running ones.
pub(crate) async fn list(running: bool) -> Result<Vec<DistroName>> {
    let printed = wsl(list_args(running), None, LIST_LIMIT).await?;
    Ok(distro::distros(&printed))
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

fn list_args(running: bool) -> Vec<OsString> {
    let running = running.then_some("--running");
    ["--list", "--quiet"]
        .into_iter()
        .chain(running)
        .map(OsString::from)
        .collect()
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

/// Prints `agent <name> <path>` for each coding agent found, also in the
/// usual per-user install folders a login shell may leave off `PATH`, and
/// `node beep-node <version>` and `id <id>` when beep-node is installed.
fn probe_script() -> String {
    let agents: Vec<&str> = HARNESSES.iter().map(|harness| harness.agent).collect();
    format!(
        "PATH=\"$HOME/.claude/local:$HOME/.local/bin:$HOME/.npm-global/bin:$HOME/.bun/bin:\
         $HOME/.volta/bin:$HOME/.local/share/pnpm:$PATH\"\n\
         for d in \"$HOME\"/.nvm/versions/node/*/bin; do [ -d \"$d\" ] && PATH=\"$PATH:$d\"; done\n\
         for a in {agents}; do p=$(command -v \"$a\") && echo \"agent $a $p\"; done\n\
         if [ -x {NODE} ]; then\n\
         \x20 echo \"node $({NODE} version)\"\n\
         \x20 i=$({NODE} id 2>/dev/null) && echo \"id $i\"\n\
         fi\n\
         exit 0\n",
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
