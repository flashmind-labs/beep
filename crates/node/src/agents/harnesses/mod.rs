//! The harnesses page of a node: what it finds of each coding agent it
//! knows (version, newest version, where it is, its sign-in), updating
//! one, and the user's settings for each, applied when one starts.

mod install;

use super::{
    Adapter, AgentKind, KNOWN, flsh_config,
    kinds::{direct, in_wsl},
    models::{flsh_dir_of, pi_dir_of},
    unix_now,
};
use crate::{
    AsyncMutex, lock,
    machine::env::{claude_logged_in, opencode_logged_in, output},
    process::spawn_env::{self, Scope},
};
use anyhow::{Context, Result, ensure};
use beep_proto::node::harness::{
    AgentSettings, HARNESSES, HarnessConfig, HarnessDriver, HarnessInfo, HarnessReport, harness,
    parse_version,
};
use beep_proto::{AgentSlug, FlshSettings};
use reqwest::Client;
use std::{
    collections::HashMap,
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::{sync::OnceCell, time::timeout};

/// How long the newest versions found are kept before being looked up
/// again.
const LATEST_FOR: Duration = Duration::from_secs(55 * 60);
/// How long a registry lookup may take.
const NPM_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a sign-in status command may take.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an install or update may take, every way tried together.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The user's settings for the harnesses, as the server last sent them.
#[derive(Default)]
pub(super) struct Settings(Mutex<Saved>);

#[derive(Default)]
struct Saved {
    /// Each harness's, by agent.
    harnesses: HashMap<String, HarnessConfig>,
    /// flsh's own, written to its config before it starts.
    flsh: Option<FlshSettings>,
}

impl Settings {
    /// Takes `settings` in place of the ones before.
    pub(super) fn set(&self, settings: AgentSettings) {
        let harnesses: HashMap<String, HarnessConfig> = settings
            .harnesses
            .into_iter()
            .map(|config| (config.agent.clone(), config))
            .collect();
        log::info!(
            "harness settings: {} harnesses, {} off, flsh settings {}",
            harnesses.len(),
            harnesses.values().filter(|config| !config.enabled).count(),
            if settings.flsh.is_some() {
                "set"
            } else {
                "unset"
            },
        );
        *lock(&self.0) = Saved {
            harnesses,
            flsh: settings.flsh,
        };
    }

    /// The settings now, as `set` took them.
    pub(super) fn current(&self) -> AgentSettings {
        let saved = lock(&self.0);
        let mut harnesses: Vec<HarnessConfig> = saved.harnesses.values().cloned().collect();
        harnesses.sort_by(|a, b| a.agent.cmp(&b.agent));
        AgentSettings {
            harnesses,
            flsh: saved.flsh.clone(),
        }
    }

    /// Writes the user's flsh settings into the config `kind` (as
    /// `configured` gives it) reads, before it starts: all of them for
    /// flsh, the routing for pi. flsh gets its default stream timeout on
    /// hosts without saved settings.
    pub(super) async fn prepare(&self, kind: &AgentKind) -> Result<()> {
        let name = &kind.name;
        let config_dir = |dir: Option<PathBuf>| {
            dir.with_context(|| format!("{name}'s config folder is not known"))
        };
        let settings = lock(&self.0).flsh.clone();
        let write = if AgentSlug::FLSH == name.as_str() {
            let dir = config_dir(flsh_dir_of(kind))?;
            tokio::task::spawn_blocking(move || flsh_config::apply(&dir, settings.as_ref()))
        } else if AgentSlug::PI == name.as_str()
            && let Some(settings) = settings
        {
            let dir = config_dir(pi_dir_of(kind))?;
            tokio::task::spawn_blocking(move || flsh_config::pi::apply(&dir, &settings))
        } else {
            return Ok(());
        };

        write
            .await
            .context("writing the agent's settings stopped")?
            .with_context(|| format!("Beep's settings could not be written to {name}'s config"))
    }

    /// How `kind` starts with the user's settings; an error when they
    /// turned it off.
    pub(super) fn configured(&self, kind: &AgentKind) -> Result<AgentKind> {
        match lock(&self.0).harnesses.get(&kind.name) {
            Some(config) => {
                ensure!(
                    config.enabled,
                    "{} is turned off on this machine",
                    kind.name
                );
                Ok(configure(kind, config))
            }
            None => Ok(kind.clone()),
        }
    }

    fn get(&self, agent: &str) -> Option<HarnessConfig> {
        lock(&self.0).harnesses.get(agent).cloned()
    }
}

/// `kind` with `config` applied: its program swapped, its home variable
/// and env vars set, and its arguments added after the node's own. With the
/// `Direct` driver the node's own bridge runs the CLI; the program and
/// arguments set for its adapter do not apply. An agent found without its
/// adapter stays on the bridge unless an adapter program is set.
fn configure(kind: &AgentKind, config: &HarnessConfig) -> AgentKind {
    let direct = match config.driver {
        HarnessDriver::Direct => direct(kind),
        _ if in_wsl(kind) => Some(kind.clone()),
        HarnessDriver::Acp | HarnessDriver::Other => None,
    };
    let adapter = direct.is_none();
    let mut kind = direct.unwrap_or_else(|| kind.clone());

    if adapter {
        let program = config.program.as_deref().map(str::trim);
        if let Some(program) = program
            && !program.is_empty()
        {
            kind.program = program.to_owned();
            kind.bridge = None;
        }

        if kind.bridge.is_none() {
            kind.args.extend(config.args.iter().cloned());
        }
    }

    // A home set for Windows is no path inside WSL.
    if let (Some(home), Some(known), false) = (
        config.home.as_deref().filter(|h| !h.trim().is_empty()),
        harness(&kind.name),
        in_wsl(&kind),
    ) {
        kind.env
            .insert(known.home_var.to_owned(), expand_home(home.trim()));
    }
    kind.default_model.clone_from(&config.default_model);
    kind.compact = config.compact;
    kind.compact_models.clone_from(&config.compact_models);

    for var in &config.env {
        kind.env
            .insert(var.name.clone(), var.value.expose().to_owned());
    }

    kind
}

/// `path` with a leading `~` for the home folder.
fn expand_home(path: &str) -> String {
    let rest = match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => rest,
        _ => return path.to_owned(),
    };

    match dirs::home_dir() {
        Some(home) => format!("{}{rest}", home.display()),
        None => path.to_owned(),
    }
}

