//! Readies the machine for Beep's agents: `Provision` installs `gh`, `teacli`,
//! `flsh` (in `~/.local/bin`) and Beep's skills, and `Provision::apply` hands the processes the
//! node starts the user's credentials, kept in memory.

mod setup;
mod skills;
mod teacli;
mod tools;

pub(crate) use skills::{beep_dir as skills_dir, brief as skills_brief, installed_by_beep};
pub(crate) use tools::{curl, listed_sha256, remove_dir, unpack, verify};

use crate::{AsyncMutex, lock, machine::spawn_env};
use anyhow::{Context, Result};
use beep_proto::node::{
    ForgejoLogin, LinkRx, LinkTx, MachineCredentials,
    setup::{Need, Reply as SetupReply, Request as SetupRequest},
    tool_update::{Available, Check, Reply, Request, Tool},
};
use beep_proto::node::events::Topic;
use beep_proto::watched;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Notify, OwnedMutexGuard, watch},
    task::spawn_blocking,
};
use tokio_util::task::AbortOnDropHandle;

/// The topic that says `Provision::updatable` changed.
pub(crate) const UPDATES_TOPIC: &str = "tool-updates";

/// One lock for each tool, taken while it is installed.
#[derive(Default)]
struct Installs(Mutex<HashMap<String, Arc<AsyncMutex<()>>>>);

