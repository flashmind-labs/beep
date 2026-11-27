//! The models a coding agent can run on here, for the start form's Model
//! field (`find`, `ModelCache`). flsh lists them with `flsh models`; the
//! others are read from a `session/new` made only to ask.

use super::{AgentKind, acp::{self, MAX_MODELS, Offered}, launch::{AgentIn, AgentOut, Started}, known};
use crate::{
    AsyncMutex,
    logs::found::Agent,
    lock,
    process::spawn_env::{self, Scope},
    process::{STOP_GRACE, command},
};
use agent_client_protocol::{Builder, Client, ConnectionTo, JsonRpcMessage};
use anyhow::{Context, Result};
use beep_proto::node::agent::{ModelList, ModelOption, ModelPick};
use std::{
    collections::HashMap,
    ffi::OsString,
    future::Future,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::mpsc, task::spawn_blocking, time::Instant};

mod context;
mod names;

/// How long an agent's list is kept.
const KEEP: Duration = Duration::from_secs(5 * 60);
/// How long an agent has to start and make a session; npx may first
/// download its adapter.
const PROBE_TIMEOUT: Duration = Duration::from_secs(45);
/// How long `flsh models` has to read its providers' catalogs.
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// The most of `flsh models`' output read.
const MAX_LIST_OUTPUT: u64 = 1024 * 1024;
/// The largest flsh `config.toml` read.
const MAX_CONFIG: u64 = 256 * 1024;

/// The lists found lately, by agent.
#[derive(Default)]
pub(super) struct ModelCache {
    found: Mutex<HashMap<String, Found>>,
    /// Held while an agent is asked, so a second request for the same agent
    /// waits for the first one's list. Each agent has its own: a slow probe
    /// of one does not hold back the others.
    finding: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

struct Found {
    list: ModelList,
    at: Instant,
}

impl ModelCache {
    /// `agent`'s list: the one kept when it is recent, else what `find`
    /// gives, kept.
    pub(super) async fn get(
        &self,
        agent: &str,
        find: impl Future<Output = ModelList>,
    ) -> ModelList {
        if let Some(list) = self.recent(agent, Instant::now()) {
            return list;
        }

        let finding = lock(&self.finding)
            .entry(agent.to_owned())
            .or_default()
            .clone();
        let _finding = finding.lock().await;

        if let Some(list) = self.recent(agent, Instant::now()) {
            return list;
        }

        let list = find.await;
        self.keep(agent, list.clone(), Instant::now());
        list
    }

    /// `agent`'s list as of `now`, when it was found less than `KEEP` ago.
    fn recent(&self, agent: &str, now: Instant) -> Option<ModelList> {
        let found = lock(&self.found);
        found
            .get(agent)
            .filter(|found| now.saturating_duration_since(found.at) < KEEP)
            .map(|found| found.list.clone())
    }

