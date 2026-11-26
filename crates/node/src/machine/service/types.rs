//! What the managers share: how the service starts the node, which manager
//! it runs under, what a restart did, and the small text and process helpers
//! they all use. Nothing here names a manager's own file.

use crate::files::paths;
use anyhow::{Context, Result, bail};
use std::{
    env, fmt,
    path::{Path, PathBuf},
    process::Stdio,
};

/// How the service starts the node: the same command `run` serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The installed binary, with symlinks resolved.
    pub(super) exe: PathBuf,
    /// The node's key and settings folder (`--dir`).
    pub(super) dir: PathBuf,
    /// The user's environment folder (`--workspace`), if given.
    pub(super) workspace: Option<PathBuf>,
    /// The installing shell's `PATH`. A service starts with a bare one and
    /// would not find tools like `claude` or `gh` in the user's own folders.
    pub(super) search: String,
}

impl Launch {
    /// The launch for this binary, `dir` and `workspace`, with this shell's `PATH`.
    pub fn new(dir: &Path, workspace: Option<&Path>) -> Result<Self> {
        let exe = env::current_exe().context("finding this binary")?;
        let exe =
            paths::canonicalize(&exe).with_context(|| format!("resolving {}", exe.display()))?;
        let dir = paths::canonicalize(dir).with_context(|| {
            format!(
                "{} does not exist; run `beep-node mesh join <ticket>` first",
                dir.display()
            )
        })?;
        let workspace = workspace
            .map(std::path::absolute)
            .transpose()
            .context("resolving --workspace")?;
        // An unset PATH leaves the service with the manager's default one.
        let search = env::var("PATH").unwrap_or_default();
        Ok(Self {
            exe,
            dir,
            workspace,
            search,
        })
    }

    /// The node's arguments after the binary.
    pub(super) fn args(&self) -> Vec<String> {
        self.args_shown(|path| path.display().to_string())
    }

    /// The node's arguments, with paths written by `show`.
    pub(super) fn args_shown(&self, show: impl Fn(&Path) -> String) -> Vec<String> {
        let mut args = vec!["--dir".to_owned(), show(&self.dir)];
        if let Some(workspace) = &self.workspace {
            args.push("--workspace".to_owned());
            args.push(show(workspace));
        }
        args.push("run".to_owned());
        args
    }
}

/// The service manager the node runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    /// A systemd user unit (Linux).
    Systemd,
    /// A systemd system unit, run as root (Linux).
    SystemdSystem,
    /// An OpenRC service, run as root (Linux).
    OpenRc,
    /// A SysV init script, run as root (Linux).
    SysVInit,
    /// A LaunchAgent (macOS).
    Launchd,
    /// A scheduled task started at logon (Windows).
    TaskScheduler,
}

impl fmt::Display for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Systemd => "systemd user",
            Self::SystemdSystem => "systemd",
            Self::OpenRc => "OpenRC",
            Self::SysVInit => "SysV init",
            Self::Launchd => "launchd",
            Self::TaskScheduler => "Task Scheduler",
        })
    }
}

/// What restarting the node's service did, after its binary was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restart {
    /// The service was running and now runs the new binary.
    Restarted(Manager),
    /// The service is installed but was not running, so it was left stopped.
    Stopped(Manager),
    /// The service runs another binary than the one replaced; its unit,
    /// plist or task file.
    OtherBinary(PathBuf),
    /// No service of the node's is installed here.
    NotInstalled,
}

/// Runs `program` with `args`, failing with what it printed to stderr.
pub(super) fn run(program: &str, args: &[&str]) -> Result<()> {
    let out = crate::run::process::background_command(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("running {program}"))?;
    if !out.status.success() {
        bail!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    Ok(())
}

/// Text escaped for XML.
pub(super) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The argument after `--dir` in a node's command line.
pub(super) fn dir_arg(args: &[String]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == "--dir")?;
    args.get(at + 1).map(PathBuf::from)
}

/// Text with the five XML entities replaced.
pub(super) fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
