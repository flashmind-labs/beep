//! Settings, Agent accounts: how the coding agents sign in on the user's
//! machines. Claude Code signs in once, here: Beep runs `claude
//! setup-token`, which opens the browser and prints a long-lived token, and
//! the token goes straight to the server, which hands it to the machines.
//! Codex signs in on each machine with its device sign-in, which the
//! server runs there, or uses an API key the server hands out.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
    projects::unknown_op,
    settings::cli_login::{find_tool, refused},
    settings::credentials::plain,
};
use anyhow::Context;
use beep_proto::{
    AgentAccount, AgentAccounts, AgentMachine, AgentToken, CancelCodexLogin, CodexLogin,
    CodexLoginState, GetAgentAccounts, GetCodexLogin, RevokeAgentToken, SaveAgentToken, SecretText,
    StartCodexLogin,
};
use serde::Serialize;
use std::{
    path::Path,
    process::Stdio,
    str::FromStr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tauri::State;
use tokio::{
    io::AsyncReadExt,
    process::{Child, ChildStdout, Command},
    select,
    sync::oneshot,
    time::timeout,
};

type Service<'a> = State<'a, Arc<Beep>>;

/// How long the browser sign-in may take.
const SIGN_IN_LIMIT: Duration = Duration::from_secs(10 * 60);
/// How long `claude` may take to exit once it printed the token.
const EXIT_WAIT: Duration = Duration::from_secs(3);
/// The most of `claude`'s output kept while looking for the token.
const MAX_OUTPUT: usize = 256 * 1024;
/// What every Claude Code OAuth token starts with.
const TOKEN_PREFIX: &str = "sk-ant-oat";

/// A coding agent's token as the bar shows it: never the secret.
#[derive(Debug, Serialize)]
pub struct TokenView {
    last4: String,
    /// Milliseconds since the Unix epoch.
    created_at: u64,
    unreadable: bool,
}

impl From<AgentToken> for TokenView {
    fn from(token: AgentToken) -> Self {
        Self {
            last4: token.last4,
            created_at: epoch_ms(token.created_at),
            unreadable: token.unreadable,
        }
    }
}

/// One machine with Claude Code or Codex.
#[derive(Debug, Serialize)]
pub struct MachineView {
    id: String,
    name: String,
    online: bool,
    claude: bool,
    codex: bool,
    claude_sessions: u32,
    codex_signed_in: Option<bool>,
}

impl From<AgentMachine> for MachineView {
    fn from(machine: AgentMachine) -> Self {
        Self {
            id: machine.machine_id,
            name: machine.machine_name,
            online: machine.online,
            claude: machine.claude,
            codex: machine.codex,
            claude_sessions: machine.claude_sessions,
            codex_signed_in: machine.codex_signed_in,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AccountsView {
    claude: Option<TokenView>,
    codex_key: Option<TokenView>,
    machines: Vec<MachineView>,
}

/// A Codex sign-in under way on a machine.
#[derive(Debug, Serialize)]
pub struct CodexLoginView {
    task: u32,
    url: String,
    code: String,
    /// Milliseconds since the Unix epoch.
    expires_at: u64,
}

/// How a Codex sign-in stands: `waiting`, `signed_in`, `failed` (with
/// `message`) or `gone`.
#[derive(Debug, Serialize)]
pub struct CodexStateView {
    state: &'static str,
    message: Option<String>,
}

impl From<CodexLoginState> for CodexStateView {
    fn from(state: CodexLoginState) -> Self {
        let (state, message) = match state {
            CodexLoginState::Waiting {} => ("waiting", None),
            CodexLoginState::SignedIn {} => ("signed_in", None),
            CodexLoginState::Failed { message } => ("failed", Some(message)),
            CodexLoginState::Gone {} => ("gone", None),
        };
        Self { state, message }
    }
}

fn outdated(e: DesktopError) -> DesktopError {
    if unknown_op(&e) {
        DesktopError::BadRequest(
            "The server needs an update before it keeps agent accounts.".into(),
        )
    } else {
        plain(e)
    }
}

fn agent(slug: &str) -> Result<AgentAccount> {
    AgentAccount::from_str(slug).map_err(|_| refused("Beep keeps no token for that agent"))
}

/// The agents' tokens and machines; `None` from a server too old to keep
/// them.
#[tauri::command]
pub async fn agent_accounts(beep: Service<'_>) -> Result<Option<AccountsView>> {
    let AgentAccounts {
        claude,
        codex_key,
        machines,
    } = match beep.call(GetAgentAccounts {}).await {
        Ok(accounts) => accounts,
        Err(e) if unknown_op(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some(AccountsView {
        claude: claude.map(Into::into),
        codex_key: codex_key.map(Into::into),
        machines: machines.into_iter().map(Into::into).collect(),
    }))
}

/// Saves a pasted token for `agent` (`claude` or `codex`).
#[tauri::command]
pub async fn save_agent_token(
    beep: Service<'_>,
    agent_slug: String,
    secret: String,
) -> Result<TokenView> {
    let agent = agent(&agent_slug)?;
    save(&beep, agent, SecretText::new(secret)).await
}

async fn save(beep: &Beep, agent: AgentAccount, secret: SecretText) -> Result<TokenView> {
    let saved = beep
        .call(SaveAgentToken {
            agent: agent.slug().to_owned(),
            secret,
        })
        .await
        .map_err(outdated)?;
    Ok(saved.into())
}

#[tauri::command]
pub async fn revoke_agent_token(beep: Service<'_>, agent_slug: String) -> Result<()> {
    let agent = agent(&agent_slug)?;
    beep.call(RevokeAgentToken {
        agent: agent.slug().to_owned(),
    })
    .await
    .map_err(outdated)
}

#[tauri::command]
pub async fn codex_login_start(beep: Service<'_>, machine_id: String) -> Result<CodexLoginView> {
    let CodexLogin {
        task,
        url,
        code,
        expires_at,
    } = beep
        .call(StartCodexLogin { machine_id })
        .await
        .map_err(outdated)?;
    Ok(CodexLoginView {
        task,
        url,
        code,
        expires_at: epoch_ms(expires_at),
    })
}

#[tauri::command]
pub async fn codex_login_state(
    beep: Service<'_>,
    machine_id: String,
    task: u32,
) -> Result<CodexStateView> {
    let state = beep
        .call(GetCodexLogin { machine_id, task })
        .await
        .map_err(outdated)?;
    Ok(state.into())
}

#[tauri::command]
pub async fn codex_login_cancel(beep: Service<'_>, machine_id: String, task: u32) -> Result<()> {
    beep.call(CancelCodexLogin { machine_id, task })
        .await
        .map_err(outdated)
}

/// The Claude Code sign-in under way, if any: one at a time.
#[derive(Default)]
pub struct ClaudeSignIn(Mutex<Option<oneshot::Sender<()>>>);

impl ClaudeSignIn {
    fn start(&self) -> Result<oneshot::Receiver<()>> {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(|cancel| !cancel.is_closed()) {
            return Err(refused("A Claude Code sign-in is already under way"));
        }
        let (cancel, cancelled) = oneshot::channel();
        *slot = Some(cancel);
        Ok(cancelled)
    }

    /// Stops the sign-in under way, which kills `claude`.
    pub fn stop(&self) {
        let cancel = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(cancel) = cancel
            && cancel.send(()).is_err()
        {
            log::debug!("the Claude Code sign-in had already ended");
        }
    }
}

/// Runs `claude setup-token` on this Mac, which opens the browser, and
/// saves the token it prints once the user approves. The token never
/// reaches the page.
#[tauri::command]
pub async fn claude_sign_in(
    beep: Service<'_>,
    sign_in: State<'_, ClaudeSignIn>,
) -> Result<TokenView> {
    let claude = find_tool("claude").ok_or_else(|| {
        refused("Claude Code is not installed on this Mac. Paste a token instead.")
    })?;
    let cancelled = sign_in.start()?;
    let token = setup_token(&claude, cancelled).await;
    sign_in.stop();
    save(&beep, AgentAccount::Claude, token?).await
}

#[tauri::command]
pub fn cancel_claude_sign_in(sign_in: State<'_, ClaudeSignIn>) {
    sign_in.stop();
}

/// Runs `claude setup-token` in a terminal of its own (it draws its prompt
/// only on one), wide enough that the token is printed on one line, until
/// it prints the token, ends, times out or is cancelled.
async fn setup_token(claude: &Path, cancelled: oneshot::Receiver<()>) -> Result<SecretText> {
    let mut child = Command::new("/usr/bin/script")
        .args(["-q", "/dev/null", "/bin/sh", "-c"])
        .arg("stty cols 1000 rows 50 2>/dev/null; exec \"$0\" setup-token")
        .arg(claude)
        // Held open: `script` passes its end on to `claude`.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("starting claude setup-token")?;
    let stdout = child
        .stdout
        .take()
        .context("claude's output was not piped")?;
    let stopped = select! {
        found = timeout(SIGN_IN_LIMIT, read_token(stdout)) => match found {
            Ok(Ok(token)) => {
                finish(&mut child).await;
                return Ok(token);
            }
            Ok(Err(e)) => {
                finish(&mut child).await;
                return Err(e);
            }
            Err(_) => "The sign-in was not approved within 10 minutes",
        },
        _ = cancelled => "Sign-in cancelled",
    };
    if let Err(e) = child.kill().await {
        log::warn!("stopping claude setup-token: {e}");
    }
    Err(refused(stopped))
}

/// Lets `claude` exit on its own for a moment, then kills it.
async fn finish(child: &mut Child) {
    if timeout(EXIT_WAIT, child.wait()).await.is_err()
        && let Err(e) = child.kill().await
    {
        log::warn!("stopping claude setup-token: {e}");
    }
}

/// Reads `claude`'s output until the token is in it.
async fn read_token(mut stdout: ChildStdout) -> Result<SecretText> {
    let mut seen: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stdout
            .read(&mut chunk)
            .await
            .context("reading claude's output")?;
        if read == 0 {
            let output = String::from_utf8_lossy(&seen);
            return Err(refused(&parse::ended_without_token(&output)));
        }
        seen.extend_from_slice(&chunk[..read]);
        if seen.len() > MAX_OUTPUT {
            let cut = seen.len() - MAX_OUTPUT;
            seen.drain(..cut);
        }
        if let Some(token) = parse::token(&String::from_utf8_lossy(&seen)) {
            return Ok(SecretText::new(token));
        }
    }
}

/// Reading the token out of what `claude setup-token` drew on its
/// terminal. Plain functions, so they test without a terminal.
mod parse {
    use super::TOKEN_PREFIX;

    /// The longest line of `claude`'s passed on as the reason it failed.
    const REASON_CHARS: usize = 200;

    /// Why `claude setup-token` ended without a token: the last line it
    /// drew.
    pub(super) fn ended_without_token(output: &str) -> String {
        let text = plain(output);
        let last = text
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or_default();
        if last.is_empty() {
            return "Claude Code ended without a token. Paste one instead.".into();
        }
        let short: String = last.chars().take(REASON_CHARS).collect();
        format!("Claude Code ended without a token: {short}")
    }

    /// `text` without terminal escapes. A cursor move forward stands for
    /// the spaces it skips; other moves end the line.
    pub(super) fn plain(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' => match chars.next() {
                    Some('[') => {
                        let mut last = ' ';
                        for c in chars.by_ref() {
                            if c.is_ascii_alphabetic() || c == '~' {
                                last = c;
                                break;
                            }
                        }
                        match last {
                            'C' => out.push(' '),
                            'A' | 'B' | 'E' | 'F' | 'H' | 'f' => out.push('\n'),
                            _ => {}
                        }
                    }
                    // OSC, such as a hyperlink: up to BEL or ESC \.
                    Some(']') => {
                        while let Some(c) = chars.next() {
                            if c == '\u{7}' {
                                break;
                            }
                            if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    _ => {}
                },
                '\r' => {}
                '\n' | '\t' => out.push(c),
                c if c.is_control() => {}
                c => out.push(c),
            }
        }
        out
    }

    fn token_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '-' || c == '_'
    }

    /// The OAuth token in `output`, once all of it is there: the line
    /// after it must have come, as a token wrapped over several lines
    /// goes on in the next.
    pub(super) fn token(output: &str) -> Option<String> {
        let text = plain(output);
        let at = text.find(TOKEN_PREFIX)?;
        let mut lines = text[at..].lines();
        let mut token: String = lines
            .next()?
            .chars()
            .take_while(|c| token_char(*c))
            .collect();
        let mut ended = false;
        for line in lines {
            let line = line.trim();
            if !line.is_empty() && line.chars().all(token_char) {
                token.push_str(line);
                continue;
            }
            ended = true;
            break;
        }
        (ended && token.len() > TOKEN_PREFIX.len() + 20).then_some(token)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const TOKEN: &str = "sk-ant-oat01-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-AbCdEfGhIjKl";

        #[test]
        fn the_token_is_read_from_what_claude_drew() {
            let drawn = format!(
                "\u{1b}[32m\u{2713}\u{1b}[39m\u{1b}[1CLong-lived\u{1b}[1Cauthentication\u{1b}[1Ctoken\
                 \u{1b}[1Ccreated\u{1b}[1Csuccessfully!\r\n\r\nYour OAuth token (valid for 1 year):\r\n\r\n\
                 \u{1b}[33m{TOKEN}\u{1b}[39m\r\n\r\nStore this token securely.\r\n"
            );
            assert_eq!(token(&drawn).as_deref(), Some(TOKEN));
        }

        #[test]
        fn a_wrapped_token_is_put_together() {
            let (head, tail) = TOKEN.split_at(30);
            let drawn = format!("Your OAuth token:\r\n{head}\r\n{tail}\r\n\r\nStore it.\r\n");
            assert_eq!(token(&drawn).as_deref(), Some(TOKEN));
        }

        #[test]
        fn not_before_the_whole_token_came() {
            // The rest of the line may still be on its way.
            assert_eq!(token(&format!("Your OAuth token:\r\n{TOKEN}")), None);
            assert_eq!(token("Paste code here if prompted >"), None);
        }

        #[test]
        fn an_end_without_a_token_says_the_last_line() {
            assert_eq!(
                ended_without_token("Welcome\r\n\u{1b}[31mOAuth error: denied\u{1b}[39m\r\n\r\n"),
                "Claude Code ended without a token: OAuth error: denied"
            );
            assert_eq!(
                ended_without_token(""),
                "Claude Code ended without a token. Paste one instead."
            );
        }

        #[test]
        fn hyperlinks_and_moves_are_dropped() {
            let drawn =
                "\u{1b}]8;id=1;https://claude.com/cai/oauth\u{7}link\u{1b}]8;;\u{7}\u{1b}[2Ahi";
            assert_eq!(plain(drawn), "link\nhi");
        }
    }
}