/// The newest versions published, by npm package, kept a while.
#[derive(Default)]
pub(super) struct Latest {
    found: Mutex<LatestFound>,
    /// flsh publishes releases outside npm.
    flsh: beep_mesh::machine::releases::ToolReleases,
    /// One update at a time: two installs of one package would race.
    updating: AsyncMutex<()>,
    /// npm's global folders, looked up once.
    npm_global: OnceCell<Option<install::NpmGlobal>>,
}

#[derive(Default)]
struct LatestFound {
    versions: HashMap<&'static str, String>,
    at: Option<Instant>,
    /// Unix seconds of `at`.
    checked_at: u64,
}

impl Latest {
    /// What the node knows of npm: whether it is there, and its global
    /// folders.
    async fn npm(&self) -> install::Npm {
        if find("npm").is_none() {
            return install::Npm::default();
        }

        let global = self
            .npm_global
            .get_or_init(|| async {
                let prefix = output("npm", &["prefix", "-g"]).await?;
                let prefix = prefix
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())?;
                Some(install::NpmGlobal::from(std::path::Path::new(prefix)))
            })
            .await
            .clone();
        install::Npm {
            found: true,
            global,
        }
    }

    /// The newest version of each of `packages`, looked up again when
    /// `refresh` says so or the last look is old. Gives when that was.
    async fn versions(
        &self,
        packages: &[&'static str],
        refresh: bool,
    ) -> (HashMap<&'static str, String>, u64) {
        {
            let found = lock(&self.found);
            let fresh = found.at.is_some_and(|at| at.elapsed() < LATEST_FOR);
            let all = packages.iter().all(|p| found.versions.contains_key(p));

            if !refresh && fresh && all {
                return (found.versions.clone(), found.checked_at);
            }
        }
        let has_npm = find("npm").is_some();
        let http = beep_proto::https::client()
            .timeout(NPM_TIMEOUT)
            .build()
            .ok();
        let looked = futures::future::join_all(packages.iter().map(|package| {
            let http = http.as_ref();
            async move {
                let version = if has_npm {
                    timeout(NPM_TIMEOUT, output("npm", &["view", package, "version"]))
                        .await
                        .ok()
                        .flatten()
                        .as_deref()
                        .and_then(parse_version)
                } else if let Some(http) = http {
                    registry_version(http, package).await
                } else {
                    None
                };
                (*package, version)
            }
        }))
        .await;
        let mut found = lock(&self.found);

        for (package, version) in looked {
            if let Some(version) = version {
                found.versions.insert(package, version);
            }
        }

        found.at = Some(Instant::now());
        found.checked_at = unix_now();
        (found.versions.clone(), found.checked_at)
    }
}

