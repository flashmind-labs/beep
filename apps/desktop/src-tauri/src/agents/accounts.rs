//! Settings, Agent accounts: how the coding agents sign in on the user's
//! machines. Claude Code signs in once, here: Beep runs `claude
//! setup-token`, which opens the browser and prints a long-lived token, and
//! the token goes into the mesh, which hands it to the machines.
//! Codex signs in on each machine with its device sign-in, which the
//! machine runs, or uses an API key the mesh hands out.

use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::{
        Machines, agents as direct,
        projects::{FleetState, desk},
    },
    settings::cli_login::{find_tool, refused},
};
use anyhow::Context;
use beep_fleet::Session;
use beep_proto::{
    AgentAccount, AgentAccounts, AgentMachine, AgentToken, CodexLogin, CodexLoginState, SecretText,
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

fn agent(slug: &str) -> Result<AgentAccount> {
    AgentAccount::from_str(slug).map_err(|_| refused("Beep keeps no token for that agent"))
}

/// The agents' tokens and machines.
#[tauri::command]
pub async fn agent_accounts(fleet: FleetState<'_>) -> Result<Option<AccountsView>> {
    let AgentAccounts {
        claude,
        codex_key,
        machines,
    } = desk(&fleet).await?.agent_accounts().await?;
    Ok(Some(AccountsView {
        claude: claude.map(Into::into),
        codex_key: codex_key.map(Into::into),
        machines: machines.into_iter().map(Into::into).collect(),
    }))
}

/// Saves a pasted token for `agent` (`claude` or `codex`).
#[tauri::command]
pub async fn save_agent_token(
    fleet: FleetState<'_>,
    agent_slug: String,
    secret: String,
) -> Result<TokenView> {
    let agent = agent(&agent_slug)?;
    save(&fleet, agent, SecretText::new(secret)).await
}

async fn save(fleet: &Arc<Session>, agent: AgentAccount, secret: SecretText) -> Result<TokenView> {
    let saved = desk(fleet)
        .await?
        .save_agent_token(agent.as_ref(), &secret)
        .await?;
    Ok(saved.into())
}

#[tauri::command]
pub async fn revoke_agent_token(fleet: FleetState<'_>, agent_slug: String) -> Result<()> {
    let agent = agent(&agent_slug)?;
    Ok(desk(&fleet)
        .await?
        .revoke_agent_token(agent.as_ref())
        .await?)
}

#[tauri::command]
pub async fn codex_login_start(fleet: Machines<'_>, machine_id: String) -> Result<CodexLoginView> {
    let CodexLogin {
        task,
        url,
        code,
        expires_at,
    } = direct::codex_login_start(&fleet, fleet.machine(&machine_id)?).await?;
    Ok(CodexLoginView {
        task,
        url,
        code,
        expires_at: epoch_ms(expires_at),
    })
}

#[tauri::command]
pub async fn codex_login_state(
    fleet: Machines<'_>,
    machine_id: String,
    task: u32,
) -> Result<CodexStateView> {
    let state = direct::codex_login_state(&fleet, fleet.machine(&machine_id)?, task).await?;
    Ok(state.into())
}

#[tauri::command]
pub async fn codex_login_cancel(fleet: Machines<'_>, machine_id: String, task: u32) -> Result<()> {
    direct::codex_login_cancel(&fleet, fleet.machine(&machine_id)?, task).await
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
    fleet: FleetState<'_>,
    sign_in: State<'_, ClaudeSignIn>,
) -> Result<TokenView> {
    let claude = find_tool("claude").ok_or_else(|| {
        refused("Claude Code is not installed on this Mac. Paste a token instead.")
    })?;
    let cancelled = sign_in.start()?;
    let token = setup_token(&claude, cancelled).await;
    sign_in.stop();
    save(&fleet, AgentAccount::Claude, token?).await
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
mod parse;
