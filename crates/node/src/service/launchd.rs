//! The node as a macOS LaunchAgent: started at login and restarted when it
//! stops, with its output in the user's Logs folder.

use super::{Launch, Manager, Restart, escape};
use anyhow::{Context, Result, bail};
use std::{
    fs,
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
    launchctl(&["enable", &service])?;
    launchctl(&["bootstrap", &domain, &path.display().to_string()])?;
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

pub(super) fn restart(exe: &Path) -> Result<Restart> {
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
    if !loaded {
        return Ok(Restart::Stopped(Manager::Launchd));
    }
    launchctl(&["kickstart", "-k", &service])?;
    Ok(Restart::Restarted(Manager::Launchd))
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
        bail!("launchd only runs on macOS")
    }
}

/// The agent's property list.
fn plist(launch: &Launch, log: &Path) -> String {
    let args: String = std::iter::once(launch.exe.display().to_string())
        .chain(launch.args())
        .map(|arg| format!("        <string>{}</string>\n", escape(&arg)))
        .collect();
    let log = escape(&log.display().to_string());
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
         \x20       <string>{path}</string>\n\
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
        path = escape(&launch.search),
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

fn launchctl(args: &[&str]) -> Result<()> {
    let out = Command::new("launchctl")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running launchctl")?;
    if !out.status.success() {
        bail!(
            "launchctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::launch;
    use super::*;

    #[test]
    fn the_plist_runs_the_node_at_login_and_keeps_it_alive() {
        let plist = plist(
            &launch(None),
            Path::new("/Users/user/Library/Logs/beep-node.log"),
        );
        assert!(plist.contains("<string>com.flashmind.beep-node</string>"));
        assert!(plist.contains(
            "    <array>\n\
             \x20       <string>/opt/beep node/beep-node</string>\n\
             \x20       <string>--dir</string>\n\
             \x20       <string>/srv/node</string>\n\
             \x20       <string>run</string>\n\
             \x20   </array>\n"
        ));
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(
            plist.contains("<key>PATH</key>\n        <string>/usr/bin:/home/user/50%/bin</string>")
        );
        assert!(plist.contains(
            "<key>StandardErrorPath</key>\n    <string>/Users/user/Library/Logs/beep-node.log</string>"
        ));
        // The node's own filter keeps the networking crates quiet.
        assert!(!plist.contains("RUST_LOG"), "{plist}");
    }

    #[test]
    fn a_restart_only_follows_a_plist_that_runs_this_binary() {
        let plist = plist(&launch(None), Path::new("/l.log"));
        assert!(runs(&plist, Path::new("/opt/beep node/beep-node")));
        assert!(!runs(&plist, Path::new("/opt/beep node/beep")));
        assert!(!runs(&plist, Path::new("/srv/node")));
    }

    #[test]
    fn the_plist_passes_the_workspace_and_escapes_it() {
        let plist = plist(&launch(Some("/Users/user/R&D <new>")), Path::new("/l.log"));
        assert!(plist.contains(
            "<string>--workspace</string>\n        <string>/Users/user/R&amp;D &lt;new&gt;</string>\n        <string>run</string>"
        ), "{plist}");
    }
}
