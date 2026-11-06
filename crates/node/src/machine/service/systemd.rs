//! The node as a systemd unit: a user unit, kept running after logout and
//! started at boot by lingering, or, for root without a user instance, a
//! system unit.

use super::{Launch, Manager, Restart};
use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(super) const UNIT: &str = "beep-node.service";

/// Where the system unit lives.
const SYSTEM_UNIT: &str = "/etc/systemd/system/beep-node.service";

/// Which systemd instance runs the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    /// The user's own instance (`systemctl --user`).
    User,
    /// PID 1, for a node run as root.
    System,
}

impl Scope {
    fn manager(self) -> Manager {
        match self {
            Self::User => Manager::Systemd,
            Self::System => Manager::SystemdSystem,
        }
    }

    /// `--user` for the user's instance; the system's takes no flag.
    pub(super) fn flags(self) -> &'static [&'static str] {
        match self {
            Self::User => &["--user"],
            Self::System => &[],
        }
    }

    /// The target that starts the unit with the instance.
    fn wanted_by(self) -> &'static str {
        match self {
            Self::User => "default.target",
            Self::System => "multi-user.target",
        }
    }

    pub(super) fn unit_path(self) -> Result<PathBuf> {
        match self {
            Self::User => Ok(dirs::config_dir()
                .context("this system has no config folder")?
                .join("systemd/user")
                .join(UNIT)),
            Self::System => Ok(PathBuf::from(SYSTEM_UNIT)),
        }
    }
}

/// Whether a systemd user instance answers here; the reason when not.
pub(super) fn usable() -> Result<(), String> {
    // What sd_booted(3) checks: absent in containers and on other inits.
    if !Path::new("/run/systemd/system").is_dir() {
        return Err("this machine was not started by systemd".to_owned());
    }
    let out = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .map_err(|e| format!("running systemctl: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let said = said.lines().next().unwrap_or("no output").trim();
        return Err(format!("systemctl --user failed: {said}"));
    }
    Ok(())
}

pub(super) fn install(launch: &Launch, scope: Scope) -> Result<PathBuf> {
    let path = scope.unit_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&path, unit(launch, scope)).with_context(|| format!("writing {}", path.display()))?;
    systemctl(scope, &["daemon-reload"])?;
    systemctl(scope, &["enable", "--now", UNIT])?;
    // Picks up a new binary or unit when it was already running.
    systemctl(scope, &["restart", UNIT])?;
    if scope == Scope::User {
        linger();
    }
    Ok(path)
}

pub(super) fn uninstall(scope: Scope) -> Result<()> {
    let path = scope.unit_path()?;
    if !path.exists() {
        println!("{UNIT} is not installed.");
        return Ok(());
    }
    systemctl(scope, &["disable", "--now", UNIT])?;
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    systemctl(scope, &["daemon-reload"])?;
    println!("Stopped and removed {}.", path.display());
    Ok(())
}

pub(super) fn start(scope: Scope) -> Result<()> {
    systemctl(scope, &["start", UNIT])
}

pub(super) fn restart(scope: Scope) -> Result<()> {
    systemctl(scope, &["restart", UNIT])
}

/// What `restart` would do, without doing it.
pub(super) fn plan(exe: &Path, scope: Scope) -> Result<Restart> {
    let path = scope.unit_path()?;
    let unit = match fs::read_to_string(&path) {
        Ok(unit) => unit,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Restart::NotInstalled),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    if !runs(&unit, exe) {
        return Ok(Restart::OtherBinary(path));
    }
    // A unit from an older node gets this one's settings, keeping its
    // command and PATH.
    if let Some(newer) = refreshed(&unit, scope) {
        fs::write(&path, newer).with_context(|| format!("writing {}", path.display()))?;
        systemctl(scope, &["daemon-reload"])?;
    }
    if !active(scope) {
        return Ok(Restart::Stopped(scope.manager()));
    }
    Ok(Restart::Restarted(scope.manager()))
}

/// The folder the unit's `ExecStart` passes as `--dir`.
pub(super) fn dir_in(unit: &str) -> Option<PathBuf> {
    let exec = unit
        .lines()
        .find_map(|line| line.strip_prefix("ExecStart="))?;
    super::existing::dir_arg(&split(exec))
}

/// A command line as `quote_arg` wrote it: quoted words, where a backslash
/// takes the next character and `%%` and `$$` stand for one.
fn split(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == ' ' {
            chars.next();
            continue;
        }
        let mut word = String::new();
        let quoted = c == '"';
        if quoted {
            chars.next();
        }
        while let Some(c) = chars.next() {
            match c {
                '"' if quoted => break,
                ' ' if !quoted => break,
                '\\' => word.extend(chars.next()),
                '%' | '$' if chars.peek() == Some(&c) => {
                    chars.next();
                    word.push(c);
                }
                _ => word.push(c),
            }
        }
        words.push(word);
    }
    words
}

/// Whether `unit` starts the node from `exe`.
fn runs(unit: &str, exe: &Path) -> bool {
    let start = format!("ExecStart={} ", quote_arg(&exe.display().to_string()));
    unit.lines().any(|line| line.starts_with(&start))
}

