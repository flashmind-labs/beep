//! The node as a macOS LaunchAgent: started at login and restarted when it
//! stops, with its output in the user's Logs folder.

use super::types::{Launch, Manager, Restart, escape, run};
use anyhow::{Context, Result};
use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(super) const LABEL: &str = "com.flashmind.beep-node";

pub(super) fn install(launch: &Launch) -> Result<PathBuf> {
    let path = plist_path()?;
    let log = log_path()?;

    for folder in [path.parent(), log.parent()].into_iter().flatten() {
        fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
    }

    fs::write(&path, plist(launch, &log)).with_context(|| format!("writing {}", path.display()))?;
    let domain = domain()?;
    let service = format!("{domain}/{LABEL}");
    // Unloads an older copy; fails, harmlessly, when none is loaded.
    bootout(&service);
    // A service once disabled stays disabled across bootstraps.
    run("launchctl", &["enable", &service])?;
    run(
        "launchctl",
        &["bootstrap", &domain, &path.display().to_string()],
    )?;
    Ok(path)
}

pub(super) fn uninstall() -> Result<()> {
    let path = plist_path()?;

    if !path.exists() {
        println!("{LABEL} is not installed.");
        return Ok(());
    }

    bootout(&format!("{}/{LABEL}", domain()?));
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    println!("Stopped and removed {}.", path.display());
    Ok(())
}

