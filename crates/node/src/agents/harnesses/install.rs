//! How each harness was installed (`method`) and updating it that way (`plan`,
//! `run`): Homebrew, a global npm package, else the harness's own updater.
//! Nothing runs with sudo.

use crate::fs::paths;
use crate::process::spawn_env::{self, Scope};
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

impl From<&Path> for NpmGlobal {
    fn from(prefix: &Path) -> Self {
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
}

impl NpmGlobal {
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

/// The arguments that make `agent`'s CLI at `cli` update itself, when it can.
fn own_updater(agent: &str, cli: &Path) -> Option<&'static [&'static str]> {
    match agent {
        "claude" | "hermes" => Some(&["update"]),
        // Only Codex's standalone install, under `~/.codex/packages`, updates
        // itself; npm and Homebrew update theirs.
        "codex" if standalone_codex(cli) => Some(&["update"]),
        "opencode" => Some(&["upgrade"]),
        _ => None,
    }
}

/// Whether `cli`, with its links followed, is Codex's standalone install.
fn standalone_codex(cli: &Path) -> bool {
    let real = paths::canonicalize(cli).unwrap_or_else(|_| cli.to_path_buf());
    real.components()
        .collect::<Vec<_>>()
        .windows(3)
        .any(|parts| {
            parts[0].as_os_str() == ".codex"
                && parts[1].as_os_str() == "packages"
                && parts[2].as_os_str() == "standalone"
        })
}

/// How `agent`'s CLI at `cli` was installed, when the node can tell and
/// knows how to update it.
pub(super) fn method(agent: &str, cli: &Path, npm: &Npm) -> Option<Method> {
    let real = paths::canonicalize(cli).unwrap_or_else(|_| cli.to_path_buf());

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

    own_updater(agent, cli).map(|_| Method::Own)
}

/// The formula or cask `real` (a CLI with its links followed) belongs to,
/// when it is in Homebrew's `Cellar` or `Caskroom`.
fn brew_keg(real: &Path) -> Option<Method> {
    let parts: Vec<Component<'_>> = real.components().collect();
    let at = parts
        .iter()
        .position(|part| matches!(part.as_os_str().to_str(), Some("Cellar" | "Caskroom")))?;
    let mut name = parts.get(at + 1)?.as_os_str().to_str()?.to_owned();
    let cask = parts[at].as_os_str() == "Caskroom";
    let prefix: PathBuf = parts[..at].iter().collect();

    if !cask {
        // Read metadata without evaluating code from an untrusted tap.
        let keg: PathBuf = parts.get(..at + 3)?.iter().collect();
        if let Ok(receipt) = std::fs::read(keg.join("INSTALL_RECEIPT.json"))
            && let Ok(receipt) = serde_json::from_slice::<serde_json::Value>(&receipt)
            && let Some(tap) = receipt["source"]["tap"].as_str()
            && tap.split('/').count() == 2
            && tap.split('/').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            })
        {
            name = format!("{tap}/{name}");
        }
    }

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

        // An argument with spaces is quoted, so the line can be pasted into a shell.
        for arg in &self.args {
            if arg.contains(' ') {
                write!(f, " \"{arg}\"")?;
            } else {
                write!(f, " {arg}")?;
            }
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
/// updater when that is another way. A missing one tries npm, then its
/// own installer. Empty when the node knows none.
pub(super) fn plan(target: &Target<'_>, npm: &Npm) -> Vec<Step> {
    let package = harness(target.agent).and_then(|known| known.npm);
    let Some(cli) = target.cli else {
        return package
            .filter(|_| npm.found)
            .map(|package| npm_install(&[package]))
            .into_iter()
            .chain(own_installer(target.agent))
            .collect();
    };
    let own = own_updater(target.agent, cli).map(|args| Step::new(cli.display().to_string(), args));
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

/// A harness's own installer, which needs no Node.js: Claude Code's
/// native install, the way it installs on Windows without npm, and
/// Antigravity's only one. Antigravity then updates itself.
fn own_installer(agent: &str) -> Option<Step> {
    match agent {
        "claude" if cfg!(windows) => Some(Step::new(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", "irm https://claude.ai/install.ps1 | iex"],
        )),
        "claude" => Some(Step::new("sh", &["-c", "curl -fsSL https://claude.ai/install.sh | bash"])),
        "agy" if cfg!(windows) => Some(Step::new(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", "irm https://antigravity.google/cli/install.ps1 | iex"],
        )),
        "agy" => Some(Step::new("sh", &["-c", "curl -fsSL https://antigravity.google/cli/install.sh | bash"])),
        _ => None,
    }
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
struct Failed {
    step: Step,
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
            "this node knows no way to install or update {agent}; install Node.js, or update it by hand"
        );
    }

    let deadline = Instant::now() + budget;
    let mut failed: Vec<Failed> = Vec::new();

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
        let output = clean_output(&ran.output);
        let trust = formula_trust(step, &output);
        failed.push(Failed {
            step: step.clone(),
            output,
        });

        if let Some(trust) = trust {
            // Updating the installed formula authorizes only that formula,
            // never every package or command in its tap.
            for command in [&trust, step] {
                let left = deadline.saturating_duration_since(Instant::now());

                if left.is_zero() {
                    bail!("{}", failure_message(&failed));
                }

                let ran = runner.run(command, left).await;

                if !ran.ok {
                    failed.push(Failed {
                        step: command.clone(),
                        output: clean_output(&ran.output),
                    });
                    bail!("{}", failure_message(&failed));
                }
            }

            return Ok(());
        }
    }

    bail!("{}", failure_message(&failed))
}

/// Trust only the installed formula named by the update plan, and only when
/// Homebrew refuses that same formula. Never execute its suggested command.
fn formula_trust(step: &Step, output: &str) -> Option<Step> {
    if Path::new(&step.program).file_name()? != "brew" {
        return None;
    }

    let [action, name] = step.args.as_slice() else {
        return None;
    };

    if action != "upgrade" || name.split('/').count() != 3 {
        return None;
    }

    let refusal = format!("Error: Refusing to load formula {name} from untrusted tap ");
    output
        .lines()
        .any(|line| line.starts_with(&refusal))
        .then(|| Step::new(&step.program, &["trust", "--formula", name]))
}

fn failure_message(failed: &[Failed]) -> String {
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
#[path = "../../tests/agents_harnesses_install.rs"]
mod tests;
