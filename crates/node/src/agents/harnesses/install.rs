//! How each harness was installed, and updating it with that installer:
//! Homebrew for a formula or cask, npm for a global npm package, else the
//! harness's own updater. When that fails and another way applies, it is
//! tried once. Nothing runs with sudo.

use crate::provision::spawn_env::{self, Scope};
use anyhow::{Result, bail};
use beep_proto::node::harness::{clean_output, failure_line, harness};
use std::{
    fmt,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::time::timeout;

/// The longest the line saying why an update failed gets.
const SUMMARY_CHARS: usize = 300;
/// The most of each command's output kept for the details.
const DETAIL_CHARS: usize = 4000;

/// How a harness's CLI was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Method {
    /// A Homebrew formula, or a cask when `cask`, under `prefix`.
    Brew {
        prefix: PathBuf,
        name: String,
        cask: bool,
    },
    /// A global npm package.
    Npm,
    /// Its own installer: it updates itself.
    Own,
}

/// npm's global folders, from `npm prefix -g`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NpmGlobal {
    /// Where npm links the commands of global packages.
    bin: PathBuf,
    /// Where global packages are.
    modules: PathBuf,
}

impl NpmGlobal {
    pub(super) fn from_prefix(prefix: &Path) -> Self {
        if cfg!(windows) {
            Self {
                bin: prefix.to_path_buf(),
                modules: prefix.join("node_modules"),
            }
        } else {
            Self {
                bin: prefix.join("bin"),
                modules: prefix.join("lib").join("node_modules"),
            }
        }
    }

    /// Whether `cli` is a command npm linked for `package`.
    fn installed(&self, cli: &Path, package: &str) -> bool {
        cli.parent() == Some(self.bin.as_path()) && self.modules.join(package).is_dir()
    }
}

/// What the node knows of npm.
#[derive(Debug, Clone, Default)]
pub(super) struct Npm {
    /// The `npm` command is found.
    pub(super) found: bool,
    pub(super) global: Option<NpmGlobal>,
}

/// The arguments that make `agent` update itself, when it can.
fn own_updater(agent: &str) -> Option<&'static [&'static str]> {
    match agent {
        "claude" => Some(&["update"]),
        "opencode" => Some(&["upgrade"]),
        _ => None,
    }
}

/// How `agent`'s CLI at `cli` was installed, when the node can tell and
/// knows how to update it.
pub(super) fn method(agent: &str, cli: &Path, npm: &Npm) -> Option<Method> {
    let real = std::fs::canonicalize(cli).unwrap_or_else(|_| cli.to_path_buf());
    if let Some(brew) = brew_keg(&real) {
        return Some(brew);
    }
    let package = harness(agent).and_then(|known| known.npm);
    if let Some(package) = package {
        let linked = npm
            .global
            .as_ref()
            .is_some_and(|global| global.installed(cli, package));
        if in_node_modules(&real) || linked {
            return Some(Method::Npm);
        }
    }
    own_updater(agent).map(|_| Method::Own)
}

/// The formula or cask `real` (a CLI with its links followed) belongs to,
/// when it is in Homebrew's `Cellar` or `Caskroom`.
fn brew_keg(real: &Path) -> Option<Method> {
    let parts: Vec<Component<'_>> = real.components().collect();
    let at = parts
        .iter()
        .position(|part| matches!(part.as_os_str().to_str(), Some("Cellar" | "Caskroom")))?;
    let name = parts.get(at + 1)?.as_os_str().to_str()?.to_owned();
    let cask = parts[at].as_os_str() == "Caskroom";
    let prefix: PathBuf = parts[..at].iter().collect();
    Some(Method::Brew { prefix, name, cask })
}

/// Whether `real` is inside a `node_modules` folder.
fn in_node_modules(real: &Path) -> bool {
    real.components()
        .any(|part| part.as_os_str() == "node_modules")
}

/// One command an update runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) program: String,
    pub(super) args: Vec<String>,
}

impl Step {
    fn new(program: impl Into<String>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }

