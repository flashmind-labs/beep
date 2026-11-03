//! Agent accounts in Settings: how the caller's coding agents sign in on
//! their machines. Claude Code signs in once, with a long-lived token Beep
//! keeps sealed and hands to the machines as `CLAUDE_CODE_OAUTH_TOKEN`.
//! Codex signs in on each machine, with its device sign-in run there as a
//! background task (`process/1`), or takes an API key Beep hands out the
//! same way.

use super::{
    Reply,
    agent_sessions::{IN_FLIGHT, NODE_LIMIT},
    credentials::trimmed,
    own_nodes::{linked_keys, online},
};
use crate::{
    credentials::{Slot, last4},
    nodes::Online,
    protocol::{
        node::{
            EnvReport,
            process::{Reply as TaskReply, Request as TaskRequest, TaskId, TaskState},
        },
        *,
    },
    router::Cx,
    store::{AgentTokenRow, NewCredential},
};
use anyhow::{Result, bail};
use futures::{StreamExt, stream};
use reqwest::Url;
use secrecy::ExposeSecret;
use std::{
    str::FromStr,
    time::{Duration, SystemTime},
};
use tokio::time::{sleep, timeout};

/// What the node runs for Codex's device sign-in.
const CODEX_LOGIN: &str = "codex login --device-auth";
/// What tells whether Codex is signed in on a machine.
const CODEX_STATUS: &str = "codex login status";
/// How long `codex login status` may take.
const STATUS_LIMIT: Duration = Duration::from_secs(5);
/// How long Codex may take to print its page and code.
const CODE_LIMIT: Duration = Duration::from_secs(20);
/// How often its output is read while waiting for the code.
const CODE_POLL: Duration = Duration::from_millis(300);
/// How long a code lasts when Codex does not say; it says 15 minutes.
const CODE_LIFETIME: Duration = Duration::from_secs(15 * 60);
/// The longest reason from Codex passed on.
const REASON_CHARS: usize = 200;

pub(super) async fn get_agent_accounts(cx: Cx, _: GetAgentAccounts) -> Reply<AgentAccounts> {
    let tokens = cx.user.agent_tokens(&cx.app.credential_key).await?;
    let token = |agent: AgentAccount| {
        tokens
            .iter()
            .find(|row| row.agent == agent)
            .map(agent_token)
    };
    let keys = linked_keys(&cx).await?;
    // The futures are made up front: a closure inside the stream would keep
    // the reply future from being `Send`.
    let asks: Vec<_> = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .filter(|node| keys.contains(&node.endpoint))
        .filter_map(|node| {
            let env = node.env.unwrap_or_default();
            let tools = Tools::of(&env);
            if !tools.claude && !tools.codex {
                return None;
            }
            let online = online(&cx, &node.endpoint);
            Some(machine(node.endpoint, node.name, env, tools, online))
        })
        .collect();
    let mut machines: Vec<AgentMachine> = stream::iter(asks).buffered(IN_FLIGHT).collect().await;
    machines.sort_by_key(|machine| machine.machine_name.to_lowercase());
    Ok(AgentAccounts {
        claude: token(AgentAccount::Claude),
        codex_key: token(AgentAccount::Codex),
        machines,
    })
}

pub(super) async fn save_agent_token(
    cx: Cx,
    SaveAgentToken { agent, secret }: SaveAgentToken,
) -> Reply<AgentToken> {
    let agent = agent_named(&agent)?;
    let secret = trimmed(&secret)?;
    if let Some(why) = refusal(agent, secret.expose_secret()) {
        return Err(ApiError::bad_request(why));
    }
    let shown = last4(secret.expose_secret());
    let row = cx
        .user
        .save_credential(
            &cx.app.credential_key,
            NewCredential {
                slot: &Slot::AgentToken(agent),
                secret: &secret,
                last4: &shown,
                account: None,
                to_machines: true,
            },
        )
        .await?;
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(AgentToken {
        last4: row.last4,
        created_at: SystemTime::from(row.created_at),
        unreadable: false,
    })
}

pub(super) async fn revoke_agent_token(
    cx: Cx,
    RevokeAgentToken { agent }: RevokeAgentToken,
) -> Reply<()> {
    let agent = agent_named(&agent)?;
    if !cx.user.remove_agent_token(agent).await? {
        return Err(ApiError::NotFound {});
    }
    cx.app.nodes.credentials_changed(cx.user.id());
    Ok(())
}

