//! The node as an `/etc/init.d` service, for root on Linux without systemd:
//! an OpenRC service that restarts it, or a SysV init script that does not.
//! Both write their output to `LOG`.

use super::types::{Launch, Manager, Restart};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const NAME: &str = "beep-node";
pub(super) const SCRIPT: &str = "/etc/init.d/beep-node";
pub(super) const LOG: &str = "/var/log/beep-node.log";

const OPENRC: &str = include_str!("../../../contrib/beep-node.openrc");
const SYSV: &str = include_str!("../../../contrib/beep-node.init");

/// The first line of an OpenRC script, which tells the two apart.
const OPENRC_SHEBANG: &str = "#!/sbin/openrc-run";

/// Which init runs the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Init {
    OpenRc,
    SysV,
}

impl Init {
    fn manager(self) -> Manager {
        match self {
            Self::OpenRc => Manager::OpenRc,
            Self::SysV => Manager::SysVInit,
        }
    }

    fn template(self) -> &'static str {
        match self {
            Self::OpenRc => OPENRC,
            Self::SysV => SYSV,
        }
    }

    /// Runs the script's `verb` (start, stop, restart, status).
    fn command(self, verb: &str) -> Command {
        let mut command = match self {
            Self::OpenRc => {
                let mut command = Command::new("rc-service");
                command.arg(NAME);
                command
            }
            Self::SysV => Command::new(SCRIPT),
        };
        command.arg(verb).stdin(Stdio::null());
        command
    }

    fn run(self, verb: &str) -> Result<()> {
        let status = self
            .command(verb)
            .status()
            .with_context(|| format!("running {SCRIPT} {verb}"))?;
        if !status.success() {
            bail!("{SCRIPT} {verb} failed ({status})");
        }

        Ok(())
    }
}

/// The init the installed script was written for, by its first line.
pub(super) fn installed() -> Result<Option<Init>> {
    match fs::read_to_string(SCRIPT) {
        Ok(script) => Ok(Some(written_for(&script))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {SCRIPT}")),
    }
}

fn written_for(script: &str) -> Init {
    if script.starts_with(OPENRC_SHEBANG) {
        Init::OpenRc
    } else {
        Init::SysV
    }
}

pub(super) fn install(launch: &Launch, init: Init) -> Result<PathBuf> {
    fs::write(SCRIPT, script(launch, init)).with_context(|| format!("writing {SCRIPT}"))?;
    // The init runs the script itself, so it must be executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(SCRIPT, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("making {SCRIPT} executable"))?;
    }
    match init {
        Init::OpenRc => run("rc-update", &["add", NAME, "default"])?,
        // Debian's tool or Red Hat's, whichever this machine has.
        Init::SysV => {
            if !quietly("update-rc.d", &[NAME, "defaults"])
                && !quietly("chkconfig", &["--add", NAME])
            {
                println!(
                    "Neither update-rc.d nor chkconfig worked, so beep-node does not start \
                     with the machine. Link {SCRIPT} into your runlevels by hand."
                );
            }
        }
    }
    // Stops one that ran an older binary or script, then starts it.
    init.run("restart")?;
    Ok(PathBuf::from(SCRIPT))
}

pub(super) fn uninstall(init: Init) -> Result<()> {
    if !Path::new(SCRIPT).exists() {
        println!("{SCRIPT} is not installed.");
        return Ok(());
    }

    init.run("stop")?;

    match init {
        Init::OpenRc => run("rc-update", &["del", NAME, "default"])?,
        Init::SysV => {
            // Only the tool this machine has works.
            quietly("update-rc.d", &["-f", NAME, "remove"]);
            quietly("chkconfig", &["--del", NAME]);
        }
    }

    fs::remove_file(SCRIPT).with_context(|| format!("removing {SCRIPT}"))?;
    println!("Stopped and removed {SCRIPT}.");
    Ok(())
}

pub(super) fn start(init: Init) -> Result<()> {
    init.run("start")
}

/// Restarts the service. Its own process group, so the stop that ends this
/// node, when it is the service's, does not end the restart with it.
pub(super) fn restart(init: Init) -> Result<()> {
    let mut command = init.command("restart");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let status = command
        .status()
        .with_context(|| format!("running {SCRIPT} restart"))?;
    if !status.success() {
        bail!("{SCRIPT} restart failed ({status})");
    }

    Ok(())
}

/// What `restart` would do, without doing it.
pub(super) fn plan(exe: &Path, init: Init) -> Result<Restart> {
    let script = match fs::read_to_string(SCRIPT) {
        Ok(script) => script,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Restart::NotInstalled),
        Err(e) => return Err(e).with_context(|| format!("reading {SCRIPT}")),
    };

    if !runs(&script, exe) {
        return Ok(Restart::OtherBinary(PathBuf::from(SCRIPT)));
    }

    let running = init
        .command("status")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !running {
        return Ok(Restart::Stopped(init.manager()));
    }

    Ok(Restart::Restarted(init.manager()))
}

/// The folder the script passes as `--dir`.
pub(super) fn dir_in(script: &str) -> Option<PathBuf> {
    let args = script
        .lines()
        .find_map(|line| line.strip_prefix("command_args="))?;
    let args = words(args).into_iter().next()?;
    super::types::dir_arg(&words(&args))
}

/// Whether `script` starts the node from `exe`.
fn runs(script: &str, exe: &Path) -> bool {
    let line = format!("command={}", command(exe));
    script.lines().any(|candidate| candidate == line)
}

/// The script for `init`, starting the node as `launch` says.
fn script(launch: &Launch, init: Init) -> String {
    let args: Vec<String> = launch.args().iter().map(|arg| quote(arg)).collect();
    let search = format!("PATH={}", launch.search);
    init.template()
        .replace("@COMMAND@", &command(&launch.exe))
        .replace("@ARGS@", &quote_twice(&args.join(" ")))
        // OpenRC's `supervise-daemon --env`, read through eval.
        .replace("@ENV@", &quote_twice(&format!("--env {}", quote(&search))))
        // SysV's script sets it for itself and the node.
        .replace("@PATH@", &quote(&launch.search))
}

/// The value of `command=`: the binary quoted for the shell, then again for
/// the `eval` that reads it.
fn command(exe: &Path) -> String {
    quote_twice(&quote(&exe.display().to_string()))
}

/// A word quoted for the shell: in single quotes, each one in it as `'\''`.
fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Shell-quoted words quoted once more, in double quotes, so the assignment
/// keeps them as they are for `eval`.
fn quote_twice(words: &str) -> String {
    let escaped = words
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

/// Shell words as `quote` and `quote_twice` write them: single quotes keep
/// everything, double quotes and a bare backslash take the next character.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => words.extend(word.take()),
            '\'' => {
                let word = word.get_or_insert_default();
                word.extend(chars.by_ref().take_while(|&c| c != '\''));
            }
            '"' => {
                let word = word.get_or_insert_default();

                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => word.extend(chars.next()),
                        c => word.push(c),
                    }
                }
            }
            '\\' => word.get_or_insert_default().extend(chars.next()),
            c => word.get_or_insert_default().push(c),
        }
    }

    words.extend(word);
    words
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    super::types::run(program, args)
}

/// Runs `program`, saying only whether it worked.
fn quietly(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
#[path = "../../tests/machine_service_initd.rs"]
mod tests;
