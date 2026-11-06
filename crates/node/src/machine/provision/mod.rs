//! Readies the machine for Beep's agents: `Provision` installs `gh`, `teacli`,
//! `flsh` and Beep's skills, and `Provision::apply` hands the processes the
//! node starts the user's credentials, kept in memory.

mod skills;
pub(crate) mod spawn_env;
mod teacli;
mod tools;

pub(crate) use skills::installed_by_beep;

use crate::{AsyncMutex, lock};
use anyhow::{Context, Result};
use beep_proto::node::{
    ForgejoLogin, LinkRx, LinkTx, MachineCredentials,
    tool_update::{Reply, Request, Tool},
};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{sync::Notify, task::spawn_blocking};
use tokio_util::task::AbortOnDropHandle;

/// What the node readies on the machine, made once per node process.
pub struct Provision {
    /// The node's own folder: `bin` for the tools it installs, `shims` and
    /// `teacli` for teacli's config.
    dir: PathBuf,
    /// The Forgejo servers from the last credentials, for when teacli is
    /// installed after they came.
    forgejo: Mutex<Vec<ForgejoLogin>>,
    /// Woken when a tool was installed, so the machine is probed again.
    changed: Notify,
    /// Held while teacli's config is written, so two writes never mix.
    teacli: AsyncMutex<()>,
    /// Held while a tool is installed, so two installs never share a
    /// download folder.
    installing: AsyncMutex<()>,
}

impl Provision {
    /// The provisioning for a node whose folder is `dir`. Processes started from
    /// now on find the tools it installs.
    pub fn new(dir: &Path) -> Arc<Self> {
        let provision = Self {
            dir: dir.to_owned(),
            forgejo: Mutex::default(),
            changed: Notify::new(),
            teacli: AsyncMutex::new(()),
            installing: AsyncMutex::new(()),
        };
        spawn_env::set_path(vec![provision.bin()]);
        Arc::new(provision)
    }

    fn bin(&self) -> PathBuf {
        self.dir.join("bin")
    }

    /// Removes the teacli config a node before this one left, then
    /// installs the skills, then each missing tool in turn, in the
    /// background. A failure is logged and the node goes on without it.
    /// Dropping the handle stops the work.
    pub fn start(self: &Arc<Self>) -> AbortOnDropHandle<()> {
        let provision = self.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            // Tokens wait for the server: a node that stopped while they
            // were revoked must not keep using them.
            provision.configure_teacli().await;
            let dir = provision.dir.clone();
            match spawn_blocking(move || install_skills(&dir)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("agent skills not installed: {e:#}"),
                Err(e) => log::warn!("installing agent skills failed: {e}"),
            }
            if provision.install_tools().await {
                provision.configure_teacli().await;
                provision.changed.notify_one();
            }
        }))
    }

    /// Whether any tool was installed.
    async fn install_tools(&self) -> bool {
        let _one = self.installing.lock().await;
        let mut installed = false;
        let missing = |name| which::which_in(name, Some(self.tool_path()), "/").is_err();
        for tool in tools::PINNED {
            if missing(tool.name) {
                log::info!("installing {}", tool.name);
                installed |= installed_ok(tool.name, tool.install(&self.bin()).await);
            }
        }
        if missing("flsh") && tools::flsh_target(env::consts::OS, env::consts::ARCH).is_some() {
            log::info!("installing flsh");
            let result = tools::install_newest(Tool::Flsh, &self.bin()).await;
            installed |= installed_ok("flsh", result);
        }
        installed
    }

    /// Serves one `tool-update/1` stream: installs the release the server
    /// names, then has the machine probed again.
    pub(crate) async fn serve_updates(&self, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
        let Some(Request::Install { tool, version }) = rx.recv::<Request>().await? else {
            return Ok(());
        };
        let reply = match self.update(tool, &version).await {
            Ok(()) => Reply::Installed {},
            Err(e) => {
                log::warn!("{tool} {version} not installed: {e:#}");
                Reply::Failed {
                    reason: format!("{e:#}"),
                }
            }
        };
        tx.send(&reply).await?;
        tx.finish().await
    }

    async fn update(&self, tool: Tool, version: &str) -> Result<()> {
        {
            let _one = self.installing.lock().await;
            log::info!("installing {tool} {version}");
            let path = tools::install_release(tool, version, &self.bin()).await?;
            log::info!("installed {tool} {version} at {}", path.display());
        }
        if tool == Tool::Teacli {
            self.configure_teacli().await;
        }
        self.changed.notify_one();
        Ok(())
    }

    /// Takes the user's credentials in place of the ones before.
    pub async fn apply(&self, credentials: MachineCredentials) {
        let MachineCredentials { env, forgejo } = credentials;
        log::info!(
            "credentials: {} env vars, {} Forgejo servers",
            env.len(),
            forgejo.len()
        );
        spawn_env::set_vars(env);
        *lock(&self.forgejo) = forgejo;
        self.configure_teacli().await;
    }

    /// Drops the credentials, from memory and from teacli's config, when
    /// the server can no longer say they are current.
    pub async fn forget(&self) {
        spawn_env::set_vars(Vec::new());
        lock(&self.forgejo).clear();
        self.configure_teacli().await;
    }

    /// Finishes when a tool was installed since the last call.
    pub async fn changed(&self) {
        self.changed.notified().await;
    }

    /// Points teacli at the Forgejo servers, or back at the user's own
    /// config when there are none. The one place tokens reach disk: teacli
    /// reads them only from a config file.
    async fn configure_teacli(&self) {
        let _writing = self.teacli.lock().await;
        let logins = lock(&self.forgejo).clone();
        let dir = self.dir.clone();
        let teacli = which::which_in("teacli", Some(self.tool_path()), "/").ok();
        let configured = spawn_blocking(move || {
            let user_ini = dirs::home_dir()
                .map(|home| teacli::user_config(&home))
                .and_then(|path| fs::read_to_string(path).ok());
            teacli::configure(&dir, &logins, user_ini.as_deref(), teacli.as_deref())
        })
        .await
        .context("configuring teacli stopped");
        let shim = match configured {
            Ok(Ok(shim)) => shim,
            Ok(Err(e)) | Err(e) => {
                log::warn!("teacli not configured: {e:#}");
                false
            }
        };
        let mut path = Vec::new();
        if shim {
            path.push(teacli::shims_dir(&self.dir));
        }
        path.push(self.bin());
        spawn_env::set_path(path);
    }

    /// The node's `bin` then the inherited `PATH`, without the shims: where
    /// the real tools are.
    fn tool_path(&self) -> OsString {
        let inherited = env::var_os("PATH").unwrap_or_default();
        let dirs = std::iter::once(self.bin()).chain(env::split_paths(&inherited));
        env::join_paths(dirs).unwrap_or(inherited)
    }
}

