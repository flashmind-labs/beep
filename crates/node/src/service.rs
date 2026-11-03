//! `beep-node service`: runs the node in the background under the system's
//! service manager, so it starts with the machine (or the login, on macOS
//! and Windows) and comes back when it stops. Linux uses a systemd user
//! unit, macOS a LaunchAgent, Windows a scheduled task.

use anyhow::{Context, Result, bail};
use std::{
    env, fmt,
    path::{Path, PathBuf},
};

mod launchd;
pub mod logs;
#[cfg(windows)]
mod parent;
mod systemd;
mod windows;

/// How the service starts the node: the same command `setup` would serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The installed binary, with symlinks resolved.
    exe: PathBuf,
    /// The node's key and settings folder (`--dir`).
    dir: PathBuf,
    /// The user's environment folder (`--workspace`), if given.
    workspace: Option<PathBuf>,
    /// The installing shell's `PATH`. A service starts with a bare one and
    /// would not find tools like `claude` or `gh` in the user's own folders.
    search: String,
}

impl Launch {
    /// The launch for this binary, `dir` and `workspace`, with this shell's `PATH`.
    pub fn new(dir: &Path, workspace: Option<&Path>) -> Result<Self> {
        let exe = env::current_exe().context("finding this binary")?;
        let exe = exe
            .canonicalize()
            .with_context(|| format!("resolving {}", exe.display()))?;
        let dir = dir.canonicalize().with_context(|| {
            format!(
                "{} does not exist; run `beep-node setup` first",
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
    fn args(&self) -> Vec<String> {
        self.args_shown(|path| path.display().to_string())
    }

    /// The node's arguments, with paths written by `show`.
    fn args_shown(&self, show: impl Fn(&Path) -> String) -> Vec<String> {
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
    /// A LaunchAgent (macOS).
    Launchd,
    /// A scheduled task started at logon (Windows).
    TaskScheduler,
}

/// Why the node cannot run as a service on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unusable {
    /// Linux without a reachable systemd user instance, as in most
    /// containers.
    NoSystemd(String),
    /// A system with no service manager the node knows.
    Os(&'static str),
}

impl fmt::Display for Unusable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSystemd(why) => write!(f, "no systemd user service manager here ({why})"),
            Self::Os(os) => write!(f, "beep-node cannot run as a service on {os}"),
        }
    }
}

impl std::error::Error for Unusable {}

/// The service manager on this machine, or why there is none to use.
pub fn manager() -> Result<Manager, Unusable> {
    choose(env::consts::OS, systemd::usable)
}

/// Picks the manager for `os`. `systemd` says whether a systemd user
/// instance answers; it is only asked on Linux.
fn choose(
    os: &'static str,
    systemd: impl FnOnce() -> Result<(), String>,
) -> Result<Manager, Unusable> {
    match os {
        "linux" => systemd()
            .map(|()| Manager::Systemd)
            .map_err(Unusable::NoSystemd),
        "macos" => Ok(Manager::Launchd),
        "windows" => Ok(Manager::TaskScheduler),
        other => Err(Unusable::Os(other)),
    }
}

impl Manager {
    /// Writes the service, starts it and makes it start again with the
    /// machine (or login). Replaces one already installed.
    pub fn install(self, launch: &Launch) -> Result<Installed> {
        match self {
            Self::Systemd => systemd::install(launch),
            Self::Launchd => launchd::install(launch),
            Self::TaskScheduler => windows::install(launch),
        }
        .map(|file| Installed {
            manager: self,
            file,
        })
    }

    /// Stops the service and removes it.
    pub fn uninstall(self) -> Result<()> {
        match self {
            Self::Systemd => systemd::uninstall(),
            Self::Launchd => launchd::uninstall(),
            Self::TaskScheduler => windows::uninstall(),
        }
    }
}

/// A service that was installed and started.
#[derive(Debug)]
pub struct Installed {
    pub manager: Manager,
    /// The unit file or plist written.
    pub file: PathBuf,
}

impl Installed {
    /// Where it runs and how to follow or stop it, for printing.
    pub fn describe(&self) -> Result<String> {
        Ok(match self.manager {
            Manager::Systemd => format!(
                "Beep runs in the background as the systemd user service beep-node \
                 ({}).\n\
                 Logs: journalctl --user -u beep-node -f\n\
                 Stop: systemctl --user stop beep-node, or remove it with \
                 `beep-node service uninstall`",
                self.file.display()
            ),
            Manager::Launchd => format!(
                "Beep runs in the background as the LaunchAgent {} ({}) and \
                 starts when you log in.\n\
                 Logs: tail -f {}\n\
                 Stop and remove it: beep-node service uninstall",
                launchd::LABEL,
                self.file.display(),
                launchd::log_path()?.display()
            ),
            Manager::TaskScheduler => format!(
                "Beep runs in the background as the scheduled task {:?} ({}) and \
                 starts when you log in.\n\
                 Logs: {}\n\
                 Stop and remove it: beep-node service uninstall",
                windows::TASK,
                self.file.display(),
                windows::log_dir()?.display()
            ),
        })
    }
}

/// Finishes when the process that started this one exits: on Windows, the
/// service's node passes `--with-parent` so stopping the scheduled task,
/// which ends only the process it started, stops the node too. Never
/// finishes elsewhere.
pub async fn parent_exited() -> Result<()> {
    #[cfg(windows)]
    {
        let parent = parent::open_parent()?;
        tokio::task::spawn_blocking(move || parent::wait(&parent)).await??;
        Ok(())
    }
    #[cfg(not(windows))]
    std::future::pending().await
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

/// Restarts the service `setup` installed, if it runs `exe` (canonical, as
/// `Launch` keeps it), so it picks up a new binary at that path.
pub fn restart(exe: &Path) -> Result<Restart> {
    match env::consts::OS {
        "linux" => systemd::restart(exe),
        "macos" => launchd::restart(exe),
        "windows" => windows::restart(exe),
        _ => Ok(Restart::NotInstalled),
    }
}

impl Manager {
    /// The command that starts the installed service, for a hint.
    pub fn start_command(self) -> &'static str {
        match self {
            Self::Systemd => "systemctl --user start beep-node",
            Self::Launchd => "launchctl kickstart gui/$(id -u)/com.flashmind.beep-node",
            Self::TaskScheduler => "schtasks /Run /TN \"Beep node\"",
        }
    }
}

/// `beep-node service install`.
pub fn install(dir: &Path, workspace: Option<&Path>) -> Result<()> {
    let manager = manager()?;
    let installed = manager.install(&Launch::new(dir, workspace)?)?;
    println!("{}", installed.describe()?);
    Ok(())
}

/// `beep-node service uninstall`.
pub fn uninstall() -> Result<()> {
    match env::consts::OS {
        "linux" => Manager::Systemd.uninstall(),
        "macos" => Manager::Launchd.uninstall(),
        "windows" => Manager::TaskScheduler.uninstall(),
        other => bail!(Unusable::Os(other)),
    }
}

/// Text escaped for XML.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn launch(workspace: Option<&str>) -> Launch {
        Launch {
            exe: PathBuf::from("/opt/beep node/beep-node"),
            dir: PathBuf::from("/srv/node"),
            workspace: workspace.map(PathBuf::from),
            search: "/usr/bin:/home/user/50%/bin".to_owned(),
        }
    }

