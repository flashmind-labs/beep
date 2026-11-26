//! What the machine has, for `Report::Env`: OS, tools on `PATH`, which CLIs
//! are signed in, Android emulators.

mod apps;
mod gpu;
mod latest;
mod probe;
pub(crate) mod locate;

pub(crate) use latest::{Probes, changed, fresh, machine};
pub(crate) use probe::{beside, output};
use probe::{first_line, ran};

use crate::files::syncthing::Syncthing;
use crate::machine::spawn_env::{self, Scope};
use beep_proto::node::harness::parse_version;
use beep_proto::node::{EnvReport, Login, ToolInfo};
use std::{
    env,
    path::{Path, PathBuf},
};
#[cfg(windows)]
use std::time::Duration;
use tokio::sync::OnceCell;
#[cfg(windows)]
use tokio::time::timeout;

/// How long listing and looking into the WSL distributions may take.
#[cfg(windows)]
const WSL_LIMIT: Duration = Duration::from_secs(20);

/// Tools worth knowing about, with the arguments that print their version.
const KNOWN_TOOLS: &[(&str, &[&str])] = &[
    ("git", &["--version"]),
    ("gh", &["--version"]),
    ("teacli", &["--version"]),
    ("docker", &["--version"]),
    ("cargo", &["--version"]),
    ("node", &["--version"]),
    ("python3", &["--version"]),
    ("go", &["version"]),
    ("just", &["--version"]),
    ("claude", &["--version"]),
    ("codex", &["--version"]),
    ("agy", &["--version"]),
    ("pi", &["--version"]),
    ("flsh", &["--version"]),
    ("opencode", &["--version"]),
    ("hermes", &["--version"]),
    ("kubectl", &["version", "--client"]),
    ("adb", &["version"]),
    ("xcodebuild", &["-version"]),
    ("brew", &["--version"]),
];

/// What the machine has, its WSL distributions looked into beside the rest.
pub async fn probe() -> EnvReport {
    let (report, wsl) = tokio::join!(probe_machine(), wsl());
    EnvReport { wsl, ..report }
}

/// The WSL distributions of a Windows machine, within `WSL_LIMIT`.
#[cfg(windows)]
async fn wsl() -> Vec<beep_proto::node::WslDistro> {
    timeout(WSL_LIMIT, crate::machine::wsl::report())
        .await
        .unwrap_or_else(|_| {
            log::warn!("the WSL distributions took over {} s to list", WSL_LIMIT.as_secs());
            Vec::new()
        })
}

#[cfg(not(windows))]
async fn wsl() -> Vec<beep_proto::node::WslDistro> {
    Vec::new()
}

async fn probe_machine() -> EnvReport {
    let (tools, found) = probe_tools(ToolScope::All).await;
    let home = dirs::home_dir();
    let at = |name: &str| {
        found
            .iter()
            .find(|(found, _)| *found == name)
            .map(|(_, program)| program.clone())
    };
    let mut logins = Vec::new();

    if let Some(program) = at("claude") {
        let claude = login(
            "claude",
            &program,
            &["auth", "status"],
            claude_logged_in,
            claude_account,
        )
        .await;
        logins.push(claude);
    }

    if let Some(program) = at("codex") {
        logins.push(login("codex", &program, &["login", "status"], |_| None, |_| None).await);
    }
    if let Some(program) = at("opencode") {
        logins.push(
            login(
                "opencode",
                &program,
                &["auth", "list"],
                opencode_logged_in,
                |_| None,
            )
            .await,
        );
    }
    // It has no status command: listing its models fails when signed out.
    if let Some(program) = at("agy") {
        logins.push(login("agy", &program, &["models"], |_| None, |_| None).await);
    }
    // Signed in when its sign-in file is there; never read.
    if at("pi").is_some() {
        let file = has_file(home.as_deref(), ".pi/agent/auth.json").await;
        logins.push(stored("pi", file));
    }
    if let Some(program) = at("gh") {
        logins.push(login("gh", &program, &["auth", "status"], |_| None, gh_account).await);
    }
    EnvReport {
        os: env::consts::OS.to_owned(),
        os_version: os_version().await,
        arch: env::consts::ARCH.to_owned(),
        hostname: output("hostname", &[])
            .await
            .as_deref()
            .and_then(first_line)
            .unwrap_or_default(),
        cpus: std::thread::available_parallelism()
            .map_or(0, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)),
        memory_mb: memory_mb().await.unwrap_or(0),
        home: dirs::home_dir()
            .map(|home| home.display().to_string())
            .unwrap_or_default(),
        tools,
        logins,
        avds: avds().await,
        apps: apps::apps().await,
        gpus: gpu::gpus().await,
        syncthing: Syncthing::local().folders().await,
        // Filled in by the status stream, which knows the installs.
        adapters: Vec::new(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        // Filled in by `probe`, at the same time.
        wsl: Vec::new(),
    }
}

