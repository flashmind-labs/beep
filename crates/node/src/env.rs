//! What the machine has, for `Up::Env`: OS, tools on `PATH`, which CLIs
//! are signed in, Android emulators.

mod apps;
mod gpu;

use crate::provision::spawn_env;
use beep_proto::node::{EnvReport, Login, ToolInfo};
use std::{env, path::PathBuf, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

/// How long one probe command may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest version line kept.
const MAX_VERSION: usize = 80;

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
    ("gemini", &["--version"]),
    ("pi", &["--version"]),
    ("flsh", &["--version"]),
    ("kubectl", &["version", "--client"]),
    ("adb", &["version"]),
    ("xcodebuild", &["-version"]),
    ("brew", &["--version"]),
];

pub async fn probe() -> EnvReport {
    let mut tools = Vec::new();
    // Found as the processes the node starts find them, with the tools it
    // installed.
    let path = spawn_env::search_path();
    for (name, args) in KNOWN_TOOLS {
        if which::which_in(name, path.as_ref(), "/").is_ok() {
            let version = output(name, args).await.as_deref().and_then(first_line);
            tools.push(ToolInfo {
                name: (*name).to_owned(),
                version,
            });
        }
    }
    let has = |name: &str| tools.iter().any(|tool| tool.name == name);
    let mut logins = Vec::new();
    if has("claude") {
        logins.push(login("claude", &["auth", "status"], claude_account).await);
    }
    if has("codex") {
        logins.push(login("codex", &["login", "status"], |_| None).await);
    }
    if has("gh") {
        logins.push(login("gh", &["auth", "status"], gh_account).await);
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
    }
}

/// What `program args` printed, stdout then stderr, when it ran in time.
async fn output(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    spawn_env::apply(&mut command);
    let run = command
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = timeout(PROBE_TIMEOUT, run).await.ok()?.ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(text)
}

fn first_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_VERSION).collect())
}

/// Whether `tool` is signed in, from its status command: it succeeds only
/// when signed in, and `account` picks the account from what it printed.
async fn login(tool: &str, args: &[&str], account: fn(&str) -> Option<String>) -> Login {
    let mut command = Command::new(tool);
    // With the credentials, so `gh auth status` sees the token Beep gave.
    spawn_env::apply(&mut command);
    let run = command
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let (signed_in, text) = match timeout(PROBE_TIMEOUT, run).await {
        Ok(Ok(out)) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), text)
        }
        _ => (false, String::new()),
    };
    let signed_in = signed_in && claude_logged_in(&text) != Some(false);
    Login {
        tool: tool.into(),
        signed_in,
        account: signed_in.then(|| account(&text)).flatten(),
    }
}

/// `loggedIn` in `claude auth status`, which prints JSON; `None` for any
/// other output.
fn claude_logged_in(status: &str) -> Option<bool> {
    let status: serde_json::Value = serde_json::from_str(status).ok()?;
    status.get("loggedIn")?.as_bool()
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

async fn os_version() -> String {
    match env::consts::OS {
        "macos" => match output("sw_vers", &["-productVersion"])
            .await
            .as_deref()
            .and_then(first_line)
        {
            Some(version) => format!("macOS {version}"),
            None => "macOS".into(),
        },
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

async fn memory_mb() -> Option<u64> {
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

/// `MemTotal` from `/proc/meminfo`, which gives it in kB.
fn mem_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// The Android emulator, on `PATH` or in the SDK a service may not have on it.
fn emulator() -> Option<PathBuf> {
    which::which("emulator").ok().or_else(|| {
        ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
            .iter()
            .filter_map(env::var_os)
            .map(|sdk| PathBuf::from(sdk).join("emulator/emulator"))
            .find(|path| path.is_file())
    })
}

async fn avds() -> Vec<String> {
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
mod tests {
    use super::*;

    #[test]
    fn reads_claude_auth_status() {
        let status = r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "me@example.com"}"#;
        assert_eq!(claude_logged_in(status), Some(true));
        assert_eq!(claude_account(status).as_deref(), Some("me@example.com"));
        assert_eq!(claude_logged_in(r#"{"loggedIn": false}"#), Some(false));
        assert_eq!(claude_logged_in("Logged in using ChatGPT"), None);
    }

    #[test]
    fn gh_status_names_the_account() {
        let status = "github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Active account: true";
        assert_eq!(gh_account(status).as_deref(), Some("octocat"));
        assert_eq!(
            gh_account("You are not logged into any GitHub hosts."),
            None
        );
    }

    #[test]
    fn os_release_gives_its_pretty_name() {
        let text = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.3 LTS\"\nID=ubuntu";
        assert_eq!(pretty_name(text).as_deref(), Some("Ubuntu 24.04.3 LTS"));
    }

    #[test]
    fn meminfo_total_is_in_megabytes() {
        assert_eq!(
            mem_total_mb("MemTotal:       65775432 kB\nMemFree: 1 kB"),
            Some(64233)
        );
    }

    #[test]
    fn avd_list_skips_log_lines() {
        let text = "INFO    | Storing crashdata in: /tmp/x\nPixel_7\nPixel_9_Pro\n";
        assert_eq!(avd_names(text), ["Pixel_7", "Pixel_9_Pro"]);
    }

    #[test]
    fn a_version_is_its_first_line_cut_short() {
        assert_eq!(
            first_line("\n  git version 2.51.0\nmore").as_deref(),
            Some("git version 2.51.0")
        );
        assert_eq!(
            first_line(&"x".repeat(200)).map(|v| v.len()),
            Some(MAX_VERSION)
        );
    }
}