    #[test]
    fn linux_uses_systemd_only_when_a_user_instance_answers() {
        assert_eq!(choose("linux", || Ok(())), Ok(Manager::Systemd));
        assert_eq!(
            choose("linux", || Err("no bus".to_owned())),
            Err(Unusable::NoSystemd("no bus".to_owned()))
        );
    }

    #[test]
    fn macos_uses_launchd_without_asking_systemd() {
        let asked = choose("macos", || panic!("systemd asked on macOS"));
        assert_eq!(asked, Ok(Manager::Launchd));
    }

    #[test]
    fn windows_uses_task_scheduler() {
        let asked = choose("windows", || panic!("systemd asked on Windows"));
        assert_eq!(asked, Ok(Manager::TaskScheduler));
    }

    #[test]
    fn other_systems_serve_in_the_foreground() {
        assert_eq!(choose("freebsd", || Ok(())), Err(Unusable::Os("freebsd")));
    }

    #[test]
    fn the_node_runs_with_its_folder_and_workspace() {
        assert_eq!(launch(None).args(), ["--dir", "/srv/node", "run"]);
        assert_eq!(
            launch(Some("/home/user/work")).args(),
            [
                "--dir",
                "/srv/node",
                "--workspace",
                "/home/user/work",
                "run"
            ]
        );
    }
}
