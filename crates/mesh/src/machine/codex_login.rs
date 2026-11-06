//! Codex's device sign-in, run on a machine as a background task: the page
//! and one-time code it prints, how the sign-in stands, and giving it up.

use super::{Tasks, said};
use crate::{MeshError, PeerClient};
use beep_proto::{
    CodexLogin, CodexLoginState,
    node::process::{self, TaskId, TaskState},
};
use std::time::{Duration, SystemTime};
use tokio::time::{sleep, timeout};
use url::Url;

/// What the node runs for Codex's device sign-in.
const CODEX_LOGIN: &str = "codex login --device-auth";
/// How long Codex may take to print its page and code.
const CODE_LIMIT: Duration = Duration::from_secs(20);
/// How often its output is read while waiting for the code.
const CODE_POLL: Duration = Duration::from_millis(300);
/// How long a code lasts when Codex does not say; it says 15 minutes.
const CODE_LIFETIME: Duration = Duration::from_secs(15 * 60);
/// The longest reason from Codex passed on.
const REASON_CHARS: usize = 200;

/// Starts the sign-in and waits for the page and code.
pub async fn start(client: &PeerClient) -> Result<CodexLogin, MeshError> {
    let tasks = Tasks(client);
    let id = tasks.start(CODEX_LOGIN, None).await?;
    match timeout(CODE_LIMIT, wait_for_code(client, id)).await {
        Ok(Ok(prompt)) => Ok(CodexLogin {
            task: id.0,
            url: prompt.url,
            code: prompt.code,
            expires_at: SystemTime::now() + prompt.lifetime,
        }),
        Ok(Err(why)) => {
            give_up(client, id).await;
            Err(said(why))
        }
        Err(_) => {
            give_up(client, id).await;
            Err(said("Codex did not give a sign-in code in time."))
        }
    }
}

/// How the sign-in `task` stands; gone when the machine no longer has it.
pub async fn state(client: &PeerClient, task: u32) -> Result<CodexLoginState, MeshError> {
    let reply = Tasks(client)
        .ask(&process::Request::Status { id: TaskId(task) })
        .await?;
    Ok(match reply {
        process::Reply::Status { task, lines, .. } if task.command == CODEX_LOGIN => {
            login_state(task.state, &lines)
        }
        _ => CodexLoginState::Gone {},
    })
}

/// Stops the sign-in `task` when it still waits.
pub async fn cancel(client: &PeerClient, task: u32) -> Result<(), MeshError> {
    Tasks(client).stop(TaskId(task), CODEX_LOGIN).await
}

/// Stops a sign-in that is being given up on; a failure is only logged.
async fn give_up(client: &PeerClient, id: TaskId) {
    if let Err(e) = Tasks(client).stop(id, CODEX_LOGIN).await {
        tracing::warn!("codex sign-in not stopped: {e:#}");
    }
}

/// Reads the task's output until Codex prints its page and code, or what it said if it ended first.
async fn wait_for_code(client: &PeerClient, id: TaskId) -> Result<Prompt, String> {
    loop {
        let reply = Tasks(client)
            .ask(&process::Request::Status { id })
            .await
            .map_err(|e| format!("{e:#}"))?;
        let process::Reply::Status { task, lines, .. } = reply else {
            return Err("the sign-in stopped on the machine.".into());
        };
        if let Some(prompt) = Prompt::read(&lines) {
            return Ok(prompt);
        }
        if !task.state.is_running() {
            return Err(failure(&lines));
        }
        sleep(CODE_POLL).await;
    }
}

fn login_state(state: TaskState, lines: &[String]) -> CodexLoginState {
    match state {
        TaskState::Running => CodexLoginState::Waiting {},
        TaskState::Exited { code: Some(0) } => CodexLoginState::SignedIn {},
        TaskState::Stopped => CodexLoginState::Failed {
            message: "The sign-in was stopped.".into(),
        },
        _ => CodexLoginState::Failed {
            message: failure(lines),
        },
    }
}

/// What Codex printed of its device sign-in.
#[derive(Debug, PartialEq)]
struct Prompt {
    url: String,
    code: String,
    lifetime: Duration,
}

