//! The node as a Windows scheduled task: started at logon in the user's
//! session, with no window and without administrator rights. The task runs
//! the node under `conhost --headless`, so neither it nor the console
//! programs it starts open a window, and starts it again every few minutes
//! if it stopped. The node exits with that conhost (`--with-parent`), as
//! ending the task ends only conhost. Its log goes to a file, since there is
//! no console to read.

use super::{Launch, Manager, Restart, escape};
use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The task's name in Task Scheduler.
pub(super) const TASK: &str = "Beep node";

pub(super) fn install(launch: &Launch) -> Result<PathBuf> {
    let path = task_path()?;
    let logs = log_dir()?;
    for folder in [path.parent(), Some(logs.as_path())].into_iter().flatten() {
        fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
    }
    let xml = task_xml(launch, &user()?, &conhost(), &logs.join(LOG_NAME));
    fs::write(&path, utf16(&xml)).with_context(|| format!("writing {}", path.display()))?;
    schtasks(&[
        "/Create",
        "/TN",
        TASK,
        "/XML",
        &path.display().to_string(),
        "/F",
    ])?;
    // Picks up a new binary when one was already running.
    end();
    schtasks(&["/Run", "/TN", TASK])?;
    Ok(path)
}

pub(super) fn uninstall() -> Result<()> {
    if !installed() {
        println!("The scheduled task {TASK:?} is not installed.");
        return Ok(());
    }
    end();
    schtasks(&["/Delete", "/TN", TASK, "/F"])?;
    let path = task_path()?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("removing {}", path.display())),
    }
    println!("Stopped and removed the scheduled task {TASK:?}.");
    Ok(())
}

pub(super) fn restart(exe: &Path) -> Result<Restart> {
    if !installed() {
        return Ok(Restart::NotInstalled);
    }
    let path = task_path()?;
    let xml = match fs::read(&path) {
        Ok(bytes) => from_utf16(&bytes),
        // Installed by hand, or the file was removed: nothing says what it runs.
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Restart::OtherBinary(path)),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    if !runs(&xml, exe) {
        return Ok(Restart::OtherBinary(path));
    }
    // Ending the task ends conhost, and the node with it (`--with-parent`).
    end();
    schtasks(&["/Run", "/TN", TASK])?;
    Ok(Restart::Restarted(Manager::TaskScheduler))
}

/// Whether the task's `xml` starts the node from `exe`.
fn runs(xml: &str, exe: &Path) -> bool {
    let program = format!("<Arguments>--headless {} ", escape(&quote_arg(&plain(exe))));
    xml.contains(&program)
}

/// The node's files that are not its settings: the task as installed and
/// the logs.
fn local_dir() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("this system has no local data folder")?
        .join("beep-node"))
}

/// The task definition, kept to show what was installed.
fn task_path() -> Result<PathBuf> {
    Ok(local_dir()?.join("task.xml"))
}

/// Where the service's log files go.
pub(super) fn log_dir() -> Result<PathBuf> {
    Ok(local_dir()?.join("logs"))
}

/// The log file's name; the node starts a dated one each day.
pub(super) const LOG_NAME: &str = "beep-node.log";

/// `DOMAIN\user`, whom the task runs as and whose logon starts it.
fn user() -> Result<String> {
    let name = env::var("USERNAME").context("USERNAME is not set")?;
    Ok(match env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{name}"),
        _ => name,
    })
}

/// The console host that runs the node without a window.
fn conhost() -> PathBuf {
    let windows =
        env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    windows.join("System32").join("conhost.exe")
}

