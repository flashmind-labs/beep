//! The harnesses page of a node: what it finds of each coding agent it
//! knows (version, newest version, where it is, its sign-in), updating
//! one, and the user's settings for each, applied when one starts.

use super::{Adapter, AgentKind, KNOWN, flsh_config, models::flsh_config_dir, unix_now};
use crate::{
    AsyncMutex,
    env::{claude_logged_in, opencode_logged_in, output},
    provision::spawn_env::{self, Scope},
};
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::harness::{
    AgentSettings, HARNESSES, HarnessConfig, HarnessInfo, HarnessReport, harness, parse_version,
};
use beep_proto::{AgentSlug, FlshSettings};
use std::{
    collections::HashMap,
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tokio::time::timeout;

/// How long the newest versions found are kept before being looked up
/// again.
const LATEST_FOR: Duration = Duration::from_secs(6 * 60 * 60);
/// How long one `npm view` may take.
const NPM_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a sign-in status command may take.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an install or update may take.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The most of an update's output kept to say why it failed.
const MAX_FAILURE: usize = 600;

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
    fn lock(&self) -> MutexGuard<'_, Saved> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

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
        *self.lock() = Saved {
            harnesses,
            flsh: settings.flsh,
        };
    }

    /// Writes the user's flsh settings into the config `kind` (as
    /// `configured` gives it) reads, before it starts. Nothing for another
    /// agent, or while the user has none. An error when they cannot be
    /// written: flsh must not start without a setting such as zero data
    /// retention.
    pub(super) async fn prepare(&self, kind: &AgentKind) -> Result<()> {
        if AgentSlug::FLSH != kind.name.as_str() {
            return Ok(());
        }
        let Some(flsh) = self.lock().flsh.clone() else {
            return Ok(());
        };
        // flsh runs with the kind's variables on top of the node's own.
        let env = |name: &str| match kind.env.get(name) {
            Some(value) => Some(OsString::from(value)),
            None => std::env::var_os(name),
        };
        let dir = flsh_config_dir(&env, dirs::home_dir().as_deref())
            .context("flsh's config folder is not known")?;
        tokio::task::spawn_blocking(move || flsh_config::apply(&dir, &flsh))
            .await
            .context("writing flsh's settings stopped")?
            .context("Beep's flsh settings could not be written to flsh's config")
    }

    /// How `kind` starts with the user's settings; an error when they
    /// turned it off.
    pub(super) fn configured(&self, kind: &AgentKind) -> Result<AgentKind> {
        match self.lock().harnesses.get(&kind.name) {
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
        self.lock().harnesses.get(agent).cloned()
    }
}

/// `kind` with `config` applied: its program swapped, its home variable
/// and env vars set, and its arguments added after the node's own.
fn configure(kind: &AgentKind, config: &HarnessConfig) -> AgentKind {
    let mut kind = kind.clone();
    if let Some(program) = config.program.as_deref().filter(|p| !p.trim().is_empty()) {
        kind.program = program.trim().to_owned();
    }
    if let (Some(home), Some(known)) = (
        config.home.as_deref().filter(|h| !h.trim().is_empty()),
        harness(&kind.name),
    ) {
        kind.env
            .insert(known.home_var.to_owned(), expand_home(home.trim()));
    }
    kind.args.extend(config.args.iter().cloned());
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
    /// One update at a time: two installs of one package would race.
    updating: AsyncMutex<()>,
}

#[derive(Default)]
struct LatestFound {
    versions: HashMap<&'static str, String>,
    at: Option<Instant>,
    /// Unix seconds of `at`.
    checked_at: u64,
}