impl Prompt {
    /// The page and code in Codex's output, once both are there:
    ///
    /// ```text
    /// 1. Open this link in your browser and sign in to your account
    ///    https://auth.openai.com/codex/device
    /// 2. Enter this one-time code (expires in 15 minutes)
    ///    ABCD-12345
    /// ```
    fn read(lines: &[String]) -> Option<Self> {
        let mut url = None;
        let mut code = None;
        let mut lifetime = CODE_LIFETIME;
        for line in lines.iter().map(|line| plain(line)) {
            for word in line.split_whitespace() {
                if url.is_none() {
                    url = device_page(word);
                }
                if code.is_none() && is_code(word) {
                    code = Some(word.to_owned());
                }
            }
            if let Some(minutes) = expires_in(&line) {
                lifetime = Duration::from_secs(minutes * 60);
            }
        }
        Some(Self {
            url: url?,
            code: code?,
            lifetime,
        })
    }
}

/// `word` when it is an https page of OpenAI's, the only pages passed on.
fn device_page(word: &str) -> Option<String> {
    let word = word.trim_end_matches(['.', ',', ')', '"', '\'']);
    let url = Url::parse(word).ok()?;
    let host = url.host_str()?;
    let theirs = ["openai.com", "chatgpt.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
    (url.scheme() == "https" && theirs).then(|| url.to_string())
}

/// A one-time code such as `ABCD-12345`: hyphen-joined capitals and digits.
fn is_code(word: &str) -> bool {
    let groups: Vec<&str> = word.split('-').collect();
    (4..=20).contains(&word.len())
        && groups.len() >= 2
        && groups.iter().all(|group| {
            !group.is_empty()
                && group
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

/// N in "expires in N minutes".
fn expires_in(line: &str) -> Option<u64> {
    let rest = line.split("expires in ").nth(1)?;
    let mut words = rest.split_whitespace();
    let minutes: u64 = words.next()?.parse().ok()?;
    words
        .next()?
        .starts_with("minute")
        .then_some(minutes)
        .filter(|minutes| (1..=120).contains(minutes))
}

/// `line` without terminal colours and other escape sequences.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if !c.is_control() || c == '\t' {
                out.push(c);
            }
            continue;
        }
        // CSI: ESC [ parameters, then one final letter.
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() || c == '~' {
                    break;
                }
            }
        }
    }
    out
}

/// Why the sign-in ended, from the last line Codex printed.
fn failure(lines: &[String]) -> String {
    let last = lines
        .iter()
        .map(|line| plain(line))
        .rfind(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with("WARNING")
        })
        .unwrap_or_default();
    let last = last.trim();
    if last.is_empty() {
        return "Codex could not sign in.".into();
    }
    let short: String = last.chars().take(REASON_CHARS).collect();
    format!("Codex could not sign in: {short}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What codex-cli 0.166 prints, colours and all.
    const DEVICE_AUTH: &str = "\n\u{1b}[1mWelcome to Codex\u{1b}[0m [v\u{1b}[90m0.166.0\u{1b}[0m]\n\
        \u{1b}[90mOpenAI's command-line coding agent\u{1b}[0m\n\n\
        Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
        1. Open this link in your browser and sign in to your account\n   \
        \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m\n\n\
        2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \
        \u{1b}[94mUZIW-O7IAI\u{1b}[0m\n\n\
        \u{1b}[90mContinue only if you started this login in Codex. If a website or another \
        person gave you this code, cancel.\u{1b}[0m\n";

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn the_page_and_code_are_read_from_codex() {
        assert_eq!(
            Prompt::read(&lines(DEVICE_AUTH)),
            Some(Prompt {
                url: "https://auth.openai.com/codex/device".into(),
                code: "UZIW-O7IAI".into(),
                lifetime: Duration::from_secs(15 * 60),
            })
        );
        // Not before the code is printed.
        let half: Vec<String> = lines(DEVICE_AUTH).into_iter().take(9).collect();
        assert_eq!(Prompt::read(&half), None);
    }

    #[test]
    fn only_openai_pages_are_passed_on() {
        assert!(device_page("https://auth.openai.com/codex/device").is_some());
        assert!(device_page("https://chatgpt.com/device.").is_some());
        assert_eq!(device_page("http://auth.openai.com/codex/device"), None);
        assert_eq!(device_page("https://openai.com.example.net/device"), None);
        assert_eq!(device_page("https://example.com"), None);
    }
}
