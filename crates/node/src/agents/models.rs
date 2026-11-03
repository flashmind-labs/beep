//! The models a coding agent can run on here, for the start form's Model
//! field. flsh lists them with `flsh models` and names its default in its
//! `config.toml`; the others say in their answer to `session/new`, so a
//! session is made once to ask and then closed. Each agent's list is kept a
//! few minutes, and one agent is asked at a time.

use super::{
    AgentKind,
    acp::{self, MAX_MODELS, Offered},
};
use crate::{
    AsyncMutex,
    found::Agent,
    process::{GroupLeader, STOP_GRACE, command, own_group},
    provision::spawn_env::{self, Scope},
};
use agent_client_protocol::{Client, ConnectionTo};
use anyhow::{Context, Result};
use beep_proto::{
    AgentAccount,
    node::agent::{ModelList, ModelOption, ModelPick},
};
use std::{
    collections::HashMap,
    ffi::OsString,
    future::Future,
    path::{Path, PathBuf},
    process::Stdio,
    str::FromStr,
    sync::{Mutex, PoisonError},
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::mpsc, task::spawn_blocking, time::Instant};

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
    /// Held while an agent is asked, so one is asked at a time and a second
    /// request for the same agent waits for the first one's list.
    finding: AsyncMutex<()>,
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
        let _finding = self.finding.lock().await;
        if let Some(list) = self.recent(agent, Instant::now()) {
            return list;
        }
        let list = find.await;
        self.keep(agent, list.clone(), Instant::now());
        list
    }

    /// `agent`'s list as of `now`, when it was found less than `KEEP` ago.
    fn recent(&self, agent: &str, now: Instant) -> Option<ModelList> {
        let found = self.found.lock().unwrap_or_else(PoisonError::into_inner);
        found
            .get(agent)
            .filter(|found| now.saturating_duration_since(found.at) < KEEP)
            .map(|found| found.list.clone())
    }

    fn keep(&self, agent: &str, list: ModelList, at: Instant) {
        let mut found = self.found.lock().unwrap_or_else(PoisonError::into_inner);
        found.insert(agent.to_owned(), Found { list, at });
    }
}