/// Logs how installing `name` went; whether it was installed.
fn installed_ok(name: &str, result: Result<PathBuf>) -> bool {
    match result {
        Ok(path) => {
            log::info!("installed {name} at {}", path.display());
            true
        }
        Err(e) => {
            log::warn!("{name} not installed: {e:#}");
            false
        }
    }
}

/// Installs every skill into every agent's skills folder found, keeping
/// the record of what it wrote in the node's folder `node_dir`.
fn install_skills(node_dir: &Path) -> Result<()> {
    let home = dirs::home_dir().context("no home folder")?;
    let mut installed = skills::Installed::load(&node_dir.join("installed-skills"))?;
    for dir in skills::targets(&home) {
        for skill in skills::SKILLS {
            let outcome = skills::install(&dir, skill, &mut installed)
                .with_context(|| format!("installing the {} skill", skill.name))?;
            log::debug!("skill {} in {}: {outcome:?}", skill.name, dir.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{SecretText, node::EnvSecret};

    #[tokio::test]
    async fn forgetting_removes_the_tokens_from_disk() {
        let _env = spawn_env::TEST_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let teacli = dir.path().join("bin/teacli");
        fs::create_dir_all(teacli.parent().unwrap()).unwrap();
        fs::write(&teacli, "#!/bin/sh\n").unwrap();
        crate::files::fsat::set_mode(&teacli, 0o755).unwrap();
        let provision = Provision::new(dir.path());
        provision
            .apply(MachineCredentials {
                env: vec![EnvSecret {
                    name: "BEEP_TEST_FORGET".into(),
                    value: SecretText::new("key"),
                }],
                forgejo: vec![ForgejoLogin {
                    url: "https://git.example.com".into(),
                    token: SecretText::new("token"),
                }],
            })
            .await;
        let config = teacli::config_path(dir.path());
        assert!(config.exists());
        provision.forget().await;
        assert!(!config.exists());
        assert!(!teacli::shims_dir(dir.path()).join("teacli").exists());
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "printf %s \"$BEEP_TEST_FORGET\""]);
        spawn_env::apply(&mut command, spawn_env::Scope::Shell);
        let out = command.output().await.unwrap();
        assert!(out.stdout.is_empty());
    }
}