    fn keep(&self, agent: &str, list: ModelList, at: Instant) {
        let mut found = lock(&self.found);
        found.insert(agent.to_owned(), Found { list, at });
    }
}

/// The models `kind` can run on here. Never fails: an agent that cannot
/// say gives a list without models.
pub(super) async fn find(kind: AgentKind) -> ModelList {
    let flag = known(&kind.name).is_some_and(|known| known.model_flag);
    let agent = kind.name.parse::<Agent>().ok();

    if agent == Some(Agent::Flsh) {
        return flsh(&kind).await;
    }

    let Some(home) = dirs::home_dir() else {
        return listed(Offered::default(), flag);
    };
    let offered = match tokio::time::timeout(PROBE_TIMEOUT, probe(&kind, &home)).await {
        Ok(Ok(offered)) => offered,
        Ok(Err(e)) => {
            log::info!("{} did not list its models: {e:#}", kind.name);
            Offered::default()
        }
        Err(_) => {
            log::info!("{} did not list its models in time", kind.name);
            Offered::default()
        }
    };
    let mut list = listed(offered, flag);
    if agent == Some(Agent::Pi) {
        for model in &mut list.models {
            names::split_pi_name(model);
        }
    }

    context::enrich(&kind, &mut list).await;
    list
}

/// The list an ACP agent's `offered` models make. With none, a model can
/// still be named when the agent's command line takes one (`flag`).
fn listed(offered: Offered, flag: bool) -> ModelList {
    let pick = if !offered.models.is_empty() && (offered.switch.is_some() || flag) {
        ModelPick::Listed
    } else if flag {
        ModelPick::Named
    } else {
        ModelPick::Fixed
    };
    let models = match pick {
        ModelPick::Listed => offered.models,
        _ => Vec::new(),
    };
    ModelList {
        pick,
        current: offered.current,
        models,
    }
}

/// Starts `kind` in `cwd`, makes a session to read the models it offers,
/// and stops it.
async fn probe(kind: &AgentKind, cwd: &Path) -> Result<Offered> {
    let protocol = acp::Protocol::from_env(&kind.env)?;
    let mut process = command(&kind.program);
    spawn_env::apply(&mut process, Scope::for_agent(&kind.name, None));
    process.args(&kind.args).envs(&kind.env).current_dir(cwd);
    let Started {
        stdin,
        stdout,
        mut running,
        ..
    } = super::start(process, kind.bridge.as_ref())?;
    let asked = ask(stdin, stdout, cwd, protocol).await;
    running.stop(STOP_GRACE).await;
    asked
}

/// Asks the agent on `stdin` and `stdout` for a session in `cwd` and reads
/// the models its answer offers.
async fn ask(stdin: AgentIn, stdout: AgentOut, cwd: &Path, protocol: acp::Protocol) -> Result<Offered> {
    // An oversized answer is dropped, and the request then fails.
    let (oversized, _reports) = mpsc::channel(1);
    let (protocol, answer) = Builder::new(Client)
        .name("beep-node")
        .connect_with(
            acp::transport(stdin, stdout, oversized),
            async |cx: ConnectionTo<agent_client_protocol::Agent>| {
                let request = protocol.request(acp::initialize(false).to_untyped_message()?)?;
                let answer = cx.send_request(request).block_task().await?;
                let protocol = protocol.initialized(answer)?.protocol;
                let request = protocol.request(acp::new_session(cwd, Vec::new(), None, true)?)?;
                let answer = cx.send_request(request).block_task().await?;
                Ok((protocol, answer))
            },
        )
        .await
        .context("the agent did not make a session")?;
    Ok(protocol.offers(&answer))
}

/// flsh's models: `flsh models` lists every configured provider's, and its
/// `config.toml` names the default. With no list, any name can be given.
async fn flsh(kind: &AgentKind) -> ModelList {
    let current = match flsh_dir_of(kind) {
        Some(dir) => spawn_blocking(move || flsh_default_model(&dir.join("config.toml")))
            .await
            .unwrap_or_else(|e| {
                log::warn!("reading flsh's config failed: {e}");
                None
            }),
        None => None,
    };
    let models = match tokio::time::timeout(LIST_TIMEOUT, flsh_models(kind)).await {
        Ok(Ok(models)) => models,
        Ok(Err(e)) => {
            log::info!("flsh did not list its models: {e:#}");
            Vec::new()
        }
        Err(_) => {
            log::info!("flsh did not list its models in time");
            Vec::new()
        }
    };
    let pick = if models.is_empty() {
        ModelPick::Named
    } else {
        ModelPick::Listed
    };
    ModelList {
        pick,
        current,
        models,
    }
}

/// What `flsh models` prints: one `provider:model` per line.
async fn flsh_models(kind: &AgentKind) -> Result<Vec<ModelOption>> {
    let mut process = command(&kind.program);
    spawn_env::apply(&mut process, Scope::Agent(None));
    process
        .arg("models")
        .envs(&kind.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = process
        .spawn()
        .with_context(|| format!("starting {} models", kind.program))?;
    let stdout = child.stdout.take().context("flsh has no stdout")?;
    let mut output = Vec::new();
    stdout
        .take(MAX_LIST_OUTPUT)
        .read_to_end(&mut output)
        .await
        .context("reading flsh's models")?;
    let status = child.wait().await.context("waiting for flsh")?;
    anyhow::ensure!(status.success(), "flsh models exited with {status}");
    Ok(flsh_listing(&String::from_utf8_lossy(&output)))
}

/// The models in `flsh models`' output, in its order.
fn flsh_listing(output: &str) -> Vec<ModelOption> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains(char::is_whitespace))
        .take(MAX_MODELS)
        .map(|id| {
            let (provider, name) = names::flsh_name(id);
            ModelOption {
                id: id.to_owned(),
                name,
                provider,
                ..ModelOption::default()
            }
        })
        .collect()
}

/// Where `kind` keeps flsh's `config.toml`: flsh runs with the kind's
/// variables on top of the node's own.
pub(super) fn flsh_dir_of(kind: &AgentKind) -> Option<PathBuf> {
    flsh_config_dir(&env_of(kind), dirs::home_dir().as_deref())
}

/// Where `kind` keeps pi's `models.json`.
pub(super) fn pi_dir_of(kind: &AgentKind) -> Option<PathBuf> {
    crate::logs::found::pi_agent_dir(&env_of(kind))
}

/// The variables `kind` runs with: its own on top of the node's.
fn env_of(kind: &AgentKind) -> impl Fn(&str) -> Option<OsString> + '_ {
    |name: &str| match kind.env.get(name) {
        Some(value) => Some(OsString::from(value)),
        None => std::env::var_os(name),
    }
}

/// Where flsh keeps `config.toml`, as flsh finds it: `$FLASHMIND_HOME`,
/// else `~/.flashmind` when it exists, else the platform's config folder
/// (`$XDG_CONFIG_HOME` or `~/.config` outside Windows) plus `flashmind`.
fn flsh_config_dir(env: &dyn Fn(&str) -> Option<OsString>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = env("FLASHMIND_HOME")
        && !dir.is_empty()
    {
        let dir = PathBuf::from(dir);
        return dir.is_absolute().then_some(dir);
    }

    if let Some(legacy) = home.map(|home| home.join(".flashmind"))
        && legacy.is_dir()
    {
        return Some(legacy);
    }

    crate::config_home(env, home).map(|dir| dir.join("flashmind"))
}

/// The `model` flsh's `config.toml` at `path` names, when it does.
fn flsh_default_model(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();

    if let Err(e) = file.take(MAX_CONFIG).read_to_string(&mut text) {
        log::debug!("flsh's config not read; path={} error={e}", path.display());
        return None;
    }

    let config: toml::Table = match toml::from_str(&text) {
        Ok(config) => config,
        Err(e) => {
            log::debug!("flsh's config not read; path={} error={e}", path.display());
            return None;
        }
    };
    config
        .get("model")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "../tests/agents_models.rs"]
mod tests;