/// The public npm registry also lists releases when npm is not installed.
async fn registry_version(http: &Client, package: &str) -> Option<String> {
    let package = package.replace('/', "%2F");
    let response = http
        .get(format!("https://registry.npmjs.org/{package}/latest"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let body = response.text().await.ok()?;
    let metadata: serde_json::Value = serde_json::from_str(&body).ok()?;
    parse_version(metadata.get("version")?.as_str()?)
}

/// Where `program` is found, as the processes the node starts find it.
pub(super) fn find(program: &str) -> Option<PathBuf> {
    let path: Option<OsString> = spawn_env::search_path();
    which::which_in(program, path.as_ref(), "/").ok()
}

/// What the node knows of how `agent` is installed and runs.
struct Found {
    agent: String,
    cli: Option<PathBuf>,
    /// The node's own way to start it, when it can.
    kind: Option<AgentKind>,
}

/// The harnesses Beep knows, then any other `kinds` holds.
fn found(kinds: &[AgentKind]) -> Vec<Found> {
    let known = HARNESSES.iter().map(|harness| harness.agent);
    let extra = kinds
        .iter()
        .map(|kind| kind.name.as_str())
        .filter(|name| harness(name).is_none());
    known
        .chain(extra)
        .map(|agent| Found {
            agent: agent.to_owned(),
            cli: find(agent),
            kind: kinds.iter().find(|kind| kind.name == agent).cloned(),
        })
        .collect()
}

/// The npm package of `agent`'s ACP adapter, when it has one.
fn adapter_package(agent: &str) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|known| known.name == agent)
        .and_then(|known| match known.adapter {
            Adapter::Npm { package, .. } => Some(package),
            Adapter::Flags(_) => None,
        })
}

/// The commands that install `found` when it is missing and update it
/// after, in the order to try them.
fn update_steps(found: &Found, npm: &install::Npm) -> Vec<install::Step> {
    // Its ACP adapter goes along when npm installed that too.
    let npm_adapter = adapter_package(&found.agent).filter(|_| {
        found
            .kind
            .as_ref()
            .filter(|kind| kind.program != "npx")
            .and_then(|kind| find(&kind.program))
            .is_some_and(|bin| {
                install::method(&found.agent, &bin, npm) == Some(install::Method::Npm)
            })
    });
    let target = install::Target {
        agent: &found.agent,
        cli: found.cli.as_deref(),
        npm_adapter,
    };
    install::plan(&target, npm)
}

/// Refreshes the versions of installed harnesses without probing sign-ins.
pub(super) async fn refresh_versions(kinds: &[AgentKind], latest: &Latest) {
    latest.versions(&npm_packages(&found(kinds)), false).await;
}

/// The npm packages of the installed harnesses that have one.
fn npm_packages(found: &[Found]) -> Vec<&'static str> {
    found
        .iter()
        .filter(|found| found.cli.is_some())
        .filter_map(|found| harness(&found.agent)?.npm)
        .collect()
}

/// What the node finds of every harness, with the newest versions looked
/// up again when `refresh` says so.
pub(super) async fn report(
    kinds: &[AgentKind],
    settings: &Settings,
    latest: &Latest,
    refresh: bool,
) -> HarnessReport {
    let found = found(kinds);
    let npm = latest.npm().await;
    let packages = npm_packages(&found);
    let flsh_installed = found
        .iter()
        .any(|item| AgentSlug::FLSH == item.agent.as_str() && item.cli.is_some());
    let ((newest, checked_at), flsh, harnesses) = tokio::join!(
        latest.versions(&packages, refresh),
        async {
            if flsh_installed {
                latest
                    .flsh
                    .latest(beep_proto::node::tool_update::Tool::Flsh)
                    .await
            } else {
                None
            }
        },
        futures::future::join_all(found.iter().map(|found| info(
            found,
            settings.get(&found.agent),
            &npm
        )))
    );
    let harnesses = harnesses
        .into_iter()
        .map(|mut info| {
            info.latest = if AgentSlug::FLSH == info.agent.as_str() {
                flsh.clone()
            } else {
                harness(&info.agent)
                    .and_then(|known| known.npm)
                    .and_then(|package| newest.get(package).cloned())
            };
            info
        })
        .collect();
    HarnessReport {
        harnesses,
        checked_at,
    }
}