pub(crate) enum ToolScope {
    All,
    Updatable,
}

/// The tools available to node processes and their executable paths.
pub(crate) async fn probe_tools(scope: ToolScope) -> (Vec<ToolInfo>, Vec<(&'static str, PathBuf)>) {
    let mut tools = Vec::new();
    // Found as the processes the node starts find them, with the tools it
    // installed.
    let path = spawn_env::search_path();
    let home = dirs::home_dir();
    let mut found: Vec<(&str, PathBuf)> = Vec::new();

    for (name, args) in KNOWN_TOOLS {
        if matches!(scope, ToolScope::Updatable) && !matches!(*name, "gh" | "teacli" | "flsh") {
            continue;
        }
        // Also in the install folders the agents are started from, which a
        // service's bare `PATH` leaves out.
        let program = which::which_in(name, path.as_ref(), "/")
            .ok()
            .or_else(|| locate::fallback(name, home.as_deref()?));
        if let Some(program) = program {
            let version = match ran(&program, args, Scope::Probe).await {
                // A stub that only says what is missing, like `xcodebuild`
                // with no Xcode, is not the tool.
                Some((false, _)) => continue,
                Some((true, text)) => first_line(&text).map(|line| tool_version(name, &line)),
                None => None,
            };
            tools.push(ToolInfo {
                name: (*name).to_owned(),
                version,
            });
            found.push((name, program));
        }
    }
    (tools, found)
}

/// The bare version in `line`, what tool `name` printed first: `0.162.1`
/// for `codex-cli 0.162.1`, `2.2.5` for `opencode v2.2.5`, `1.22.0` for
/// `go version go1.22.0 linux/amd64`. A line with no version in it is kept
/// as it is.
fn tool_version(name: &str, line: &str) -> String {
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|word| {
            word.strip_prefix(name)
                .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
                .unwrap_or(word)
        })
        .collect();
    parse_version(&words.join(" ")).unwrap_or_else(|| line.to_owned())
}

/// Whether `tool` is signed in, from its status command: it succeeds only
/// when signed in, unless `logged_in` reads otherwise from what it printed,
/// and `account` picks the account from it.
async fn login(
    tool: &str,
    program: &Path,
    args: &[&str],
    logged_in: fn(&str) -> Option<bool>,
    account: fn(&str) -> Option<String>,
) -> Login {
    // With the GitHub token, so `gh auth status` sees the one Beep gave.
    let (signed_in, text) = ran(program, args, Scope::Agent(None))
        .await
        .unwrap_or_default();
    let signed_in = signed_in && logged_in(&text) != Some(false);
    Login {
        tool: tool.into(),
        signed_in,
        account: signed_in.then(|| account(&text)).flatten(),
    }
}

/// A sign-in known only by whether it is there.
fn stored(tool: &str, signed_in: bool) -> Login {
    Login {
        tool: tool.into(),
        signed_in,
        account: None,
    }
}

/// Whether `path` under `home` is a file with more than an empty JSON
/// object in it, by its size: what it holds is a secret.
async fn has_file(home: Option<&Path>, path: &str) -> bool {
    let Some(home) = home else {
        return false;
    };
    tokio::fs::metadata(home.join(path))
        .await
        .is_ok_and(|meta| meta.is_file() && meta.len() > 2)
}

/// `loggedIn` in `claude auth status`, which prints JSON; `None` for any
/// other output.
pub(crate) fn claude_logged_in(status: &str) -> Option<bool> {
    let status: serde_json::Value = serde_json::from_str(status).ok()?;
    status.get("loggedIn")?.as_bool()
}

/// Whether `opencode auth list` counts a stored credential: it always
/// succeeds, and ends its credentials box with "<n> credentials", so no
/// count reads as signed out.
pub(crate) fn opencode_logged_in(list: &str) -> Option<bool> {
    let count = list.lines().find_map(|line| {
        let mut words = line.split_whitespace().rev();
        words.next()?.starts_with("credential").then_some(())?;
        let count: String = words.next()?.chars().filter(char::is_ascii_digit).collect();
        count.parse::<u32>().ok()
    });
    Some(count.is_some_and(|count| count > 0))
}

/// The email in `claude auth status`.
fn claude_account(status: &str) -> Option<String> {
    let status: serde_json::Value = serde_json::from_str(status).ok()?;
    Some(status.get("email")?.as_str()?.to_owned())
}

/// The account in `gh auth status`, such as "Logged in to github.com account octocat (keyring)".
fn gh_account(status: &str) -> Option<String> {
    let rest = status.split(" account ").nth(1)?;
    let account = rest.split_whitespace().next()?;
    Some(account.to_owned())
}