pub(super) async fn start_codex_login(
    cx: Cx,
    StartCodexLogin { machine_id }: StartCodexLogin,
) -> Reply<CodexLogin> {
    let node = linked_online(&cx, &machine_id).await?;
    let started = node
        .tasks(&TaskRequest::Start {
            command: CODEX_LOGIN.to_owned(),
            cwd: None,
        })
        .await
        .and_then(|reply| match reply {
            TaskReply::Started { id } => Ok(id),
            TaskReply::Error { message } => bail!("{message}"),
            other => bail!("the node answered {other:?}"),
        })
        .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", node.name)))?;
    match timeout(CODE_LIMIT, wait_for_code(&node, started)).await {
        Ok(Ok(prompt)) => Ok(CodexLogin {
            task: started.0,
            url: prompt.url,
            code: prompt.code,
            expires_at: SystemTime::now() + prompt.lifetime,
        }),
        Ok(Err(why)) => {
            stop_quietly(&node, started).await;
            Err(ApiError::bad_request(format!("{}: {why}", node.name)))
        }
        Err(_) => {
            stop_quietly(&node, started).await;
            Err(ApiError::bad_request(format!(
                "{}: Codex did not give a sign-in code in time.",
                node.name
            )))
        }
    }
}

pub(super) async fn get_codex_login(
    cx: Cx,
    GetCodexLogin { machine_id, task }: GetCodexLogin,
) -> Reply<CodexLoginState> {
    if !linked_keys(&cx).await?.contains(&machine_id) {
        return Err(ApiError::NotFound {});
    }
    let Some(node) = online(&cx, &machine_id) else {
        return Ok(CodexLoginState::Gone {});
    };
    let status = node.tasks(&TaskRequest::Status { id: TaskId(task) }).await;
    Ok(match status {
        Ok(TaskReply::Status { task, lines, .. }) if task.command == CODEX_LOGIN => {
            login_state(task.state, &lines)
        }
        Ok(_) => CodexLoginState::Gone {},
        Err(e) => return Err(ApiError::bad_request(format!("{}: {e:#}", node.name))),
    })
}

pub(super) async fn cancel_codex_login(
    cx: Cx,
    CancelCodexLogin { machine_id, task }: CancelCodexLogin,
) -> Reply<()> {
    if !linked_keys(&cx).await?.contains(&machine_id) {
        return Err(ApiError::NotFound {});
    }
    let Some(node) = online(&cx, &machine_id) else {
        return Ok(());
    };
    stop(&node, TaskId(task))
        .await
        .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", node.name)))
}

/// Which agents a machine has, from its last report.
struct Tools {
    claude: bool,
    codex: bool,
}

impl Tools {
    fn of(env: &EnvReport) -> Self {
        let has = |name: &str| env.tools.iter().any(|tool| tool.name == name);
        Self {
            claude: has("claude"),
            codex: has("codex"),
        }
    }
}

/// One machine's row: its Claude Code sessions and Codex sign-in asked of
/// it when it is online. What it does not answer in time is left unknown.
async fn machine(
    id: String,
    name: String,
    env: EnvReport,
    tools: Tools,
    online: Option<Online>,
) -> AgentMachine {
    let (claude_sessions, codex_signed_in) = match &online {
        Some(node) => {
            let sessions = async {
                if !tools.claude {
                    return 0;
                }
                match timeout(NODE_LIMIT, node.agent_sessions()).await {
                    Ok(Ok(sessions)) => {
                        let count = sessions
                            .iter()
                            .filter(|session| session.agent == AgentAccount::Claude.slug())
                            .count();
                        u32::try_from(count).unwrap_or(u32::MAX)
                    }
                    _ => 0,
                }
            };
            let codex = async {
                if !tools.codex {
                    return None;
                }
                codex_signed_in(node)
                    .await
                    .or_else(|| reported_codex_login(&env))
            };
            tokio::join!(sessions, codex)
        }
        None => (0, None),
    };
    AgentMachine {
        machine_id: id,
        machine_name: name,
        online: online.is_some(),
        claude: tools.claude,
        codex: tools.codex,
        claude_sessions,
        codex_signed_in,
    }
}