/// Whether the unit is running.
fn active(scope: Scope) -> bool {
    Command::new("systemctl")
        .args(scope.flags())
        .args(["is-active", "--quiet", UNIT])
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The unit file.
fn unit(launch: &Launch, scope: Scope) -> String {
    let exec: Vec<String> = std::iter::once(launch.exe.display().to_string())
        .chain(launch.args())
        .map(|arg| quote_arg(&arg))
        .collect();
    let search = quote(&format!("PATH={}", launch.search));
    unit_text(&exec.join(" "), &search, scope)
}

/// `unit` written again with this node's settings, from its `ExecStart`
/// and `PATH`; `None` when it already has them or lacks either line.
fn refreshed(unit: &str, scope: Scope) -> Option<String> {
    let exec = unit
        .lines()
        .find_map(|line| line.strip_prefix("ExecStart="))?;
    let search = unit
        .lines()
        .filter_map(|line| line.strip_prefix("Environment="))
        .find(|value| value.starts_with("\"PATH="))?;
    let newer = unit_text(exec, search, scope);
    (newer != unit).then_some(newer)
}

/// The unit file around its command line and quoted `PATH=` setting.
/// `KillMode=mixed` stops only the node at first, so it closes its agent
/// sessions itself and says they ended with it; what is left is killed
/// once the node exits.
fn unit_text(exec: &str, search: &str, scope: Scope) -> String {
    let wanted = scope.wanted_by();
    format!(
        "[Unit]\n\
         Description=Beep node\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         ExecStart={exec}\n\
         Restart=always\n\
         RestartSec=5\n\
         KillMode=mixed\n\
         TimeoutStopSec=20\n\
         Environment={search}\n\
         \n\
         [Install]\n\
         WantedBy={wanted}\n",
    )
}

/// A value quoted for a unit file: `%` starts a specifier there, so it is doubled.
fn quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// An `ExecStart` argument, where `$` also expands a variable.
fn quote_arg(value: &str) -> String {
    quote(&value.replace('$', "$$"))
}

fn systemctl(scope: Scope, args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .args(scope.flags())
        .args(args)
        .status()
        .context("running systemctl")?;
    if !status.success() {
        let command: Vec<&str> = scope.flags().iter().chain(args).copied().collect();
        bail!("systemctl {} failed", command.join(" "));
    }
    Ok(())
}

/// Lets the user's services run without a login session and start at boot.
/// Only warns when it cannot: the service still runs while the user is in.
fn linger() {
    let user = user();
    if lingering(&user) {
        return;
    }
    let enabled = Command::new("loginctl")
        .args(["--no-ask-password", "enable-linger", &user])
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !enabled {
        println!(
            "Could not turn on lingering, so the service stops when you log out and \
             waits for a login after a reboot. To fix that, run once:\n  \
             sudo loginctl enable-linger {user}"
        );
    }
}

/// The user to linger, by name, else by uid.
fn user() -> String {
    if let Ok(name) = env::var("USER")
        && !name.is_empty()
    {
        return name;
    }
    #[cfg(unix)]
    {
        rustix::process::getuid().as_raw().to_string()
    }
    #[cfg(not(unix))]
    {
        String::new()
    }
}

/// Whether the user's services keep running after they log out.
fn lingering(user: &str) -> bool {
    Command::new("loginctl")
        .args(["show-user", user, "--property=Linger", "--value"])
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "yes")
}

#[cfg(test)]
mod tests {
    use super::super::tests::launch;
    use super::*;

    #[test]
    fn a_restart_only_follows_a_unit_that_runs_this_binary() {
        let unit = unit(&launch(None), Scope::User);
        assert!(runs(&unit, Path::new("/opt/beep node/beep-node")));
        assert!(!runs(&unit, Path::new("/opt/beep node/beep")));
        assert!(!runs(&unit, Path::new("/usr/local/bin/beep-node")));
    }

    #[test]
    fn the_unit_passes_the_workspace_and_escapes_it() {
        let mut launch = launch(Some("/home/user/my \"$work\" 100%"));
        launch.dir = PathBuf::from("/srv/a\\b");
        let unit = unit(&launch, Scope::User);
        assert!(
            unit.contains(
                "ExecStart=\"/opt/beep node/beep-node\" \"--dir\" \"/srv/a\\\\b\" \
                 \"--workspace\" \"/home/user/my \\\"$$work\\\" 100%%\" \"run\"\n"
            ),
            "{unit}"
        );
    }

    #[test]
    fn an_older_unit_is_rewritten_with_its_command_and_path() {
        let current = unit(&launch(Some("/home/user/work")), Scope::User);
        assert_eq!(refreshed(&current, Scope::User), None);
        // What `service install` wrote before KillMode and the node's own
        // log filter.
        let older = current
            .replace("KillMode=mixed\nTimeoutStopSec=20\n", "")
            .replace("\n\n[Install]", "\nEnvironment=RUST_LOG=info\n\n[Install]");
        assert_ne!(older, current);
        assert_eq!(
            refreshed(&older, Scope::User).as_deref(),
            Some(current.as_str())
        );
    }

    #[test]
    fn the_unit_names_its_folder() {
        let mut spaced = launch(None);
        spaced.dir = PathBuf::from("/srv/my $node/50%");
        assert_eq!(
            dir_in(&unit(&spaced, Scope::System)),
            Some(PathBuf::from("/srv/my $node/50%"))
        );
        let sample = "[Service]\nExecStart=\"/home/u/.local/bin/beep-node\" \"--dir\" \
            \"/home/u/.config/beep-node\" \"run\"\nRestart=always\n";
        assert_eq!(
            dir_in(sample),
            Some(PathBuf::from("/home/u/.config/beep-node"))
        );
        assert_eq!(dir_in("[Service]\nRestart=always\n"), None);
    }

    #[test]
    fn a_system_unit_starts_with_the_machine() {
        let user = unit(&launch(None), Scope::User);
        let system = unit(&launch(None), Scope::System);
        assert!(user.ends_with("WantedBy=default.target\n"), "{user}");
        assert_eq!(user.replace("default.target", "multi-user.target"), system);
    }
}