/// The models `kind` can run on here. Never fails: an agent that cannot
/// say gives a list without models.
pub(super) async fn find(kind: AgentKind) -> ModelList {
    let flag = acp::model_flag(&kind.name).is_some();
    if Agent::from_name(&kind.name) == Some(Agent::Flsh) {
        return flsh(&kind).await;
    }
    let Some(home) = dirs::home_dir() else {
        return listed(Offered::default(), flag);
    };
    match tokio::time::timeout(PROBE_TIMEOUT, probe(&kind, &home)).await {
        Ok(Ok(offered)) => listed(offered, flag),
        Ok(Err(e)) => {
            log::info!("{} did not list its models: {e:#}", kind.name);
            listed(Offered::default(), flag)
        }
        Err(_) => {
            log::info!("{} did not list its models in time", kind.name);
            listed(Offered::default(), flag)
        }
    }
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
    let mut process = command(&kind.program);
    let account = AgentAccount::from_str(&kind.name).ok();
    spawn_env::apply(&mut process, Scope::Agent(account));
    process
        .args(&kind.args)
        .envs(&kind.env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = own_group(&mut process)
        .spawn()
        .with_context(|| format!("starting {}", kind.program))?;
    let mut child = GroupLeader::new(child);
    let held = child.hold_output();
    let asked = ask(child.child_mut(), cwd).await;
    child.stop(STOP_GRACE).await;
    drop(held);
    asked
}

/// Asks the agent on `child`'s stdio for a session in `cwd` and reads the
/// models its answer offers.
async fn ask(child: &mut tokio::process::Child, cwd: &Path) -> Result<Offered> {
    let stdin = child.stdin.take().context("the agent has no stdin")?;
    let stdout = child.stdout.take().context("the agent has no stdout")?;
    // An oversized answer is dropped, and the request then fails.
    let (oversized, _reports) = mpsc::channel(1);
    let answer = Client
        .builder()
        .name("beep-node")
        .connect_with(
            acp::transport(stdin, stdout, oversized),
            async |cx: ConnectionTo<agent_client_protocol::Agent>| {
                cx.send_request(acp::initialize()).block_task().await?;
                let request = acp::new_session(cwd)?;
                cx.send_request(request).block_task().await
            },
        )
        .await
        .context("the agent did not make a session")?;
    Ok(acp::offered_models(&answer))
}

/// flsh's models: `flsh models` lists every configured provider's, and its
/// `config.toml` names the default. With no list, any name can be given.
async fn flsh(kind: &AgentKind) -> ModelList {
    // flsh runs with the kind's variables on top of the node's own.
    let env = |name: &str| match kind.env.get(name) {
        Some(value) => Some(OsString::from(value)),
        None => std::env::var_os(name),
    };
    let config = flsh_config_dir(&env, dirs::home_dir().as_deref());
    let current = match config {
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
        .map(|id| ModelOption {
            id: id.to_owned(),
            name: id.to_owned(),
            description: None,
        })
        .collect()
}

/// Where flsh keeps `config.toml`, as flsh finds it: `$FLASHMIND_HOME`,
/// else `~/.flashmind` when it exists, else the platform's config folder
/// (`$XDG_CONFIG_HOME` or `~/.config` outside Windows) plus `flashmind`.
fn flsh_config_dir(env: &dyn Fn(&str) -> Option<OsString>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = env("FLASHMIND_HOME").filter(|dir| !dir.is_empty()) {
        let dir = PathBuf::from(dir);
        return dir.is_absolute().then_some(dir);
    }
    if let Some(legacy) = home.map(|home| home.join(".flashmind"))
        && legacy.is_dir()
    {
        return Some(legacy);
    }
    if cfg!(windows) {
        return dirs::config_dir().map(|dir| dir.join("flashmind"));
    }
    env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| home.map(|home| home.join(".config")))
        .map(|dir| dir.join("flashmind"))
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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn list(current: &str) -> ModelList {
        ModelList {
            pick: ModelPick::Named,
            current: Some(current.into()),
            models: Vec::new(),
        }
    }

    #[test]
    fn a_list_is_kept_a_few_minutes() {
        let cache = ModelCache::default();
        let start = Instant::now();
        cache.keep("flsh", list("openai:gpt-test"), start);
        let later = start + Duration::from_secs(60);
        assert_eq!(cache.recent("flsh", later), Some(list("openai:gpt-test")));
        assert_eq!(cache.recent("codex", later), None, "kept per agent");
        assert_eq!(cache.recent("flsh", start + KEEP), None, "too old");
    }

    #[tokio::test]
    async fn an_agent_is_asked_once_while_its_list_is_recent() {
        let cache = ModelCache::default();
        let asked = AtomicUsize::new(0);
        let find = || async {
            asked.fetch_add(1, Ordering::SeqCst);
            list("m")
        };
        assert_eq!(cache.get("claude", find()).await, list("m"));
        assert_eq!(cache.get("claude", find()).await, list("m"));
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        cache.get("codex", find()).await;
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn flsh_lists_one_model_per_line() {
        let output = "openai:gpt-test\n\nopenrouter:vendor/model-a\nnot a model\n";
        let ids: Vec<String> = flsh_listing(output).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["openai:gpt-test", "openrouter:vendor/model-a"]);
    }

    #[test]
    fn flsh_config_is_found_where_flsh_looks() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let none = |_: &str| None;
        let flashmind_home =
            |name: &str| (name == "FLASHMIND_HOME").then(|| OsString::from("/srv/flashmind"));
        assert_eq!(
            flsh_config_dir(&flashmind_home, Some(&home)),
            Some(PathBuf::from("/srv/flashmind"))
        );
        let relative = |name: &str| (name == "FLASHMIND_HOME").then(|| OsString::from("rel"));
        assert_eq!(flsh_config_dir(&relative, Some(&home)), None);
        if !cfg!(windows) {
            assert_eq!(
                flsh_config_dir(&none, Some(&home)),
                Some(home.join(".config/flashmind"))
            );
            let xdg = |name: &str| (name == "XDG_CONFIG_HOME").then(|| OsString::from("/xdg"));
            assert_eq!(
                flsh_config_dir(&xdg, Some(&home)),
                Some(PathBuf::from("/xdg/flashmind"))
            );
        }
        std::fs::create_dir_all(home.join(".flashmind")).unwrap();
        assert_eq!(
            flsh_config_dir(&none, Some(&home)),
            Some(home.join(".flashmind"))
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn flsh_reads_the_config_in_the_home_it_is_set_to() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "model = \"set:home\"\n").unwrap();
        let kind = AgentKind {
            name: "flsh".into(),
            program: "false".into(),
            args: Vec::new(),
            env: [("FLASHMIND_HOME".into(), dir.path().display().to_string())].into(),
        };
        assert_eq!(flsh(&kind).await.current.as_deref(), Some("set:home"));
    }

    #[test]
    fn flsh_names_its_default_model_in_its_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(
            &config,
            "model = \"openai:gpt-test\"\n[providers.openai]\napi_key = \"k\"\n",
        )
        .unwrap();
        assert_eq!(
            flsh_default_model(&config).as_deref(),
            Some("openai:gpt-test")
        );
        std::fs::write(&config, "[providers.openai]\n").unwrap();
        assert_eq!(flsh_default_model(&config), None);
        std::fs::write(&config, "model = [").unwrap();
        assert_eq!(flsh_default_model(&config), None);
        assert_eq!(flsh_default_model(&dir.path().join("missing.toml")), None);
    }

    /// An adapter that answers `session/new` with models and stays up.
    #[cfg(unix)]
    const MODELS_ADAPTER: &str = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"sonnet","name":"Sonnet"},{"modelId":"opus","name":"Opus"}],"currentModelId":"sonnet"}}'
cat > /dev/null
"#;

    #[cfg(unix)]
    fn adapter(name: &str, script: &str) -> AgentKind {
        AgentKind {
            name: name.into(),
            program: "sh".into(),
            args: vec!["-c".into(), script.into(), "fake-adapter".into()],
            env: Default::default(),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_acp_agent_lists_the_models_its_session_offers() {
        let listing = find(adapter("claude", MODELS_ADAPTER)).await;
        assert_eq!(listing.pick, ModelPick::Listed);
        assert_eq!(listing.current.as_deref(), Some("sonnet"));
        let names: Vec<&str> = listing.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Sonnet", "Opus"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_agent_that_offers_no_models_has_none_to_pick() {
        let plain = r#"
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
read -r line
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line
reply '{"sessionId":"s"}'
cat > /dev/null
"#;
        assert_eq!(find(adapter("pi", plain)).await, ModelList::default());
        // Gemini CLI takes any name on its command line.
        let gemini = find(adapter("gemini", plain)).await;
        assert_eq!(gemini.pick, ModelPick::Named);
        // An agent that fails to start has none either.
        let failed = find(adapter("codex", "exit 1")).await;
        assert_eq!(failed.pick, ModelPick::Fixed);
    }
}