    /// The command with only the program's file name, for one line.
    fn short(&self) -> String {
        let name = Path::new(&self.program).file_name().map_or_else(
            || self.program.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        std::iter::once(name)
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// What a harness is, for planning its update.
pub(super) struct Target<'a> {
    pub(super) agent: &'a str,
    /// Its CLI, when it is installed.
    pub(super) cli: Option<&'a Path>,
    /// Its ACP adapter's npm package, when npm installed that too.
    pub(super) npm_adapter: Option<&'static str>,
}

/// The commands that install `target` when it is missing, or update it,
/// in the order to try them: the installer it came from, then its own
/// updater when that is another way. Empty when the node knows none.
pub(super) fn plan(target: &Target<'_>, npm: &Npm) -> Vec<Step> {
    let package = harness(target.agent).and_then(|known| known.npm);
    let Some(cli) = target.cli else {
        return package
            .filter(|_| npm.found)
            .map(|package| vec![npm_install(&[package])])
            .unwrap_or_default();
    };
    let own = own_updater(target.agent).map(|args| Step::new(cli.display().to_string(), args));
    let first = match method(target.agent, cli, npm) {
        Some(Method::Brew { prefix, name, cask }) => {
            let brew = prefix.join("bin").join("brew").display().to_string();
            let args: &[&str] = if cask {
                &["upgrade", "--cask", &name]
            } else {
                &["upgrade", &name]
            };
            Some(Step::new(brew, args))
        }
        Some(Method::Npm) if npm.found => package.map(|package| {
            let mut packages = vec![package];
            packages.extend(target.npm_adapter);
            npm_install(&packages)
        }),
        Some(Method::Npm | Method::Own) | None => None,
    };
    first.into_iter().chain(own).collect()
}

fn npm_install(packages: &[&str]) -> Step {
    let mut step = Step::new("npm", &["install", "-g"]);
    step.args
        .extend(packages.iter().map(|package| format!("{package}@latest")));
    step
}

/// How one command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ran {
    pub(super) ok: bool,
    /// What it printed, standard output then standard error.
    pub(super) output: String,
}

/// Runs the commands of an update.
pub(super) trait Runner {
    async fn run(&self, step: &Step, limit: Duration) -> Ran;
}

/// Runs them as processes, as the node's probes run.
pub(super) struct Processes;

impl Runner for Processes {
    async fn run(&self, step: &Step, limit: Duration) -> Ran {
        let mut process = crate::process::command(&step.program);
        spawn_env::apply(&mut process, Scope::Probe);
        let run = process
            .args(&step.args)
            .env("NO_COLOR", "1")
            .env("HOMEBREW_NO_ENV_HINTS", "1")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output();
        match timeout(limit, run).await {
            Err(_) => Ran {
                ok: false,
                output: format!("took longer than {} minutes", limit.as_secs().div_ceil(60)),
            },
            Ok(Err(e)) => Ran {
                ok: false,
                output: format!("could not run {}: {e}", step.program),
            },
            Ok(Ok(out)) => {
                let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
                output.push_str(&String::from_utf8_lossy(&out.stderr));
                Ran {
                    ok: out.status.success(),
                    output,
                }
            }
        }
    }
}

/// A command that failed, with its output cleaned.
struct Failed<'a> {
    step: &'a Step,
    output: String,
}

/// Runs `steps` in order until one succeeds, all within `budget`. The
/// error has one line saying why the last failed and which were tried,
/// then a blank line and each one's output.
pub(super) async fn run(
    runner: &impl Runner,
    agent: &str,
    steps: &[Step],
    budget: Duration,
) -> Result<()> {
    if steps.is_empty() {
        bail!(
            "this node knows no way to install or update {agent}; install npm, or update it by hand"
        );
    }
    let deadline = Instant::now() + budget;
    let mut failed: Vec<Failed<'_>> = Vec::new();
    for step in steps {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        log::info!("updating {agent}: {step}");
        let ran = runner.run(step, left).await;
        if ran.ok {
            return Ok(());
        }
        log::warn!("updating {agent}: {step} failed");
        failed.push(Failed {
            step,
            output: clean_output(&ran.output),
        });
    }
    bail!("{}", failure_message(&failed))
}