/// Whether `codex login status` on the node says it is signed in; `None`
/// when it could not be asked.
async fn codex_signed_in(node: &Online) -> Option<bool> {
    let mut output = String::new();
    let exited = node
        .exec(CODEX_STATUS, STATUS_LIMIT, &mut |text| {
            output.push_str(text)
        })
        .await;
    match exited {
        Ok(exited) if !exited.timed_out => {
            Some(exited.exit_code == Some(0) && !output.contains("Not logged in"))
        }
        Ok(_) => None,
        Err(e) => {
            log::debug!("codex login status on {}: {e:#}", node.name);
            None
        }
    }
}

/// The Codex sign-in the machine last reported.
fn reported_codex_login(env: &EnvReport) -> Option<bool> {
    env.logins
        .iter()
        .find(|login| login.tool == AgentAccount::Codex.slug())
        .map(|login| login.signed_in)
}

fn agent_token(row: &AgentTokenRow) -> AgentToken {
    AgentToken {
        last4: row.last4.clone(),
        created_at: SystemTime::from(row.created_at),
        unreadable: row.unreadable,
    }
}

fn agent_named(slug: &str) -> Reply<AgentAccount> {
    AgentAccount::from_str(slug)
        .map_err(|_| ApiError::bad_request(format!("Beep keeps no token for {slug:?}.")))
}

/// Why `secret` is not a token for `agent`, when it plainly is not.
fn refusal(agent: AgentAccount, secret: &str) -> Option<&'static str> {
    match agent {
        AgentAccount::Claude if secret.starts_with("sk-ant-api") => Some(
            "That is an Anthropic API key. Paste the token `claude setup-token` prints, or add \
             the key under Credentials.",
        ),
        AgentAccount::Claude | AgentAccount::Codex => None,
    }
}

/// The caller's machine `endpoint` when it is online now.
/// The caller's linked machine `endpoint`, online now; `NotFound` otherwise.
async fn linked_online(cx: &Cx, endpoint: &str) -> Reply<Online> {
    if !linked_keys(cx).await?.contains(endpoint) {
        return Err(ApiError::NotFound {});
    }
    online(cx, endpoint).ok_or(ApiError::NotFound {})
}

/// Reads the task's output until Codex printed its page and code. Fails
/// with what it said when it ended first.
async fn wait_for_code(node: &Online, id: TaskId) -> Result<Prompt, String> {
    loop {
        let reply = node
            .tasks(&TaskRequest::Status { id })
            .await
            .map_err(|e| format!("{e:#}"))?;
        let TaskReply::Status { task, lines, .. } = reply else {
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

/// Stops the task when it is still the sign-in and still runs.
async fn stop(node: &Online, id: TaskId) -> Result<()> {
    match node.tasks(&TaskRequest::Status { id }).await? {
        TaskReply::Status { task, .. }
            if task.command == CODEX_LOGIN && task.state.is_running() =>
        {
            match node.tasks(&TaskRequest::Stop { id }).await? {
                TaskReply::Error { message } => bail!("{message}"),
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

/// Stops a sign-in that is being given up on; a failure is only logged.
async fn stop_quietly(node: &Online, id: TaskId) {
    if let Err(e) = stop(node, id).await {
        log::warn!("codex sign-in not stopped on {}: {e:#}", node.name);
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

/// What Codex printed of its device sign-in: the page, the code and how
/// long the code lasts.
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

/// `word` when it is an https page of OpenAI's, the only pages passed on
/// for the app to open.
fn device_page(word: &str) -> Option<String> {
    let word = word.trim_end_matches(['.', ',', ')', '"', '\'']);
    let url = Url::parse(word).ok()?;
    let host = url.host_str()?;
    let theirs = ["openai.com", "chatgpt.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
    (url.scheme() == "https" && theirs).then(|| url.to_string())
}

/// A one-time code such as `ABCD-12345`: groups of capitals and digits
/// joined by hyphens, with a digit or two groups at least.
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

    #[test]
    fn an_api_key_is_not_taken_as_a_claude_token() {
        assert!(refusal(AgentAccount::Claude, "sk-ant-api03-abcdef").is_some());
        assert_eq!(refusal(AgentAccount::Claude, "sk-ant-oat01-abcdef"), None);
        assert_eq!(refusal(AgentAccount::Codex, "sk-proj-abcdef"), None);
    }
}
