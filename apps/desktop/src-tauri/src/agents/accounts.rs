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
        Fleet, Machines, agents as direct,
        projects::{FleetState, desk},
        view_signal::{Change, ring},
    },
    settings::cli_login::{find_tool, refused},
};
use anyhow::Context;
use beep_fleet::Session;
use beep_proto::node::events::Topic;
use beep_proto::{
    AccountName, AgentAccount, AgentAccounts, AgentMachine, AgentToken, CodexLogin,
    CodexLoginState, NamedAgentToken, SecretText, watched,
};
use serde::Serialize;
use std::{
    path::Path,
    process::Stdio,
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager, State};
use tokio::{
    io::AsyncReadExt,
    process::{ChildStdout, Command},
    select,
    sync::watch,
    time::timeout,
};

/// How long the browser sign-in may take.
const SIGN_IN_LIMIT: Duration = Duration::from_secs(10 * 60);
/// How long `claude` may take to exit once it printed the token.
const EXIT_WAIT: Duration = Duration::from_secs(3);
/// The most of `claude`'s output kept while looking for the token.
const MAX_OUTPUT: usize = 256 * 1024;

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
    named: Vec<NamedView>,
}

/// A named account of a coding agent, such as Claude Code's `Work`.
#[derive(Debug, Serialize)]
pub struct NamedView {
    agent: AgentAccount,
    name: AccountName,
    token: TokenView,
}

impl From<NamedAgentToken> for NamedView {
    fn from(named: NamedAgentToken) -> Self {
        Self {
            agent: named.agent,
            name: named.name,
            token: named.token.into(),
        }
    }
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
        named,
    } = desk(&fleet).await?.agent_accounts().await?;
    Ok(Some(AccountsView {
        claude: claude.map(Into::into),
        codex_key: codex_key.map(Into::into),
        machines: machines.into_iter().map(Into::into).collect(),
        named: named.into_iter().map(Into::into).collect(),
    }))
}

/// The named accounts alone, for the start form: no machine is asked.
#[tauri::command]
pub async fn agent_account_names(fleet: FleetState<'_>) -> Result<Vec<NamedView>> {
    let named = desk(&fleet).await?.named_agent_tokens();
    Ok(named.into_iter().map(Into::into).collect())
}

/// Saves a pasted token for `agent` (`claude` or `codex`), as its account
/// `name` or as its default account without one.
#[tauri::command]
pub async fn save_agent_token(
    app: AppHandle,
    fleet: FleetState<'_>,
    agent_slug: String,
    name: Option<String>,
    secret: String,
) -> Result<TokenView> {
    let agent = agent(&agent_slug)?;
    save(
        &app,
        &fleet,
        agent,
        name.as_deref(),
        SecretText::new(secret),
    )
    .await
}

/// Saves `secret` and tells the windows the accounts changed: the mesh pushes no accounts
/// view for a token saved or revoked here.
async fn save(
    app: &AppHandle,
    fleet: &Arc<Session>,
    agent: AgentAccount,
    name: Option<&str>,
    secret: SecretText,
) -> Result<TokenView> {
    let saved = desk(fleet)
        .await?
        .save_agent_token(agent.as_ref(), name, &secret)
        .await?;
    accounts_changed(app, agent.as_ref());
    Ok(saved.into())
}

/// The accounts of `agent` changed: the models its machines list are read again, since they
/// depend on the account.
fn accounts_changed(app: &AppHandle, agent: &str) {
    app.state::<Arc<Fleet>>()
        .projections()
        .forget_models_of(agent);
    ring(app, Change::Topic(Topic::new("accounts")));
}

