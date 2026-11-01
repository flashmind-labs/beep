//! Readies the machine for the agents Beep runs on it: installs `gh` and
//! `teacli` where they are missing and Beep's GitHub and Forgejo skills,
//! and hands the processes the node starts the user's credentials (see
//! `Down::Credentials`), which it keeps in memory. Forgejo tokens are the
//! exception: teacli reads them only from a config file, written readable
//! by the owner only.

mod skills;
pub(crate) mod spawn_env;
mod teacli;
mod tools;

use anyhow::{Context, Result};
use beep_proto::node::{ForgejoLogin, MachineCredentials};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
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
}

impl Provision {
    /// The provisioning for a node whose folder is `dir`. Processes started from
    /// now on find the tools it installs.
    pub fn new(dir: &Path) -> Arc<Self> {
        let provision = Self {
            dir: dir.to_owned(),
            forgejo: Mutex::default(),
            changed: Notify::new(),
        };
        spawn_env::set_path(vec![provision.bin()]);
        Arc::new(provision)
    }

    fn bin(&self) -> PathBuf {
        self.dir.join("bin")
    }

    /// Installs the skills, then each missing tool in turn, in the
    /// background. A failure is logged and the node goes on without it.
    /// Dropping the handle stops the work.
    pub fn start(self: &Arc<Self>) -> AbortOnDropHandle<()> {
        let provision = self.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            match spawn_blocking(install_skills).await {
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
        let mut installed = false;
        for tool in tools::PINNED {
            if which::which_in(tool.name, Some(self.tool_path()), "/").is_ok() {
                continue;
            }
            log::info!("installing {}", tool.name);
            match tool.install(&self.bin()).await {
                Ok(path) => {
                    log::info!("installed {} at {}", tool.name, path.display());
                    installed = true;
                }
                Err(e) => log::warn!("{} not installed: {e:#}", tool.name),
            }
        }
        installed
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
        *self.forgejo.lock().unwrap_or_else(PoisonError::into_inner) = forgejo;
        self.configure_teacli().await;
    }

    /// Finishes when a tool was installed since the last call.
    pub async fn changed(&self) {
        self.changed.notified().await;
    }

    /// Points teacli at the Forgejo servers, or back at the user's own
    /// config when there are none.
    async fn configure_teacli(&self) {
        let logins = self
            .forgejo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
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

/// Installs every skill into every agent's skills folder found.
fn install_skills() -> Result<()> {
    let home = dirs::home_dir().context("no home folder")?;
    for dir in skills::targets(&home) {
        for skill in skills::SKILLS {
            let outcome = skills::install(&dir, skill)
                .with_context(|| format!("installing the {} skill", skill.name))?;
            log::debug!("skill {} in {}: {outcome:?}", skill.name, dir.display());
        }
    }
    Ok(())
}