fn failure_message(failed: &[Failed<'_>]) -> String {
    let Some(last) = failed.last() else {
        return "the update ran out of time".to_owned();
    };
    let why = failure_line(&last.output, SUMMARY_CHARS);
    let mut line = if why.is_empty() {
        format!("{} failed with no output", last.step.short())
    } else {
        format!("{} failed: {why}", last.step.short())
    };
    if failed.len() > 1 {
        let tried: Vec<String> = failed.iter().map(|f| f.step.short()).collect();
        line.push_str(&format!(" (tried {})", tried.join(", then ")));
    }
    let details: Vec<String> = failed
        .iter()
        .map(|f| {
            let output = if f.output.is_empty() {
                "(no output)".to_owned()
            } else {
                last_chars(&f.output, DETAIL_CHARS)
            };
            format!("$ {}\n{output}", f.step)
        })
        .collect();
    format!("{line}\n\n{}", details.join("\n\n"))
}

/// The last `max` characters of `text`.
fn last_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().skip(count - max).collect();
    format!("…{kept}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn npm_none() -> Npm {
        Npm {
            found: true,
            global: None,
        }
    }

    /// A file at `dir/rel`, with its folders.
    fn file(dir: &Path, rel: &str) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        path
    }

    #[cfg(unix)]
    fn link(target: &Path, at: &Path) {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, at).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_brew_formula_updates_with_brew_then_its_own_updater() {
        let prefix = tempfile::tempdir().unwrap();
        let real = file(prefix.path(), "Cellar/opencode/1.3.13/bin/opencode");
        let cli = prefix.path().join("bin/opencode");
        link(&real, &cli);
        let prefix = std::fs::canonicalize(prefix.path()).unwrap();
        assert_eq!(
            method("opencode", &cli, &npm_none()),
            Some(Method::Brew {
                prefix: prefix.clone(),
                name: "opencode".into(),
                cask: false,
            })
        );
        let target = Target {
            agent: "opencode",
            cli: Some(&cli),
            npm_adapter: None,
        };
        let steps = plan(&target, &npm_none());
        let brew = prefix.join("bin/brew").display().to_string();
        assert_eq!(
            steps,
            [
                Step::new(brew, &["upgrade", "opencode"]),
                Step::new(cli.display().to_string(), &["upgrade"]),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_npm_package_under_the_brew_prefix_updates_with_npm() {
        // Node from Homebrew links global packages into Homebrew's bin.
        let prefix = tempfile::tempdir().unwrap();
        let real = file(prefix.path(), "lib/node_modules/@openai/codex/bin/codex.js");
        let cli = prefix.path().join("bin/codex");
        link(&real, &cli);
        assert_eq!(method("codex", &cli, &npm_none()), Some(Method::Npm));
        let target = Target {
            agent: "codex",
            cli: Some(&cli),
            npm_adapter: Some("@zed-industries/codex-acp"),
        };
        assert_eq!(
            plan(&target, &npm_none()),
            [Step::new(
                "npm",
                &[
                    "install",
                    "-g",
                    "@openai/codex@latest",
                    "@zed-industries/codex-acp@latest"
                ]
            )]
        );
        // Claude Code from npm falls back to `claude update`.
        let real = file(
            prefix.path(),
            "lib/node_modules/@anthropic-ai/claude-code/cli.js",
        );
        let claude = prefix.path().join("bin/claude");
        link(&real, &claude);
        let target = Target {
            agent: "claude",
            cli: Some(&claude),
            npm_adapter: None,
        };
        let steps = plan(&target, &npm_none());
        assert_eq!(steps[0].program, "npm");
        assert_eq!(
            steps[1],
            Step::new(claude.display().to_string(), &["update"])
        );
    }

    #[test]
    fn a_command_in_npms_bin_folder_is_from_npm() {
        // npm on Windows writes a shim into the prefix, not a link.
        let prefix = tempfile::tempdir().unwrap();
        let global = NpmGlobal::from_prefix(prefix.path());
        let cli = file(&global.bin, "gemini");
        let npm = Npm {
            found: true,
            global: Some(global.clone()),
        };
        // Without the package there, it is not npm's.
        assert_eq!(method("gemini", &cli, &npm), None);
        std::fs::create_dir_all(global.modules.join("@google/gemini-cli")).unwrap();
        assert_eq!(method("gemini", &cli, &npm), Some(Method::Npm));
    }

    #[cfg(unix)]
    #[test]
    fn no_step_runs_with_sudo() {
        let prefix = tempfile::tempdir().unwrap();
        let real = file(prefix.path(), "Cellar/opencode/1.3.13/bin/opencode");
        let cli = prefix.path().join("bin/opencode");
        link(&real, &cli);
        for agent in ["claude", "codex", "opencode", "pi", "gemini", "flsh"] {
            for cli in [None, Some(cli.as_path())] {
                let target = Target {
                    agent,
                    cli,
                    npm_adapter: None,
                };
                for step in plan(&target, &npm_none()) {
                    assert!(!step.program.contains("sudo"), "{step}");
                    assert!(!step.args.iter().any(|arg| arg == "sudo"), "{step}");
                }
            }
        }
    }

    /// Answers each step with the next of `answers`, keeping what ran.
    struct Fake {
        answers: Mutex<Vec<Ran>>,
        ran: Mutex<Vec<Step>>,
    }

    impl Fake {
        fn new(answers: Vec<Ran>) -> Self {
            Self {
                answers: Mutex::new(answers),
                ran: Mutex::new(Vec::new()),
            }
        }
    }

    impl Runner for Fake {
        async fn run(&self, step: &Step, _limit: Duration) -> Ran {
            self.ran.lock().unwrap().push(step.clone());
            self.answers.lock().unwrap().remove(0)
        }
    }

    fn steps() -> Vec<Step> {
        vec![
            Step::new("/srv/brew/bin/brew", &["upgrade", "opencode"]),
            Step::new("/srv/brew/bin/opencode", &["upgrade"]),
        ]
    }

    const OPENCODE_FAILED: &str = "\u{1b}[48;5;238m\u{2580}\u{2580}\u{1b}[0m\r\n\u{1b}[91m\u{1b}[1mError: \u{1b}[0mUnexpected error, check log file for more details\nSyntaxError: JSON Parse error: Unexpected EOF\n";

    #[tokio::test]
    async fn a_failed_installer_falls_back_to_the_next_once() {
        let fake = Fake::new(vec![
            Ran {
                ok: false,
                output: "Error: Refusing to load formula from an untrusted tap.".into(),
            },
            Ran {
                ok: true,
                output: String::new(),
            },
        ]);
        run(&fake, "opencode", &steps(), Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(*fake.ran.lock().unwrap(), steps());
        // The first that works ends it.
        let fake = Fake::new(vec![Ran {
            ok: true,
            output: String::new(),
        }]);
        run(&fake, "opencode", &steps(), Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(fake.ran.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn when_all_fail_the_error_names_each_and_reads_cleanly() {
        let fake = Fake::new(vec![
            Ran {
                ok: false,
                output: "==> Fetching\nError: Refusing to load formula from an untrusted tap.\n"
                    .into(),
            },
            Ran {
                ok: false,
                output: OPENCODE_FAILED.into(),
            },
        ]);
        let error = run(&fake, "opencode", &steps(), Duration::from_secs(60))
            .await
            .unwrap_err()
            .to_string();
        let (line, details) = error.split_once("\n\n").unwrap();
        assert_eq!(
            line,
            "opencode upgrade failed: Error: Unexpected error, check log file for more details \
             SyntaxError: JSON Parse error: Unexpected EOF \
             (tried brew upgrade opencode, then opencode upgrade)"
        );
        assert!(
            !error.contains('\u{1b}') && !error.contains('\u{2580}'),
            "{error}"
        );
        assert!(
            details.starts_with(
                "$ /srv/brew/bin/brew upgrade opencode\n==> Fetching\nError: Refusing"
            ),
            "{details}"
        );
        assert!(
            details.contains("\n\n$ /srv/brew/bin/opencode upgrade\nError: Unexpected"),
            "{details}"
        );
    }
}
