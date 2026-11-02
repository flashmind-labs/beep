//! What the processes the node starts get on top of the node's own
//! environment: the folders with the tools the node installed first on
//! `PATH`, and, depending on the process, the env vars of the user's
//! credentials (see [`Scope`]).
//!
//! It is process-wide, like the environment it extends: agents, exec,
//! background tasks and the machine probe all start processes, and each
//! reads it when it does.

use beep_proto::{AgentAccount, GITHUB_ENV_VAR, node::EnvSecret};
use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
    sync::{PoisonError, RwLock},
};
use tokio::process::Command;

/// Which credentials a process gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// None: a version probe needs only the tools' folders.
    Probe,
    /// The GitHub token, and the token Beep keeps for the agent itself
    /// (Claude Code's `CLAUDE_CODE_OAUTH_TOKEN`, Codex's `CODEX_API_KEY`)
    /// when the process is that agent. teacli finds its logins through the
    /// shim on `PATH`. API keys stay out of agent sessions such as Claude
    /// Code and Codex, which would otherwise bill or act on them.
    Agent(Option<AgentAccount>),
    /// Every credential the user gave to their machines: exec, background
    /// tasks and terminals, where the user runs their own commands.
    Shell,
}

impl Scope {
    fn admits(self, var: &EnvSecret) -> bool {
        match self {
            Self::Probe => false,
            Self::Agent(agent) => {
                var.name == GITHUB_ENV_VAR || agent.is_some_and(|agent| var.name == agent.env_var())
            }
            Self::Shell => true,
        }
    }
}

struct SpawnEnv {
    /// Folders put before the inherited `PATH`, first first.
    path: Vec<PathBuf>,
    /// Set on every process, over any inherited value of the same name.
    vars: Vec<EnvSecret>,
}

static SPAWN_ENV: RwLock<SpawnEnv> = RwLock::new(SpawnEnv {
    path: Vec::new(),
    vars: Vec::new(),
});

/// Held by tests that set the process-wide env, so they do not see each
/// other's.
#[cfg(test)]
pub(super) static TEST_LOCK: crate::AsyncMutex<()> = crate::AsyncMutex::const_new(());

/// Puts `dirs` before the inherited `PATH` of every process started from
/// now on.
pub(super) fn set_path(dirs: Vec<PathBuf>) {
    SPAWN_ENV
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .path = dirs;
}

/// Sets `vars` on every process started from now on, in place of the
/// credentials set before.
pub(super) fn set_vars(vars: Vec<EnvSecret>) {
    SPAWN_ENV
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .vars = vars;
}

/// Gives `command` the tools' folders and the credentials `scope` admits.
pub(crate) fn apply(command: &mut Command, scope: Scope) {
    for (name, value) in vars(scope) {
        command.env(name, value);
    }
}

/// What `apply` sets, as names and values, for a process started some
/// other way, such as in a pseudo-terminal.
pub(crate) fn vars(scope: Scope) -> Vec<(OsString, OsString)> {
    let spawn = SPAWN_ENV.read().unwrap_or_else(PoisonError::into_inner);
    let path = merged_path(&spawn.path, env::var_os("PATH").as_deref())
        .map(|path| (OsString::from("PATH"), path));
    path.into_iter()
        .chain(
            spawn
                .vars
                .iter()
                .filter(|var| scope.admits(var))
                .map(|var| {
                    (
                        OsString::from(&var.name),
                        OsString::from(var.value.expose()),
                    )
                }),
        )
        .collect()
}

/// The `PATH` processes get, for finding a tool as they would.
pub(crate) fn search_path() -> Option<OsString> {
    let spawn = SPAWN_ENV.read().unwrap_or_else(PoisonError::into_inner);
    merged_path(&spawn.path, env::var_os("PATH").as_deref()).or_else(|| env::var_os("PATH"))
}

/// `prefix` then `inherited`, each folder once; `None` when `prefix` is
/// empty, so the inherited `PATH` stays as it is.
fn merged_path(prefix: &[PathBuf], inherited: Option<&OsStr>) -> Option<OsString> {
    if prefix.is_empty() {
        return None;
    }
    let mut dirs: Vec<PathBuf> = prefix.to_vec();
    for dir in inherited.map(env::split_paths).into_iter().flatten() {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    env::join_paths(dirs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::SecretText;
    use std::process::Stdio;

    #[cfg(unix)]
    #[test]
    fn the_tools_come_first_and_once() {
        let prefix = [PathBuf::from("/srv/node/bin")];
        let merged = merged_path(&prefix, Some(OsStr::new("/usr/bin:/srv/node/bin:/bin")));
        assert_eq!(merged.unwrap(), "/srv/node/bin:/usr/bin:/bin");
        assert_eq!(merged_path(&[], Some(OsStr::new("/usr/bin"))), None);
        assert_eq!(merged_path(&prefix, None).unwrap(), "/srv/node/bin");
    }

    async fn seen(scope: Scope) -> String {
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "printf '%s,%s' \"$BEEP_TEST_CREDENTIAL\" \"$GH_TOKEN\"",
            ])
            .env("BEEP_TEST_CREDENTIAL", "inherited")
            .env_remove("GH_TOKEN")
            .stdout(Stdio::piped());
        apply(&mut command, scope);
        let out = command.output().await.unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[tokio::test]
    async fn each_process_gets_only_its_credentials() {
        let _env = TEST_LOCK.lock().await;
        set_vars(vec![
            EnvSecret {
                name: "BEEP_TEST_CREDENTIAL".into(),
                value: SecretText::new("from-beep"),
            },
            EnvSecret {
                name: GITHUB_ENV_VAR.into(),
                value: SecretText::new("gh-token"),
            },
        ]);
        let shell = seen(Scope::Shell).await;
        let agent = seen(Scope::Agent(None)).await;
        let probe = seen(Scope::Probe).await;
        set_vars(Vec::new());
        assert_eq!(shell, "from-beep,gh-token");
        assert_eq!(agent, "inherited,gh-token");
        assert_eq!(probe, "inherited,");
    }

    async fn agent_token(scope: Scope) -> String {
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "printf '%s,%s' \"$CLAUDE_CODE_OAUTH_TOKEN\" \"$CODEX_API_KEY\"",
            ])
            .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
            .env_remove("CODEX_API_KEY")
            .stdout(Stdio::piped());
        apply(&mut command, scope);
        let out = command.output().await.unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[tokio::test]
    async fn an_agent_gets_only_its_own_token() {
        let _env = TEST_LOCK.lock().await;
        set_vars(vec![
            EnvSecret {
                name: AgentAccount::Claude.env_var().into(),
                value: SecretText::new("claude-token"),
            },
            EnvSecret {
                name: AgentAccount::Codex.env_var().into(),
                value: SecretText::new("codex-key"),
            },
        ]);
        let claude = agent_token(Scope::Agent(Some(AgentAccount::Claude))).await;
        let codex = agent_token(Scope::Agent(Some(AgentAccount::Codex))).await;
        let other = agent_token(Scope::Agent(None)).await;
        let shell = agent_token(Scope::Shell).await;
        set_vars(Vec::new());
        assert_eq!(claude, "claude-token,");
        assert_eq!(codex, ",codex-key");
        assert_eq!(other, ",");
        assert_eq!(shell, "claude-token,codex-key");
    }
}