impl Installs {
    async fn lock(&self, tool: impl Display) -> OwnedMutexGuard<()> {
        let one = lock(&self.0).entry(tool.to_string()).or_default().clone();

        one.lock_owned().await
    }
}

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
    /// Held while a tool is installed, so two installs of one tool never share
    /// its download folder. Installs of different tools run together.
    installing: Installs,
    updates: watch::Sender<Option<Vec<Available>>>,
    recheck: Notify,
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
            installing: Installs::default(),
            updates: watch::channel(None).0,
            recheck: Notify::new(),
        };
        spawn_env::set_path(provision.tool_dirs());
        Arc::new(provision)
    }

    /// The node's own folder.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    fn bin(&self) -> PathBuf {
        self.dir.join("bin")
    }

    /// Where flsh goes: the user's `~/.local/bin`, else the node's `bin`
    /// when there is no home folder.
    fn flsh_bin(&self) -> PathBuf {
        dirs::home_dir().map_or_else(|| self.bin(), |home| tools::flsh_bin(&home))
    }

    /// The folder `tool` is installed in.
    fn bin_of(&self, tool: Tool) -> PathBuf {
        match tool {
            Tool::Flsh => self.flsh_bin(),
            Tool::Gh | Tool::Teacli => self.bin(),
        }
    }

    /// Removes the teacli config a node before this one left, then
    /// installs the skills, then each missing tool in turn, in the
    /// background. A failure is logged and the node goes on without it.
    /// Dropping the handle stops the work.
    pub fn start(self: &Arc<Self>) -> AbortOnDropHandle<()> {
        let provision = self.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            let checks = provision.clone();
            let checks = tokio::spawn(async move { checks.check_loop().await });
            let _checks = AbortOnDropHandle::new(checks);
            // Tokens wait for the mesh: a node that stopped while they
            // were revoked must not keep using them.
            provision.configure_teacli().await;
            let (bin, flsh_bin) = (provision.bin(), provision.flsh_bin());

            match spawn_blocking(move || tools::settle_flsh(&bin, &flsh_bin)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("flsh not moved out of the node's folder: {e}"),
                Err(e) => log::warn!("moving flsh failed: {e}"),
            }
            let dir = provision.dir.clone();

            match spawn_blocking(move || install_skills(&dir)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("agent skills not installed: {e:#}"),
                Err(e) => log::warn!("installing agent skills failed: {e}"),
            }

            if provision.install_tools().await {
                provision.configure_teacli().await;
                provision.changed.notify_one();
                provision.recheck.notify_one();
            }
            std::future::pending::<()>().await;
        }))
    }

    async fn check_loop(&self) {
        // One lookup table per tool: a table holds its lock across the
        // lookup, so a shared one would run the tools' lookups in turn.
        let releases: Vec<_> = Tool::ALL
            .into_iter()
            .map(|tool| (tool, beep_mesh::machine::releases::ToolReleases::default()))
            .collect();
        let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;

        loop {
            let (installed, _) =
                crate::machine::env::probe_tools(crate::machine::env::ToolScope::Updatable).await;
            // Each tool's latest release is looked up together, then published once.
            let lookups = releases.iter().filter_map(|(tool, releases)| {
                let tool = *tool;
                let name: &str = tool.into();
                let version = installed
                    .iter()
                    .find(|item| item.name == name)
                    .and_then(|item| item.version.clone())?;

                Some(async move {
                    let latest = releases.latest(tool).await?;

                    beep_proto::node::harness::is_newer(&latest, &version).then_some(Available {
                        tool,
                        version,
                        latest,
                    })
                })
            });
            let updates: Vec<_> = futures::future::join_all(lookups)
                .await
                .into_iter()
                .flatten()
                .collect();

            self.publish(updates);
            tokio::select! {
                _ = interval.tick() => {},
                _ = self.recheck.notified() => {},
            }
        }
    }

    /// Keeps `updates` as the latest check and says so on the `tool-updates`
    /// topic when it differs from the one before. Returns whether it did.
    fn publish(&self, updates: Vec<Available>) -> bool {
        let changed = self.updates.send_if_modified(|known| {
            let changed = known.as_ref() != Some(&updates);
            *known = Some(updates);
            changed
        });

        if changed {
            crate::events::system().changed(Topic::new(UPDATES_TOPIC));
        }

        changed
    }

    /// The tools with a newer release, once the first check ended.
    pub(crate) fn updatable(&self) -> Option<Vec<Available>> {
        watched::read_sent(&self.updates, Clone::clone)
    }

    pub(crate) async fn serve_tool_checks(&self, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
        if rx.recv::<Check>().await?.is_some() {
            let mut updates = self.updates.subscribe();
            let available = tokio::time::timeout(Duration::from_secs(55), async {
                loop {
                    if let Some(available) = watched::read_and_update(&mut updates, Clone::clone) {
                        break available;
                    }

                    if updates.changed().await.is_err() {
                        break Vec::new();
                    }
                }
            })
            .await
            .unwrap_or_default();
            tx.send(&available).await?;
        }

        tx.finish().await
    }

    /// Installs the compiler cache separately so it never delays other tools.
    pub fn start_sccache(self: &Arc<Self>) -> AbortOnDropHandle<()> {
        let provision = self.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            if which::which_in("sccache", Some(provision.tool_path()), "/").is_ok() {
                return;
            }

            log::info!("installing sccache");

            if installed_ok("sccache", tools::SCCACHE.install(&provision.bin()).await) {
                provision.changed.notify_one();
            }
        }))
    }

    /// Whether any tool was installed. The missing tools install together.
    async fn install_tools(&self) -> bool {
        let missing = |name| which::which_in(name, Some(self.tool_path()), "/").is_err();
        let pinned = tools::PINNED.iter().map(|tool| async move {
            let _one = self.installing.lock(tool.name).await;

            if !missing(tool.name) {
                return false;
            }

            log::info!("installing {}", tool.name);
            installed_ok(tool.name, tool.install(&self.bin()).await)
        });
        let flsh = async {
            let _one = self.installing.lock(Tool::Flsh).await;

            if !missing("flsh") || tools::flsh_target(env::consts::OS, env::consts::ARCH).is_none()
            {
                return false;
            }

            log::info!("installing flsh");
            let result = tools::install_newest(Tool::Flsh, &self.flsh_bin()).await;
            installed_ok("flsh", result)
        };
        let (pinned, flsh) = futures::future::join(futures::future::join_all(pinned), flsh).await;

        flsh || pinned.into_iter().any(|installed| installed)
    }

    /// Serves one `tool-update/1` stream: installs the release the caller
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

    /// Serves one `setup/1` stream: installs what the desktop asks for,
    /// then has the machine probed again.
    pub(crate) async fn serve_setup(&self, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
        let Some(SetupRequest::Install { need }) = rx.recv::<SetupRequest>().await? else {
            return Ok(());
        };
        let reply = self.set_up(need).await.unwrap_or_else(|e| {
            log::warn!("{need} not installed: {e:#}");
            SetupReply::Failed {
                reason: format!("{e:#}"),
            }
        });
        tx.send(&reply).await?;
        tx.finish().await
    }

    async fn set_up(&self, need: Need) -> Result<SetupReply> {
        let reply = {
            let _one = self.installing.lock(need).await;
            log::info!("installing {need}");

            match need {
                Need::Node => {
                    setup::install_node(&self.dir).await?;
                    SetupReply::Installed {}
                }
                Need::Git => setup::install_git().await?,
            }
        };

        // Puts the folders it installed into on processes' PATH.
        self.configure_teacli().await;
        self.changed.notify_one();
        Ok(reply)
    }

    async fn update(&self, tool: Tool, version: &str) -> Result<()> {
        {
            let _one = self.installing.lock(tool).await;
            log::info!("installing {tool} {version}");
            let path = tools::install_release(tool, version, &self.bin_of(tool)).await?;
            log::info!("installed {tool} {version} at {}", path.display());
        }
        if tool == Tool::Teacli {
            self.configure_teacli().await;
        }

        self.changed.notify_one();
        self.recheck.notify_one();
        Ok(())
    }

    /// Takes the user's credentials in place of the ones before.
    pub async fn apply(&self, credentials: MachineCredentials) {
        let MachineCredentials {
            env,
            forgejo,
            agents,
        } = credentials;
        log::info!(
            "credentials: {} env vars, {} Forgejo servers, {} agent accounts",
            env.len(),
            forgejo.len(),
            agents.len()
        );
        spawn_env::set_accounts(agents);
        spawn_env::set_vars(env);
        *lock(&self.forgejo) = forgejo;
        self.configure_teacli().await;
    }

    /// Drops the credentials, from memory and from teacli's config, when
    /// the node stops.
    pub async fn forget(&self) {
        spawn_env::set_accounts(Vec::new());
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
        path.extend(self.tool_dirs());
        spawn_env::set_path(path);
    }

    /// The node's `bin`, flsh's, then what the setup installed.
    fn tool_dirs(&self) -> Vec<PathBuf> {
        [self.bin(), self.flsh_bin()]
            .into_iter()
            .chain(setup::path_dirs(&self.dir))
            .collect()
    }

    /// The node's `bin`, flsh's, then the inherited `PATH`, without the
    /// shims: where the real tools are.
    fn tool_path(&self) -> OsString {
        let inherited = env::var_os("PATH").unwrap_or_default();
        let dirs = [self.bin(), self.flsh_bin()]
            .into_iter()
            .chain(env::split_paths(&inherited));
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

/// Installs every skill into Beep's own skills folder, keeping the record
/// of what it wrote in the node's folder `node_dir`.
fn install_skills(node_dir: &Path) -> Result<()> {
    let beep = skills::beep_dir().context("no config folder")?;
    let mut installed = skills::Installed::load(&node_dir.join("installed-skills"))?;

    for skill in skills::SKILLS {
        let outcome = skills::install(&beep, skill, &mut installed)
            .with_context(|| format!("installing the {} skill", skill.name))?;
        log::debug!("skill {} in {}: {outcome:?}", skill.name, beep.display());
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/machine_provision.rs"]
mod tests;