/// One harness as the node finds it, its sign-in read with `config`'s
/// home and env vars.
async fn info(found: &Found, config: Option<HarnessConfig>, npm: &install::Npm) -> HarnessInfo {
    let install = update_steps(found, npm).first().map(ToString::to_string);
    let Some(cli) = &found.cli else {
        return HarnessInfo {
            agent: found.agent.clone(),
            installed: false,
            runnable: found.kind.is_some(),
            install,
            ..HarnessInfo::default()
        };
    };
    let version = output(cli, &["--version"])
        .await
        .as_deref()
        .and_then(parse_version);
    let program = found.kind.as_ref().map(|kind| {
        find(&kind.program).map_or_else(|| kind.program.clone(), |p| p.display().to_string())
    });
    let base = found.kind.clone().unwrap_or_else(|| AgentKind {
        name: found.agent.clone(),
        program: cli.display().to_string(),
        args: Vec::new(),
        env: Default::default(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    });
    let kind = match &config {
        Some(config) => configure(&base, config),
        None => base,
    };
    let sign_in = sign_in(&found.agent, cli, &kind).await;
    HarnessInfo {
        agent: found.agent.clone(),
        installed: true,
        runnable: found.kind.is_some(),
        version,
        latest: None,
        cli_path: Some(cli.display().to_string()),
        program,
        signed_in: sign_in.signed_in,
        account: sign_in.account,
        plan: sign_in.plan,
        install,
    }
}

#[derive(Default)]
struct SignIn {
    signed_in: Option<bool>,
    account: Option<String>,
    plan: Option<String>,
}

/// How `agent` signs in, read with its status command where it has one.
async fn sign_in(agent: &str, cli: &std::path::Path, kind: &AgentKind) -> SignIn {
    let args: &[&str] = match agent {
        "claude" => &["auth", "status"],
        "codex" => &["login", "status"],
        "opencode" => &["auth", "list"],
        "agy" => &["models"],
        _ => return SignIn::default(),
    };
    let mut command = crate::process::probe(cli);
    spawn_env::apply(&mut command, Scope::Agent(None));
    let run = command
        .args(args)
        .envs(&kind.env)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let Ok(Ok(out)) = timeout(STATUS_TIMEOUT, run).await else {
        return SignIn::default();
    };
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    read_sign_in(agent, out.status.success(), &text)
}

/// What a status command's exit and output say of the sign-in.
fn read_sign_in(agent: &str, ok: bool, text: &str) -> SignIn {
    match agent {
        "claude" => {
            let signed_in = ok && claude_logged_in(text) != Some(false);
            let json: Option<serde_json::Value> = serde_json::from_str(text).ok();
            let field = |name: &str| {
                json.as_ref()
                    .and_then(|json| json.get(name)?.as_str().map(str::to_owned))
            };
            SignIn {
                signed_in: Some(signed_in),
                account: field("email").filter(|_| signed_in),
                plan: field("subscriptionType").filter(|_| signed_in),
            }
        }
        "codex" => SignIn {
            signed_in: Some(ok && !text.contains("Not logged in")),
            ..SignIn::default()
        },
        "agy" => SignIn {
            signed_in: Some(ok),
            ..SignIn::default()
        },
        "opencode" => SignIn {
            signed_in: Some(ok && opencode_logged_in(text) != Some(false)),
            ..SignIn::default()
        },
        _ => SignIn::default(),
    }
}

/// Installs or updates `agent` with the installer it came from, and with
/// its own updater when that fails.
pub(super) async fn update(kinds: &[AgentKind], latest: &Latest, agent: &str) -> Result<()> {
    let _one = latest.updating.lock().await;
    let found = found(kinds)
        .into_iter()
        .find(|found| found.agent == agent)
        .with_context(|| format!("this node knows no agent {agent}"))?;
    let steps = update_steps(&found, &latest.npm().await);
    install::run(&install::Processes, agent, &steps, UPDATE_TIMEOUT).await
}

#[cfg(test)]
#[path = "../../tests/agents_harnesses.rs"]
mod tests;