#[tauri::command]
pub async fn revoke_agent_token(
    app: AppHandle,
    fleet: FleetState<'_>,
    agent_slug: String,
    name: Option<String>,
) -> Result<()> {
    let agent = agent(&agent_slug)?;
    desk(&fleet)
        .await?
        .revoke_agent_token(agent.as_ref(), name.as_deref())
        .await?;
    accounts_changed(&app, agent.as_ref());
    Ok(())
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
pub struct ClaudeSignIn(Arc<Mutex<Option<watch::Sender<bool>>>>);

/// Owns the slot until the process task finishes. Only this attempt can free it.
struct SignInAttempt {
    sign_in: ClaudeSignIn,
    cancelled: watch::Receiver<bool>,
}

impl Drop for SignInAttempt {
    fn drop(&mut self) {
        *self.sign_in.lock() = None;
    }
}

impl ClaudeSignIn {
    fn lock(&self) -> MutexGuard<'_, Option<watch::Sender<bool>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn start(&self) -> Result<SignInAttempt> {
        let mut slot = self.lock();

        if slot.is_some() {
            return Err(refused("A Claude Code sign-in is already under way"));
        }

        let (cancel, cancelled) = watch::channel(false);
        *slot = Some(cancel);
        Ok(SignInAttempt {
            sign_in: Self(self.0.clone()),
            cancelled,
        })
    }

    /// Asks the sign-in to stop and gives callers a way to await teardown.
    fn stop(&self) -> Option<watch::Sender<bool>> {
        let cancel = self.lock().as_ref()?.clone();
        let _ = cancel.send(true);
        Some(cancel)
    }

    pub(crate) fn stop_and_wait(&self) -> impl Future<Output = ()> + Send + 'static {
        let stopping = self.stop();
        async move {
            if let Some(stopping) = stopping {
                // The process task owns the only receiver until the child is gone.
                stopping.closed().await;
            }
        }
    }
}

/// Runs `claude setup-token` on this Mac, which opens the browser, and
/// saves the token it prints once the user approves. The token never
/// reaches the page.
#[tauri::command]
pub async fn claude_sign_in(
    app: AppHandle,
    fleet: FleetState<'_>,
    sign_in: State<'_, ClaudeSignIn>,
    name: Option<String>,
) -> Result<TokenView> {
    // A bad name is refused before the browser opens.
    if let Some(name) = &name {
        AccountName::from_str(name)?;
    }

    let claude = find_tool("claude").ok_or_else(|| {
        refused("Claude Code is not installed on this computer. Install it in Coding agents, or paste a token.")
    })?;
    let attempt = sign_in.start()?;
    let token = tauri::async_runtime::spawn(async move {
        // A command cancelled by its window must leave the child with an owner.
        let mut attempt = attempt;
        setup_token(&claude, &mut attempt.cancelled).await
    })
    .await
    .map_err(anyhow::Error::from)??;

    save(&app, &fleet, AgentAccount::Claude, name.as_deref(), token).await
}

#[tauri::command]
pub async fn cancel_claude_sign_in(sign_in: State<'_, ClaudeSignIn>) -> Result<()> {
    sign_in.stop_and_wait().await;
    Ok(())
}

/// Runs `claude setup-token` in a terminal of its own (it draws its prompt
/// only on one), wide enough that the token is printed on one line, until
/// it prints the token, ends, times out or is cancelled.
async fn setup_token(claude: &Path, cancelled: &mut watch::Receiver<bool>) -> Result<SecretText> {
    let cancelled_error = || refused("Sign-in cancelled");

    if watched::read(cancelled, |cancelled| *cancelled) {
        return Err(cancelled_error());
    }

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
    let found = select! {
        biased;
        _ = watched::wait_for(cancelled, |cancelled| *cancelled) => Err(cancelled_error()),
        found = timeout(SIGN_IN_LIMIT, read_token(stdout)) => {
            found.map_err(|_| refused("The sign-in was not approved within 10 minutes"))
        }
    };

    let token = match found {
        Ok(token) => select! {
            biased;
            _ = watched::wait_for(cancelled, |cancelled| *cancelled) => Err(cancelled_error()),
            exited = timeout(EXIT_WAIT, child.wait()) => {
                if matches!(exited, Ok(Ok(_))) {
                    return token;
                }

                token
            }
        },
        Err(error) => Err(error),
    };

    if let Err(e) = child.kill().await {
        log::warn!("stopping claude setup-token: {e}");
    }

    token
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

#[cfg(test)]
#[path = "../tests/agents_accounts.rs"]
mod tests;