impl Latest {
    fn lock(&self) -> MutexGuard<'_, LatestFound> {
        self.found.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The newest version of each of `packages`, looked up again when
    /// `refresh` says so or the last look is old. Gives when that was.
    async fn versions(
        &self,
        packages: &[&'static str],
        refresh: bool,
    ) -> (HashMap<&'static str, String>, u64) {
        {
            let found = self.lock();
            let fresh = found.at.is_some_and(|at| at.elapsed() < LATEST_FOR);
            let all = packages.iter().all(|p| found.versions.contains_key(p));
            if !refresh && fresh && all {
                return (found.versions.clone(), found.checked_at);
            }
        }
        if !found_program("npm") {
            let found = self.lock();
            return (found.versions.clone(), found.checked_at);
        }
        let looked = futures::future::join_all(packages.iter().map(|package| async move {
            let version = timeout(NPM_TIMEOUT, output("npm", &["view", package, "version"]))
                .await
                .ok()
                .flatten()
                .as_deref()
                .and_then(parse_version);
            (*package, version)
        }))
        .await;
        let mut found = self.lock();
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

/// Whether `program` is found as the processes the node starts find it.
fn found_program(program: &str) -> bool {
    find(program).is_some()
}

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

/// Whether `path`, symlinks followed, is inside a `node_modules` folder:
/// installed with npm.
fn from_npm(path: &std::path::Path) -> bool {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    real.components()
        .any(|part| part.as_os_str() == "node_modules")
}

/// The command that installs `found` when it is missing and updates it
/// after, as program and arguments: npm for a CLI npm installed (with its
/// ACP adapter when npm installed that too), else the CLI's own updater.
fn install_command(found: &Found, npm: bool) -> Option<Vec<String>> {
    let package = harness(&found.agent).and_then(|known| known.npm);
    let Some(cli) = &found.cli else {
        return package
            .filter(|_| npm)
            .map(|package| npm_install(&[package]));
    };
    if let Some(package) = package.filter(|_| npm && from_npm(cli)) {
        let mut packages = vec![package];
        let adapter = adapter_package(&found.agent);
        let adapter_bin = found
            .kind
            .as_ref()
            .filter(|kind| kind.program != "npx")
            .and_then(|kind| find(&kind.program));
        if let (Some(adapter), Some(bin)) = (adapter, adapter_bin)
            && from_npm(&bin)
        {
            packages.push(adapter);
        }
        return Some(npm_install(&packages));
    }
    let own: &[&str] = match found.agent.as_str() {
        "claude" => &["update"],
        "opencode" => &["upgrade"],
        _ => return None,
    };
    let mut command = vec![cli.display().to_string()];
    command.extend(own.iter().map(|arg| (*arg).to_owned()));
    Some(command)
}

fn npm_install(packages: &[&str]) -> Vec<String> {
    let mut command = vec!["npm".to_owned(), "install".to_owned(), "-g".to_owned()];
    command.extend(packages.iter().map(|package| format!("{package}@latest")));
    command
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
    let npm = found_program("npm");
    let packages: Vec<&'static str> = found
        .iter()
        .filter(|found| found.cli.is_some())
        .filter_map(|found| harness(&found.agent)?.npm)
        .collect();
    let ((newest, checked_at), harnesses) = tokio::join!(
        latest.versions(&packages, refresh),
        futures::future::join_all(found.iter().map(|found| info(
            found,
            settings.get(&found.agent),
            npm
        )))
    );
    let harnesses = harnesses
        .into_iter()
        .map(|mut info| {
            info.latest = harness(&info.agent)
                .and_then(|known| known.npm)
                .and_then(|package| newest.get(package).cloned());
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
async fn info(found: &Found, config: Option<HarnessConfig>, npm: bool) -> HarnessInfo {
    let install = install_command(found, npm).map(|command| command.join(" "));
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
        _ => return SignIn::default(),
    };
    let mut command = crate::process::command(cli);
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
        "opencode" => SignIn {
            signed_in: Some(ok && opencode_logged_in(text) != Some(false)),
            ..SignIn::default()
        },
        _ => SignIn::default(),
    }
}

/// Installs or updates `agent` with the command its report names.
pub(super) async fn update(kinds: &[AgentKind], latest: &Latest, agent: &str) -> Result<()> {
    let _one = latest.updating.lock().await;
    let found = found(kinds)
        .into_iter()
        .find(|found| found.agent == agent)
        .with_context(|| format!("this node knows no agent {agent}"))?;
    let Some(command) = install_command(&found, found_program("npm")) else {
        bail!(
            "this node knows no way to install or update {agent}; install npm, or update it by hand"
        );
    };
    let Some((program, args)) = command.split_first() else {
        bail!("the update command for {agent} is empty");
    };
    log::info!("updating {agent}: {}", command.join(" "));
    let mut process = crate::process::command(program);
    spawn_env::apply(&mut process, Scope::Probe);
    let run = process
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = timeout(UPDATE_TIMEOUT, run)
        .await
        .with_context(|| format!("{} took too long", command.join(" ")))?
        .with_context(|| format!("could not run {program}"))?;
    if !out.status.success() {
        let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
        if text.trim().is_empty() {
            text = String::from_utf8_lossy(&out.stdout).into_owned();
        }
        let tail: String = text
            .trim()
            .chars()
            .rev()
            .take(MAX_FAILURE)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        bail!("{} failed: {tail}", command.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{SecretText, node::EnvSecret};
    use std::collections::BTreeMap;

    fn kind(name: &str) -> AgentKind {
        AgentKind {
            name: name.into(),
            program: "codex-acp".into(),
            args: vec!["--acp".into()],
            env: BTreeMap::from([("INITIAL_AGENT_MODE".into(), "agent-full-access".into())]),
        }
    }

    #[tokio::test]
    async fn flsh_settings_go_to_the_config_its_home_names() {
        let home = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let settings = Settings::default();
        settings.set(AgentSettings {
            harnesses: vec![
                HarnessConfig {
                    home: Some(home.path().display().to_string()),
                    ..HarnessConfig::new("flsh")
                },
                HarnessConfig {
                    home: Some(other.path().display().to_string()),
                    ..HarnessConfig::new("pi")
                },
            ],
            flsh: Some(FlshSettings {
                zdr: Some(true),
                ..FlshSettings::default()
            }),
        });
        let flsh = settings.configured(&kind("flsh")).unwrap();
        settings.prepare(&flsh).await.unwrap();
        let written = std::fs::read_to_string(home.path().join("config.toml")).unwrap();
        assert!(written.contains("zdr = true"), "{written}");
        // Another agent's start writes nothing.
        let pi = settings.configured(&kind("pi")).unwrap();
        settings.prepare(&pi).await.unwrap();
        assert!(!other.path().join("config.toml").exists());
        // Without flsh settings, flsh's config is left as it is.
        let untouched = tempfile::tempdir().unwrap();
        settings.set(AgentSettings {
            harnesses: vec![HarnessConfig {
                home: Some(untouched.path().display().to_string()),
                ..HarnessConfig::new("flsh")
            }],
            flsh: None,
        });
        let flsh = settings.configured(&kind("flsh")).unwrap();
        settings.prepare(&flsh).await.unwrap();
        assert!(!untouched.path().join("config.toml").exists());
    }

    #[test]
    fn settings_change_the_launch_command() {
        let config = HarnessConfig {
            program: Some(" /opt/codex/bin/codex-acp ".into()),
            home: Some("/srv/codex-home".into()),
            args: vec!["--profile".into(), "work".into()],
            env: vec![EnvSecret {
                name: "HTTPS_PROXY".into(),
                value: SecretText::new("http://proxy:3128"),
            }],
            ..HarnessConfig::new("codex")
        };
        let settings = Settings::default();
        settings.set(AgentSettings {
            harnesses: vec![config],
            ..AgentSettings::default()
        });
        let started = settings.configured(&kind("codex")).unwrap();
        assert_eq!(started.program, "/opt/codex/bin/codex-acp");
        assert_eq!(started.args, ["--acp", "--profile", "work"]);
        assert_eq!(started.env["CODEX_HOME"], "/srv/codex-home");
        assert_eq!(started.env["HTTPS_PROXY"], "http://proxy:3128");
        assert_eq!(started.env["INITIAL_AGENT_MODE"], "agent-full-access");
        // Another agent runs as found.
        assert_eq!(settings.configured(&kind("pi")).unwrap(), kind("pi"));
    }

    #[test]
    fn a_harness_turned_off_does_not_start() {
        let settings = Settings::default();
        settings.set(AgentSettings {
            harnesses: vec![HarnessConfig {
                enabled: false,
                ..HarnessConfig::new("codex")
            }],
            ..AgentSettings::default()
        });
        let refused = settings.configured(&kind("codex")).unwrap_err();
        assert!(refused.to_string().contains("turned off"), "{refused}");
        // New settings replace the old ones in full.
        settings.set(AgentSettings::default());
        assert!(settings.configured(&kind("codex")).is_ok());
    }

    #[test]
    fn a_home_folder_with_a_tilde_is_expanded() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            expand_home("~/.codex-work"),
            format!("{}/.codex-work", home.display())
        );
        assert_eq!(expand_home("~other/x"), "~other/x");
        assert_eq!(expand_home("/srv/x"), "/srv/x");
    }

    #[test]
    fn sign_ins_are_read_from_each_status() {
        let claude = r#"{"loggedIn":true,"email":"you@example.com","subscriptionType":"max"}"#;
        let read = read_sign_in("claude", true, claude);
        assert_eq!(read.signed_in, Some(true));
        assert_eq!(read.account.as_deref(), Some("you@example.com"));
        assert_eq!(read.plan.as_deref(), Some("max"));
        let out = read_sign_in("claude", true, r#"{"loggedIn":false}"#);
        assert_eq!(out.signed_in, Some(false));
        assert_eq!(out.account, None);
        assert_eq!(
            read_sign_in("codex", true, "Logged in using ChatGPT").signed_in,
            Some(true)
        );
        assert_eq!(
            read_sign_in("codex", false, "Not logged in").signed_in,
            Some(false)
        );
        assert_eq!(read_sign_in("pi", true, "").signed_in, None);
    }

    #[test]
    fn a_missing_npm_cli_installs_with_npm() {
        let missing = Found {
            agent: "gemini".into(),
            cli: None,
            kind: None,
        };
        assert_eq!(
            install_command(&missing, true).unwrap().join(" "),
            "npm install -g @google/gemini-cli@latest"
        );
        assert_eq!(install_command(&missing, false), None);
        let flsh = Found {
            agent: "flsh".into(),
            cli: None,
            kind: None,
        };
        assert_eq!(install_command(&flsh, true), None);
    }

    #[cfg(unix)]
    #[test]
    fn an_npm_cli_updates_with_npm_and_others_with_their_own_command() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("lib/node_modules/@openai/codex/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let codex = bin.join("codex.js");
        std::fs::write(&codex, "").unwrap();
        let link = dir.path().join("codex");
        std::os::unix::fs::symlink(&codex, &link).unwrap();
        let npm = Found {
            agent: "codex".into(),
            cli: Some(link),
            kind: None,
        };
        assert_eq!(
            install_command(&npm, true).unwrap().join(" "),
            "npm install -g @openai/codex@latest"
        );
        let claude = Found {
            agent: "claude".into(),
            cli: Some(dir.path().join("claude")),
            kind: None,
        };
        let own = install_command(&claude, true).unwrap();
        assert_eq!(own[1..], ["update"]);
        let pi = Found {
            agent: "pi".into(),
            cli: Some(dir.path().join("pi")),
            kind: None,
        };
        assert_eq!(install_command(&pi, true), None);
    }
}
