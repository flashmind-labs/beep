//! `beep-node service`: runs the node under the system's service manager
//! (`Manager::install`): on Linux a systemd user unit, or for root without
//! one a systemd system unit, an OpenRC service or a SysV init script; a
//! LaunchAgent on macOS, a scheduled task on Windows.

use crate::files::paths;
use anyhow::{Context, Result, bail};
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

mod existing;
pub use existing::Action;
mod initd;
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
        let exe =
            paths::canonicalize(&exe).with_context(|| format!("resolving {}", exe.display()))?;
        let dir = paths::canonicalize(dir).with_context(|| {
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
/// `Launch` keeps it), so it picks up a new binary at that path. A unit or
/// plist an older node wrote is written again first, with the same command
/// and `PATH`, so it gets this node's settings.
pub fn restart(exe: &Path) -> Result<Restart> {
    restart_plan(exe)?.run()
}

/// What `restart` does for `exe`, done later with `Plan::run`. Only an
/// older unit or plist is written again.
pub fn restart_plan(exe: &Path) -> Result<Plan> {
    let plain = |restart| Plan {
        restart,
        reload: false,
    };
    match present()? {
        Some(Manager::Systemd) => systemd::plan(exe, systemd::Scope::User).map(plain),
        Some(Manager::SystemdSystem) => systemd::plan(exe, systemd::Scope::System).map(plain),
        Some(Manager::OpenRc) => initd::plan(exe, initd::Init::OpenRc).map(plain),
        Some(Manager::SysVInit) => initd::plan(exe, initd::Init::SysV).map(plain),
        Some(Manager::Launchd) => launchd::plan(exe),
        Some(Manager::TaskScheduler) => windows::plan(exe).map(plain),
        None => Ok(plain(Restart::NotInstalled)),
    }
}

/// A restart planned by `restart_plan`.
#[derive(Debug)]
pub struct Plan {
    pub restart: Restart,
    /// The plist was written again, so launchd loads it anew.
    reload: bool,
}

impl Plan {
    #[cfg(test)]
    pub(crate) fn new(restart: Restart) -> Self {
        Self {
            restart,
            reload: false,
        }
    }

    /// Restarts the service when planned, saying what it did.
    pub fn run(self) -> Result<Restart> {
        match self.restart {
            Restart::Restarted(Manager::Systemd) => systemd::restart(systemd::Scope::User)?,
            Restart::Restarted(Manager::SystemdSystem) => {
                systemd::restart(systemd::Scope::System)?;
            }
            Restart::Restarted(Manager::OpenRc) => initd::restart(initd::Init::OpenRc)?,
            Restart::Restarted(Manager::SysVInit) => initd::restart(initd::Init::SysV)?,
            Restart::Restarted(Manager::Launchd) => launchd::restart(self.reload)?,
            Restart::Restarted(Manager::TaskScheduler) => windows::restart()?,
            _ => {}
        }
        Ok(self.restart)
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

/// Runs `program` with `args`, failing with what it printed to stderr.
fn run(program: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(program)
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

    fn linux(
        user: bool,
        root: bool,
        container: bool,
        init: Option<Manager>,
    ) -> Result<Manager, Unusable> {
        let user = if user {
            Ok(())
        } else {
            Err("no bus".to_owned())
        };
        choose("linux", || Linux {
            user,
            root,
            container,
            init,
        })
    }

    #[test]
    fn a_user_instance_comes_first() {
        let init = Some(Manager::SystemdSystem);
        assert_eq!(linux(true, false, false, init), Ok(Manager::Systemd));
        assert_eq!(linux(true, true, false, init), Ok(Manager::Systemd));
    }

    #[test]
    fn root_without_a_user_instance_uses_the_init() {
        for init in [Manager::SystemdSystem, Manager::OpenRc, Manager::SysVInit] {
            assert_eq!(linux(false, true, false, Some(init)), Ok(init));
        }
    }

    #[test]
    fn others_are_told_to_be_root_or_run_it_themselves() {
        let e = linux(false, false, false, Some(Manager::OpenRc)).unwrap_err();
        assert_eq!(
            e,
            Unusable::NotRoot {
                init: Manager::OpenRc,
                why: "no bus".to_owned()
            }
        );
        let said = e.to_string();
        assert!(said.contains("runs OpenRC: rerun as root"), "{said}");
        assert!(said.contains("`beep-node run`"), "{said}");
        assert_eq!(
            linux(false, true, false, None),
            Err(Unusable::NoInit("no bus".to_owned()))
        );
    }

    #[test]
    fn a_container_without_an_init_is_its_own_service() {
        assert_eq!(linux(false, true, true, None), Err(Unusable::Container));
        assert_eq!(linux(false, false, true, None), Err(Unusable::Container));
        let said = Unusable::Container.to_string();
        assert!(said.contains("--restart unless-stopped"), "{said}");
        // One that runs an init is a machine like any other.
        assert_eq!(
            linux(false, true, true, Some(Manager::OpenRc)),
            Ok(Manager::OpenRc)
        );
    }

    #[test]
    fn the_init_is_what_runs_not_what_is_installed() {
        let on = |paths: &'static [&'static str]| move |path: &str| paths.contains(&path);
        // Debian on systemd keeps /etc/init.d.
        assert_eq!(
            running_init(false, on(&["/run/systemd/system", "/etc/init.d"])),
            Some(Manager::SystemdSystem)
        );
        assert_eq!(
            running_init(false, on(&["/sbin/openrc-run", "/etc/init.d"])),
            Some(Manager::OpenRc)
        );
        assert_eq!(
            running_init(false, on(&["/etc/init.d"])),
            Some(Manager::SysVInit)
        );
        assert_eq!(running_init(false, on(&[])), None);
        // A container image holds these files with no init started.
        assert_eq!(
            running_init(true, on(&["/sbin/openrc-run", "/etc/init.d"])),
            None
        );
        assert_eq!(
            running_init(true, on(&["/run/openrc", "/etc/init.d"])),
            Some(Manager::OpenRc)
        );
        assert_eq!(
            running_init(true, on(&["/run/systemd/system"])),
            Some(Manager::SystemdSystem)
        );
    }

    #[test]
    fn containers_are_known_by_marker_or_by_pid_1() {
        let none = |_: &str| false;
        assert!(!in_container(none, None));
        assert!(!in_container(none, Some(b"PATH=/bin\0HOME=/\0")));
        assert!(in_container(none, Some(b"PATH=/bin\0container=lxc\0")));
        assert!(in_container(|path| path == "/.dockerenv", None));
        assert!(in_container(|path| path == "/run/.containerenv", None));
    }
}