/// Starts the service from its existing plist, loading it first when it is
/// not loaded.
pub(super) fn start() -> Result<()> {
    let domain = domain()?;
    let service = format!("{domain}/{LABEL}");
    run("launchctl", &["enable", &service])?;
    // Fails, harmlessly, when the plist is already loaded.
    let _ = Command::new("launchctl")
        .args(["bootstrap", &domain, &plist_path()?.display().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    run("launchctl", &["kickstart", &service])
}

/// Reloads the service so it starts the new binary at its path, in a
/// helper of its own: run from the service, the bootout stops this process.
///
/// A LaunchAgent registered at login carries a code requirement pinned to
/// its binary's ad hoc cdhash. `kickstart -k` on a replaced binary is
/// refused with `OS_REASON_CODESIGNING` and the agent stays down; loading it
/// anew records the new binary. Should it still not run, a kickstart
/// starts it.
pub(super) fn restart() -> Result<()> {
    let domain = domain()?;
    let service = format!("{domain}/{LABEL}");
    let plist = plist_path()?.display().to_string();
    let log = log_path()?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .with_context(|| format!("opening {}", log.display()))?;
    let mut helper = Command::new("/bin/sh");
    helper
        .args(["-c", RELOAD, "reload", &domain, &service, &plist])
        .stdin(Stdio::null())
        .stdout(log.try_clone().context("opening the log")?)
        .stderr(log);
    // launchd stops the service's process group, not this one.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut helper, 0);
    let mut child = helper.spawn().context("starting the reload")?;

    // The service's own node is stopped by the reload; it does not wait.
    if env::var_os("XPC_SERVICE_NAME").is_some_and(|name| name == LABEL) {
        return Ok(());
    }

    let status = child.wait().context("waiting for the reload")?;
    anyhow::ensure!(status.success(), "{LABEL} did not start again");
    Ok(())
}

/// `restart`'s helper, given the domain, the service and the plist.
const RELOAD: &str = r#"
domain=$1 service=$2 plist=$3
running() { launchctl print "$service" 2>/dev/null | grep -q "state = running"; }
launchctl bootout "$service" 2>/dev/null
i=0
while launchctl print "$service" >/dev/null 2>&1 && [ $i -lt 30 ]; do sleep 1; i=$((i+1)); done
launchctl enable "$service"
launchctl bootstrap "$domain" "$plist"
i=0
while ! running && [ $i -lt 10 ]; do sleep 1; i=$((i+1)); done
running || launchctl kickstart "$service"
sleep 2
running
"#;

/// What `restart` would do, without doing it.
pub(super) fn plan(exe: &Path) -> Result<Restart> {
    let path = plist_path()?;
    let plist = match fs::read_to_string(&path) {
        Ok(plist) => plist,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Restart::NotInstalled),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    if !runs(&plist, exe) {
        return Ok(Restart::OtherBinary(path));
    }

    let service = format!("{}/{LABEL}", domain()?);
    let loaded = Command::new("launchctl")
        .args(["print", &service])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    // A plist from an older node gets this one's settings, keeping its
    // arguments and PATH; `restart` loads it anew.
    if let Some(newer) = refreshed(&plist) {
        fs::write(&path, newer).with_context(|| format!("writing {}", path.display()))?;
    }

    if !loaded {
        return Ok(Restart::Stopped(Manager::Launchd));
    }

    Ok(Restart::Restarted(Manager::Launchd))
}

/// The folder the plist passes as `--dir`.
pub(super) fn dir_in(plist: &str) -> Option<PathBuf> {
    let flag = "<string>--dir</string>";
    let after = &plist[plist.find(flag)? + flag.len()..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    Some(PathBuf::from(super::types::unescape(&after[start..end])))
}

/// Whether `plist` starts the node from `exe`: its first program argument.
fn runs(plist: &str, exe: &Path) -> bool {
    let program = format!(
        "<array>\n        <string>{}</string>\n",
        escape(&exe.display().to_string())
    );
    plist.contains(&program)
}

pub(super) fn plist_path() -> Result<PathBuf> {
    Ok(home()?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

/// Where the service's output goes.
pub(super) fn log_path() -> Result<PathBuf> {
    Ok(home()?.join("Library/Logs/beep-node.log"))
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("this system has no home folder")
}

/// The user's GUI session, where LaunchAgents run.
fn domain() -> Result<String> {
    #[cfg(unix)]
    {
        Ok(format!("gui/{}", rustix::process::getuid().as_raw()))
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!("launchd only runs on macOS")
    }
}

/// The agent's property list.
fn plist(launch: &Launch, log: &Path) -> String {
    let args: String = std::iter::once(launch.exe.display().to_string())
        .chain(launch.args())
        .map(|arg| format!("        <string>{}</string>\n", escape(&arg)))
        .collect();
    plist_text(&Escaped {
        args: &args,
        search: &escape(&launch.search),
        log: &escape(&log.display().to_string()),
    })
}

/// The parts of a plist that vary, escaped for XML.
struct Escaped<'a> {
    /// The program's `<string>` lines.
    args: &'a str,
    search: &'a str,
    log: &'a str,
}

/// `plist` written again with this node's settings, from its arguments,
/// PATH and log; `None` when it already has them or lacks one.
fn refreshed(plist: &str) -> Option<String> {
    let between = |start: &str, end: &str| {
        let from = plist.find(start)? + start.len();
        let len = plist[from..].find(end)?;
        Some(&plist[from..from + len])
    };
    let newer = plist_text(&Escaped {
        args: between(
            "<key>ProgramArguments</key>\n    <array>\n",
            "    </array>\n",
        )?,
        search: between("<key>PATH</key>\n        <string>", "</string>")?,
        log: between("<key>StandardOutPath</key>\n    <string>", "</string>")?,
    });
    (newer != plist).then_some(newer)
}

fn plist_text(parts: &Escaped) -> String {
    let Escaped { args, search, log } = parts;
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20   <key>Label</key>\n\
         \x20   <string>{LABEL}</string>\n\
         \x20   <key>ProgramArguments</key>\n\
         \x20   <array>\n\
         {args}\
         \x20   </array>\n\
         \x20   <key>EnvironmentVariables</key>\n\
         \x20   <dict>\n\
         \x20       <key>PATH</key>\n\
         \x20       <string>{search}</string>\n\
         \x20   </dict>\n\
         \x20   <key>RunAtLoad</key>\n\
         \x20   <true/>\n\
         \x20   <key>KeepAlive</key>\n\
         \x20   <true/>\n\
         \x20   <key>ThrottleInterval</key>\n\
         \x20   <integer>5</integer>\n\
         \x20   <key>StandardOutPath</key>\n\
         \x20   <string>{log}</string>\n\
         \x20   <key>StandardErrorPath</key>\n\
         \x20   <string>{log}</string>\n\
         </dict>\n\
         </plist>\n",
    )
}

/// Stops and unloads `service`. Its failure is ignored on purpose: it fails
/// when the service is not loaded, which is the state wanted.
fn bootout(service: &str) {
    let _ = Command::new("launchctl")
        .args(["bootout", service])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
#[path = "../../tests/machine_service_launchd.rs"]
mod tests;
