//! `beep-node service`: runs the node under the system's service manager
//! (`Manager::install`): on Linux a systemd user unit, or for root without
//! one a systemd system unit, an OpenRC service or a SysV init script; a
//! LaunchAgent on macOS, a scheduled task on Windows.

use anyhow::{Result, bail};
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

mod existing;
pub use existing::Action;
mod initd;
mod launchd;
pub mod logs;
#[cfg(windows)]
mod parent;
mod systemd;
mod types;
pub use types::{Launch, Manager, Restart};
mod windows;
pub(crate) use windows::from_utf16;

/// Why the node cannot run as a service on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unusable {
    /// Linux without a systemd user instance, not run as root, where root
    /// could install a service for this init.
    NotRoot { init: Manager, why: String },
    /// A container with no init of its own: the container is the service.
    Container,
    /// Linux with neither a systemd user instance nor an init the node knows.
    NoInit(String),
    /// A system with no service manager the node knows.
    Os(&'static str),
}

impl fmt::Display for Unusable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRoot { init, why } => write!(
                f,
                "no systemd user service manager here ({why}). This machine runs {init}: rerun \
                 as root to install beep-node as its service, or run `beep-node run` in the \
                 foreground"
            ),
            Self::Container => f.write_str(
                "this is a container with no init system, so no service was installed. Make \
                 `beep-node run` the container's command and give it a restart policy, such \
                 as `docker run --restart unless-stopped`",
            ),
            Self::NoInit(why) => write!(
                f,
                "no systemd user service manager here ({why}) and no init system beep-node \
                 knows (systemd, OpenRC or SysV init); run `beep-node run` in the foreground"
            ),
            Self::Os(os) => write!(f, "beep-node cannot run as a service on {os}"),
        }
    }
}

impl std::error::Error for Unusable {}

/// The service manager on this machine, or why there is none to use.
pub fn manager() -> Result<Manager, Unusable> {
    choose(env::consts::OS, || {
        let exists = |path: &str| Path::new(path).exists();
        let container = in_container(exists, fs::read("/proc/1/environ").ok().as_deref());
        Linux {
            user: systemd::usable(),
            root: is_root(),
            container,
            init: running_init(container, exists),
        }
    })
}

/// What `choose` knows about a Linux machine.
struct Linux {
    /// Whether a systemd user instance answers; the reason when not.
    user: Result<(), String>,
    root: bool,
    container: bool,
    /// The init running the machine, as the manager root would use.
    init: Option<Manager>,
}

/// Picks the manager for `os`. `linux` looks at the machine; it is only
/// asked on Linux.
fn choose(os: &'static str, linux: impl FnOnce() -> Linux) -> Result<Manager, Unusable> {
    match os {
        "linux" => {
            let Linux {
                user,
                root,
                container,
                init,
            } = linux();
            let Err(why) = user else {
                return Ok(Manager::Systemd);
            };

            match init {
                Some(init) if root => Ok(init),
                Some(init) => Err(Unusable::NotRoot { init, why }),
                None if container => Err(Unusable::Container),
                None => Err(Unusable::NoInit(why)),
            }
        }
        "macos" => Ok(Manager::Launchd),
        "windows" => Ok(Manager::TaskScheduler),
        other => Err(Unusable::Os(other)),
    }
}

/// Whether this runs in a container: Docker's and Podman's marker files, or
/// the `container=` that LXC and systemd-nspawn give PID 1, in `environ`.
fn in_container(exists: impl Fn(&str) -> bool, environ: Option<&[u8]>) -> bool {
    exists("/.dockerenv")
        || exists("/run/.containerenv")
        || environ.is_some_and(|environ| {
            environ
                .split(|&byte| byte == 0)
                .any(|var| var.starts_with(b"container="))
        })
}

/// The init that runs this machine, by what it leaves in `/run`. A container
/// counts only an init that runs in it: its image may hold OpenRC's or
/// SysV's files with no init started.
fn running_init(container: bool, exists: impl Fn(&str) -> bool) -> Option<Manager> {
    // What sd_booted(3) checks.
    if exists("/run/systemd/system") {
        return Some(Manager::SystemdSystem);
    }

    if exists("/run/openrc") || (!container && exists("/sbin/openrc-run")) {
        return Some(Manager::OpenRc);
    }

    if !container && exists("/etc/init.d") {
        return Some(Manager::SysVInit);
    }

    None
}

