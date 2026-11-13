//! What processes the node starts get on top of its own environment: its tool
//! folders first on `PATH` and, per [`Scope`], the user's credentials. Entry
//! points are `apply` and `vars`.

use crate::{read_lock, write_lock};
use beep_proto::{AgentAccount, GITHUB_ENV_VAR, SecretText, node::EnvSecret};
use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
    str::FromStr,
    sync::RwLock,
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
    /// shim on `PATH`.
    Agent(Option<AgentAccount>),
    /// Every credential the user gave to their machines: exec, background
    /// tasks and terminals, where the user runs their own commands.
    Shell,
}

impl Scope {
    /// What the agent called `name` gets.
    pub(crate) fn for_agent(name: &str) -> Self {
        Self::Agent(AgentAccount::from_str(name).ok())
    }

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

/// Process-wide, like the environment it extends: agents, exec, background
/// tasks and the machine probe each read it as they start a process.
static SPAWN_ENV: RwLock<SpawnEnv> = RwLock::new(SpawnEnv {
    path: Vec::new(),
    vars: Vec::new(),
});

/// Held by tests that set the process-wide env, so they do not see each
/// other's.
#[cfg(test)]
pub(crate) static TEST_LOCK: crate::AsyncMutex<()> = crate::AsyncMutex::const_new(());

/// Puts `dirs` before the inherited `PATH` of every process started from
/// now on.
pub(super) fn set_path(dirs: Vec<PathBuf>) {
    write_lock(&SPAWN_ENV).path = dirs;
}

/// Sets `vars` on every process started from now on, in place of the
/// credentials set before.
pub(crate) fn set_vars(vars: Vec<EnvSecret>) {
    write_lock(&SPAWN_ENV).vars = vars;
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
    let spawn = read_lock(&SPAWN_ENV);
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

/// The credential named `name` that processes get, if the user gave one.
pub(crate) fn secret(name: &str) -> Option<SecretText> {
    read_lock(&SPAWN_ENV)
        .vars
        .iter()
        .find(|var| var.name == name)
        .map(|var| var.value.clone())
}

/// The `PATH` processes get, for finding a tool as they would.
pub(crate) fn search_path() -> Option<OsString> {
    let spawn = read_lock(&SPAWN_ENV);
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
#[path = "../../tests/machine_provision_spawn_env.rs"]
mod tests;