/// The OS release, such as "Ubuntu 24.04" or "macOS 26.3". Read once: an
/// upgrade restarts the machine.
pub async fn os_version() -> String {
    static READ: OnceCell<String> = OnceCell::const_new();
    let read = READ
        .get_or_try_init(|| async {
            let version = read_os_version().await;
            if version.is_empty() {
                Err(())
            } else {
                Ok(version)
            }
        })
        .await;
    read.cloned().unwrap_or_default()
}

async fn read_os_version() -> String {
    match env::consts::OS {
        "macos" => match output("sw_vers", &["-productVersion"])
            .await
            .as_deref()
            .and_then(first_line)
        {
            Some(version) => format!("macOS {version}"),
            None => "macOS".into(),
        },
        "windows" => "Windows".into(),
        _ => tokio::fs::read_to_string("/etc/os-release")
            .await
            .ok()
            .and_then(|text| pretty_name(&text))
            .unwrap_or_default(),
    }
}

fn pretty_name(os_release: &str) -> Option<String> {
    let line = os_release
        .lines()
        .find(|line| line.starts_with("PRETTY_NAME="))?;
    Some(line["PRETTY_NAME=".len()..].trim_matches('"').to_owned())
}

/// The machine's memory. Read once, as it cannot change while it runs.
pub(crate) async fn memory_mb() -> Option<u64> {
    static READ: OnceCell<u64> = OnceCell::const_new();
    READ.get_or_try_init(|| async { read_memory_mb().await.ok_or(()) })
        .await
        .ok()
        .copied()
}

/// Memory in use and the total, in MB, from the system's memory status.
#[cfg(windows)]
pub(crate) fn windows_memory_mb() -> anyhow::Result<(u64, u64)> {
    use anyhow::Context;
    use std::mem::size_of;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>())?,
        ..MEMORYSTATUSEX::default()
    };
    // SAFETY: `status` is the struct the call fills, with its length set
    // as the call requires.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(std::io::Error::last_os_error()).context("GlobalMemoryStatusEx");
    }

    let mb = 1024 * 1024;
    Ok((
        status.ullTotalPhys.saturating_sub(status.ullAvailPhys) / mb,
        status.ullTotalPhys / mb,
    ))
}

async fn read_memory_mb() -> Option<u64> {
    #[cfg(windows)]
    return windows_memory_mb().ok().map(|(_, total_mb)| total_mb);

    #[cfg(not(windows))]
    match env::consts::OS {
        "macos" => {
            let bytes: u64 = output("sysctl", &["-n", "hw.memsize"])
                .await?
                .trim()
                .parse()
                .ok()?;
            Some(bytes / (1024 * 1024))
        }
        _ => mem_total_mb(&tokio::fs::read_to_string("/proc/meminfo").await.ok()?),
    }
}

#[cfg_attr(windows, allow(dead_code))]
/// `MemTotal` from `/proc/meminfo`, which gives it in kB.
fn mem_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// The Android emulator, on `PATH` or in an SDK.
pub(crate) fn emulator() -> Option<PathBuf> {
    sdk_tool("emulator", "emulator")
}

/// adb, on `PATH` or in an SDK.
pub(crate) fn adb() -> Option<PathBuf> {
    sdk_tool("platform-tools", "adb")
}

/// Android SDK tool `name`, on `PATH` or in folder `dir` of an SDK a service
/// may not have on it: the one `ANDROID_HOME` names, else where Android
/// Studio and Homebrew put it.
fn sdk_tool(dir: &str, name: &str) -> Option<PathBuf> {
    which::which(name).ok().or_else(|| {
        let named = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
            .iter()
            .filter_map(env::var_os)
            .map(PathBuf::from);
        let home = dirs::home_dir();
        let usual = [
            Some(PathBuf::from("/opt/homebrew/share/android-commandlinetools")),
            home.as_ref().map(|home| home.join("Library/Android/sdk")),
            home.as_ref().map(|home| home.join("Android/Sdk")),
            dirs::data_local_dir().map(|local| local.join("Android").join("Sdk")),
        ];
        named
            .chain(usual.into_iter().flatten())
            .map(|sdk| sdk.join(dir).join(format!("{name}{}", env::consts::EXE_SUFFIX)))
            .find(|path| path.is_file())
    })
}

/// The AVDs the emulator can start, by name.
pub(crate) async fn avds() -> Vec<String> {
    let Some(emulator) = emulator() else {
        return Vec::new();
    };
    let Some(text) = output(emulator, &["-list-avds"]).await else {
        return Vec::new();
    };
    avd_names(&text)
}

/// The names `emulator -list-avds` prints, skipping its log lines.
fn avd_names(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains(' ') && !line.starts_with("INFO"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
#[path = "../../tests/machine_env.rs"]
mod tests;