fn is_root() -> bool {
    #[cfg(unix)]
    {
        rustix::process::geteuid().is_root()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// The manager whose service is installed here, if any. On Linux, the one
/// whose unit or script is there, whatever init runs now; elsewhere the one
/// manager, which itself says whether its service is installed.
fn present() -> Result<Option<Manager>> {
    if env::consts::OS != "linux" {
        return Ok(manager().ok());
    }

    if systemd::Scope::User.unit_path()?.exists() {
        return Ok(Some(Manager::Systemd));
    }

    if systemd::Scope::System.unit_path()?.exists() {
        return Ok(Some(Manager::SystemdSystem));
    }

    Ok(initd::installed()?.map(|init| match init {
        initd::Init::OpenRc => Manager::OpenRc,
        initd::Init::SysV => Manager::SysVInit,
    }))
}

impl Manager {
    /// Writes the service, starts it and makes it start again with the
    /// machine (or login). Replaces one already installed.
    pub fn install(self, launch: &Launch) -> Result<Installed> {
        match self {
            Self::Systemd => systemd::install(launch, systemd::Scope::User),
            Self::SystemdSystem => systemd::install(launch, systemd::Scope::System),
            Self::OpenRc => initd::install(launch, initd::Init::OpenRc),
            Self::SysVInit => initd::install(launch, initd::Init::SysV),
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
            Self::Systemd => systemd::uninstall(systemd::Scope::User),
            Self::SystemdSystem => systemd::uninstall(systemd::Scope::System),
            Self::OpenRc => initd::uninstall(initd::Init::OpenRc),
            Self::SysVInit => initd::uninstall(initd::Init::SysV),
            Self::Launchd => launchd::uninstall(),
            Self::TaskScheduler => windows::uninstall(),
        }
    }

    fn start(self) -> Result<()> {
        match self {
            Self::Systemd => systemd::start(systemd::Scope::User),
            Self::SystemdSystem => systemd::start(systemd::Scope::System),
            Self::OpenRc => initd::start(initd::Init::OpenRc),
            Self::SysVInit => initd::start(initd::Init::SysV),
            Self::Launchd => launchd::start(),
            Self::TaskScheduler => windows::start(),
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
            Manager::SystemdSystem => format!(
                "Beep runs in the background as the systemd service beep-node ({}).\n\
                 Logs: journalctl -u beep-node -f\n\
                 Stop: systemctl stop beep-node, or remove it with \
                 `beep-node service uninstall`",
                self.file.display()
            ),
            Manager::OpenRc => format!(
                "Beep runs in the background as the OpenRC service beep-node ({}) and \
                 starts with the machine.\n\
                 Logs: tail -f {}\n\
                 Stop: rc-service beep-node stop, or remove it with \
                 `beep-node service uninstall`",
                self.file.display(),
                initd::LOG
            ),
            Manager::SysVInit => format!(
                "Beep runs in the background as the SysV init service beep-node ({}) and \
                 starts with the machine. SysV init does not restart it if it stops.\n\
                 Logs: tail -f {}\n\
                 Stop: {} stop, or remove it with `beep-node service uninstall`",
                self.file.display(),
                initd::LOG,
                initd::SCRIPT
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
/// which ends only the process it started, stops the node too. Elsewhere
/// it finishes when stdin closes.
pub async fn parent_exited() -> Result<()> {
    #[cfg(windows)]
    {
        let parent = parent::open_parent()?;
        tokio::task::spawn_blocking(move || parent::wait(&parent)).await??;
        Ok(())
    }
    // The parent holds the other end, as a Windows node does for the node
    // it runs in WSL.
    #[cfg(not(windows))]
    {
        tokio::io::copy(&mut tokio::io::stdin(), &mut tokio::io::sink()).await?;
        Ok(())
    }
}

/// Restarts the installed service, if it runs `exe` (canonical, as
/// `Launch` keeps it), so it picks up a new binary at that path. A unit or
/// plist an older node wrote is written again first, with the same command
/// and `PATH`, so it gets this node's settings.
pub fn restart(exe: &Path) -> Result<Restart> {
    restart_plan(exe)?.run()
}

/// What `restart` does for `exe`, done later with `Restart::run`. Only an
/// older unit or plist is written again.
pub fn restart_plan(exe: &Path) -> Result<Restart> {
    match present()? {
        Some(Manager::Systemd) => systemd::plan(exe, systemd::Scope::User),
        Some(Manager::SystemdSystem) => systemd::plan(exe, systemd::Scope::System),
        Some(Manager::OpenRc) => initd::plan(exe, initd::Init::OpenRc),
        Some(Manager::SysVInit) => initd::plan(exe, initd::Init::SysV),
        Some(Manager::Launchd) => launchd::plan(exe),
        Some(Manager::TaskScheduler) => windows::plan(exe),
        None => Ok(Restart::NotInstalled),
    }
}

impl Restart {
    /// Restarts the service when planned by `restart_plan`, saying what it
    /// did.
    pub fn run(self) -> Result<Self> {
        match &self {
            Self::Restarted(Manager::Systemd) => systemd::restart(systemd::Scope::User)?,
            Self::Restarted(Manager::SystemdSystem) => {
                systemd::restart(systemd::Scope::System)?;
            }
            Self::Restarted(Manager::OpenRc) => initd::restart(initd::Init::OpenRc)?,
            Self::Restarted(Manager::SysVInit) => initd::restart(initd::Init::SysV)?,
            Self::Restarted(Manager::Launchd) => launchd::restart()?,
            Self::Restarted(Manager::TaskScheduler) => windows::restart()?,
            _ => {}
        }

        Ok(self)
    }
}

impl Manager {
    /// The command that starts the installed service, for a hint.
    pub fn start_command(self) -> &'static str {
        match self {
            Self::Systemd => "systemctl --user start beep-node",
            Self::SystemdSystem => "systemctl start beep-node",
            Self::OpenRc => "rc-service beep-node start",
            Self::SysVInit => "/etc/init.d/beep-node start",
            Self::Launchd => "launchctl kickstart gui/$(id -u)/com.flashmind.beep-node",
            Self::TaskScheduler => "schtasks /Run /TN \"Beep node\"",
        }
    }
}

/// What a join into `dir` does about the service. Fails when one for
/// another folder is installed, so a join there does not replace it.
/// Nothing is changed. Where no service manager is usable there is nothing
/// to replace.
pub fn join_action(dir: &Path) -> Result<Action> {
    let Some(manager) = present()? else {
        return Ok(Action::Install);
    };
    existing::decide(&existing::installed(manager)?, dir)
}

/// Starts the installed service without rewriting it.
pub fn start() -> Result<Manager> {
    let manager = match present()? {
        Some(manager) => manager,
        None => manager()?,
    };
    manager.start()?;
    Ok(manager)
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
    match present()? {
        Some(manager) => manager.uninstall(),
        None if env::consts::OS == "linux" => {
            println!("beep-node is not installed as a service.");
            Ok(())
        }
        None => bail!(Unusable::Os(env::consts::OS)),
    }
}

#[cfg(test)]
#[path = "../../tests/machine_service.rs"]
mod tests;
