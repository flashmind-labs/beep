//! What the machine has, for `Up::Env`: OS, tools on `PATH`, which CLIs
//! are signed in, Android emulators.

mod apps;
mod gpu;

use crate::files::syncthing::Syncthing;
use crate::machine::provision::spawn_env::{self, Scope};
use beep_proto::node::harness::parse_version;
use beep_proto::node::{EnvReport, Login, ToolInfo};
use std::{env, path::PathBuf, process::Stdio, time::Duration};
use tokio::time::timeout;

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
    ("opencode", &["--version"]),
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
            let version = output(name, args)
                .await
                .as_deref()
                .and_then(first_line)
                .map(|line| tool_version(name, &line));
            tools.push(ToolInfo {
                name: (*name).to_owned(),
                version,
            });
        }
    }
    let has = |name: &str| tools.iter().any(|tool| tool.name == name);
    let mut logins = Vec::new();
    if has("claude") {
        logins.push(
            login(
                "claude",
                &["auth", "status"],
                claude_logged_in,
                claude_account,
            )
            .await,
        );
    }
    if has("codex") {
        logins.push(login("codex", &["login", "status"], |_| None, |_| None).await);
    }
    if has("opencode") {
        logins.push(login("opencode", &["auth", "list"], opencode_logged_in, |_| None).await);
    }
    if has("gh") {
        logins.push(login("gh", &["auth", "status"], |_| None, gh_account).await);
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
    }
}

/// What `program args` printed, stdout then stderr, when it ran in time.
pub(crate) async fn output(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Option<String> {
    let mut command = crate::run::process::command(program);
    spawn_env::apply(&mut command, Scope::Probe);
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
    args: &[&str],
    logged_in: fn(&str) -> Option<bool>,
    account: fn(&str) -> Option<String>,
) -> Login {
    let mut command = crate::run::process::command(tool);
    // With the GitHub token, so `gh auth status` sees the one Beep gave.
    spawn_env::apply(&mut command, Scope::Agent(None));
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
    let signed_in = signed_in && logged_in(&text) != Some(false);
    Login {
        tool: tool.into(),
        signed_in,
        account: signed_in.then(|| account(&text)).flatten(),
    }
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

pub(crate) async fn memory_mb() -> Option<u64> {
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
    fn reads_opencode_auth_list() {
        let one = "\u{1b}[0m\n\u{250c}  Credentials \u{1b}[90m~/.local/share/opencode/auth.json\n\
            \u{2502}\n\u{25cf}  Anthropic \u{1b}[90moauth\n\u{2502}\n\u{2514}  1 credentials\n\n\
            \u{250c}  Environment\n\u{2502}\n\u{2514}  2 environment variables\n";
        assert_eq!(opencode_logged_in(one), Some(true));
        let none = "\u{250c}  Credentials\n\u{2502}\n\u{2514}  0 credentials\n";
        assert_eq!(opencode_logged_in(none), Some(false));
        assert_eq!(
            opencode_logged_in("Error: something went wrong"),
            Some(false)
        );
    }

    #[test]
    fn avd_list_skips_log_lines() {
        let text = "INFO    | Storing crashdata in: /tmp/x\nPixel_7\nPixel_9_Pro\n";
        assert_eq!(avd_names(text), ["Pixel_7", "Pixel_9_Pro"]);
    }

    #[test]
    fn tool_versions_are_bare() {
        let cases = [
            ("claude", "2.1.312 (Claude Code)", "2.1.312"),
            ("codex", "codex-cli 0.162.1", "0.162.1"),
            ("opencode", "opencode v2.2.5", "2.2.5"),
            ("opencode", "1.3.13", "1.3.13"),
            ("go", "go version go1.22.0 linux/amd64", "1.22.0"),
            ("docker", "Docker version 27.3.1, build ce12230", "27.3.1"),
            ("kubectl", "Client Version: v1.31.2", "1.31.2"),
            ("node", "v22.11.0", "22.11.0"),
            ("xcodebuild", "Xcode 16.1", "16.1"),
        ];
        for (name, line, want) in cases {
            assert_eq!(tool_version(name, line), want, "{name}: {line}");
        }
        assert_eq!(tool_version("tool", "no version here"), "no version here");
    }
}