/// The task's XML: a logon trigger plus one every five minutes, which
/// starts the node again if it stopped (a running one is left alone).
fn task_xml(launch: &Launch, user: &str, conhost: &Path, log: &Path) -> String {
    let node = std::iter::once(plain(&launch.exe))
        .chain([
            "--log-file".to_owned(),
            plain(log),
            "--with-parent".to_owned(),
        ])
        .chain(launch.args_shown(plain))
        .map(|arg| quote_arg(&arg))
        .collect::<Vec<_>>()
        .join(" ");
    let user = escape(user);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n\
         <Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\r\n\
         \x20 <RegistrationInfo>\r\n\
         \x20   <Description>Lets Beep run work on this machine.</Description>\r\n\
         \x20 </RegistrationInfo>\r\n\
         \x20 <Triggers>\r\n\
         \x20   <LogonTrigger>\r\n\
         \x20     <Enabled>true</Enabled>\r\n\
         \x20     <UserId>{user}</UserId>\r\n\
         \x20   </LogonTrigger>\r\n\
         \x20   <TimeTrigger>\r\n\
         \x20     <Repetition>\r\n\
         \x20       <Interval>PT5M</Interval>\r\n\
         \x20     </Repetition>\r\n\
         \x20     <StartBoundary>2020-01-01T00:00:00</StartBoundary>\r\n\
         \x20     <Enabled>true</Enabled>\r\n\
         \x20   </TimeTrigger>\r\n\
         \x20 </Triggers>\r\n\
         \x20 <Principals>\r\n\
         \x20   <Principal id=\"Author\">\r\n\
         \x20     <UserId>{user}</UserId>\r\n\
         \x20     <LogonType>InteractiveToken</LogonType>\r\n\
         \x20     <RunLevel>LeastPrivilege</RunLevel>\r\n\
         \x20   </Principal>\r\n\
         \x20 </Principals>\r\n\
         \x20 <Settings>\r\n\
         \x20   <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\r\n\
         \x20   <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\r\n\
         \x20   <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\r\n\
         \x20   <AllowHardTerminate>true</AllowHardTerminate>\r\n\
         \x20   <StartWhenAvailable>true</StartWhenAvailable>\r\n\
         \x20   <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\r\n\
         \x20   <IdleSettings>\r\n\
         \x20     <StopOnIdleEnd>false</StopOnIdleEnd>\r\n\
         \x20     <RestartOnIdle>false</RestartOnIdle>\r\n\
         \x20   </IdleSettings>\r\n\
         \x20   <AllowStartOnDemand>true</AllowStartOnDemand>\r\n\
         \x20   <Enabled>true</Enabled>\r\n\
         \x20   <Hidden>false</Hidden>\r\n\
         \x20   <RunOnlyIfIdle>false</RunOnlyIfIdle>\r\n\
         \x20   <WakeToRun>false</WakeToRun>\r\n\
         \x20   <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\r\n\
         \x20   <Priority>7</Priority>\r\n\
         \x20   <RestartOnFailure>\r\n\
         \x20     <Interval>PT1M</Interval>\r\n\
         \x20     <Count>999</Count>\r\n\
         \x20   </RestartOnFailure>\r\n\
         \x20 </Settings>\r\n\
         \x20 <Actions Context=\"Author\">\r\n\
         \x20   <Exec>\r\n\
         \x20     <Command>{conhost}</Command>\r\n\
         \x20     <Arguments>--headless {node}</Arguments>\r\n\
         \x20   </Exec>\r\n\
         \x20 </Actions>\r\n\
         </Task>\r\n",
        conhost = escape(&plain(conhost)),
        node = escape(&node),
    )
}

/// `path` without the `\\?\` prefix `canonicalize` adds, which some
/// programs do not take.
fn plain(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_owned(),
        Some(rest) if rest.starts_with(r"UNC\") => format!(r"\\{}", &rest[4..]),
        _ => text,
    }
}

/// One argument quoted for a Windows command line, as the C runtime and
/// Rust read it back: backslashes are literal except before a quote.
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

/// `text` as UTF-16 with a byte order mark, as Task Scheduler reads XML.
fn utf16(text: &str) -> Vec<u8> {
    std::iter::once(0xFEFF)
        .chain(text.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The task file `utf16` wrote, as text.
fn from_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let units = units.strip_prefix(&[0xFEFF]).unwrap_or(&units);
    String::from_utf16_lossy(units)
}

pub(super) fn installed() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", TASK])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Stops the running task. Its failure is ignored on purpose: it fails when
/// the task is not running, which is the state wanted.
fn end() {
    let _ = Command::new("schtasks")
        .args(["/End", "/TN", TASK])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn schtasks(args: &[&str]) -> Result<()> {
    let out = Command::new("schtasks")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running schtasks")?;
    if !out.status.success() {
        bail!(
            "schtasks {} failed: {}",
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

    fn windows_launch(workspace: Option<&str>) -> Launch {
        let mut launch = launch(workspace);
        launch.exe =
            PathBuf::from(r"\\?\C:\Users\user\AppData\Local\Programs\beep-node\beep-node.exe");
        launch.dir = PathBuf::from(r"\\?\C:\Users\user\AppData\Roaming\beep node");
        launch
    }

    fn xml(launch: &Launch) -> String {
        task_xml(
            launch,
            r"HOST\user",
            Path::new(r"C:\Windows\System32\conhost.exe"),
            Path::new(r"C:\Users\user\AppData\Local\beep-node\logs\beep-node.log"),
        )
    }

    #[test]
    fn the_task_passes_the_workspace_and_escapes_it() {
        let xml = xml(&windows_launch(Some(r"D:\work & play\")));
        assert!(
            xml.contains(r#"--workspace &quot;D:\work &amp; play\\&quot; run</Arguments>"#),
            "{xml}"
        );
    }

    #[test]
    fn arguments_quote_as_windows_reads_them() {
        assert_eq!(quote_arg(r"C:\plain\path"), r"C:\plain\path");
        assert_eq!(quote_arg(""), r#""""#);
        assert_eq!(quote_arg(r"C:\with space\"), r#""C:\with space\\""#);
        assert_eq!(quote_arg(r#"say "hi"\"#), r#""say \"hi\"\\""#);
        assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn verbatim_paths_lose_their_prefix() {
        assert_eq!(plain(Path::new(r"\\?\C:\x\y.exe")), r"C:\x\y.exe");
        assert_eq!(
            plain(Path::new(r"\\?\UNC\server\share\y")),
            r"\\server\share\y"
        );
        assert_eq!(plain(Path::new(r"C:\x")), r"C:\x");
    }

    #[test]
    fn a_restart_only_follows_a_task_that_runs_this_binary() {
        let xml = from_utf16(&utf16(&xml(&windows_launch(None))));
        let exe = r"\\?\C:\Users\user\AppData\Local\Programs\beep-node\beep-node.exe";
        assert!(runs(&xml, Path::new(exe)));
        assert!(!runs(&xml, Path::new(r"C:\Users\user\beep-node.exe")));
    }
}
