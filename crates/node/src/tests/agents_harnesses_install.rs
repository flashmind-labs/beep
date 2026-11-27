use super::*;
use std::sync::Mutex;

#[test]
fn hermes_uses_its_own_updater_without_an_npm_package() {
    let cli = Path::new("/srv/tools/hermes");
    assert_eq!(
        plan(
            &Target {
                agent: "hermes",
                cli: Some(cli),
                npm_adapter: None
            },
            &npm_none()
        ),
        [Step::new(cli.display().to_string(), &["update"])]
    );
    assert!(
        plan(
            &Target {
                agent: "hermes",
                cli: None,
                npm_adapter: None
            },
            &npm_none()
        )
        .is_empty()
    );
}

#[test]
fn antigravity_installs_with_its_script_and_then_updates_itself() {
    let missing = Target {
        agent: "agy",
        cli: None,
        npm_adapter: None,
    };
    let line = plan(&missing, &npm_none())
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        line.ends_with(if cfg!(windows) {
            "\"irm https://antigravity.google/cli/install.ps1 | iex\""
        } else {
            "\"curl -fsSL https://antigravity.google/cli/install.sh | bash\""
        }),
        "{line}"
    );

    let installed = Target {
        cli: Some(Path::new("/srv/tools/agy")),
        ..missing
    };
    assert!(plan(&installed, &npm_none()).is_empty());
}

#[test]
fn a_missing_claude_installs_with_npm_then_its_own_installer() {
    let missing = Target {
        agent: "claude",
        cli: None,
        npm_adapter: None,
    };
    let installer = own_installer("claude").expect("Claude Code has its own installer");
    let without_npm = Npm {
        found: false,
        global: None,
    };

    assert_eq!(
        plan(&missing, &without_npm),
        std::slice::from_ref(&installer)
    );
    assert_eq!(
        plan(&missing, &npm_none()),
        [
            Step::new(
                "npm",
                &["install", "-g", "@anthropic-ai/claude-code@latest"]
            ),
            installer.clone()
        ]
    );
    let line = installer.to_string();
    assert!(
        line.ends_with(if cfg!(windows) {
            "\"irm https://claude.ai/install.ps1 | iex\""
        } else {
            "\"curl -fsSL https://claude.ai/install.sh | bash\""
        }),
        "{line}"
    );
}

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
    let prefix = paths::canonicalize(prefix.path()).unwrap();
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
    let global = NpmGlobal::from(prefix.path());
    let cli = file(&global.bin, "pi");
    let npm = Npm {
        found: true,
        global: Some(global.clone()),
    };
    // Without the package there, it is not npm's.
    assert_eq!(method("pi", &cli, &npm), None);
    std::fs::create_dir_all(global.modules.join("@earendil-works/pi-coding-agent")).unwrap();
    assert_eq!(method("pi", &cli, &npm), Some(Method::Npm));
}

#[cfg(unix)]
#[test]
fn no_step_runs_with_sudo() {
    let prefix = tempfile::tempdir().unwrap();
    let real = file(prefix.path(), "Cellar/opencode/1.3.13/bin/opencode");
    let cli = prefix.path().join("bin/opencode");
    link(&real, &cli);

    for agent in ["claude", "codex", "opencode", "pi", "flsh"] {
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
            output: "==> Fetching\nError: Refusing to load formula from an untrusted tap.\n".into(),
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
        details.starts_with("$ /srv/brew/bin/brew upgrade opencode\n==> Fetching\nError: Refusing"),
        "{details}"
    );
    assert!(
        details.contains("\n\n$ /srv/brew/bin/opencode upgrade\nError: Unexpected"),
        "{details}"
    );
}

#[cfg(unix)]
#[test]
fn a_standalone_codex_updates_itself() {
    let home = tempfile::tempdir().unwrap();
    let real = file(home.path(), ".codex/packages/standalone/current/bin/codex");
    let cli = home.path().join(".local/bin/codex");
    link(&real, &cli);
    assert_eq!(method("codex", &cli, &npm_none()), Some(Method::Own));
    let target = Target {
        agent: "codex",
        cli: Some(&cli),
        npm_adapter: None,
    };
    assert_eq!(
        plan(&target, &npm_none()),
        [Step::new(cli.display().to_string(), &["update"])]
    );
}

#[cfg(unix)]
#[test]
fn brew_updates_the_formula_from_its_installed_tap() {
    let prefix = tempfile::tempdir().unwrap();
    let real = file(prefix.path(), "Cellar/opencode/1.0.0/bin/opencode");
    let receipt = prefix
        .path()
        .join("Cellar/opencode/1.0.0/INSTALL_RECEIPT.json");
    std::fs::write(receipt, r#"{"source":{"tap":"vendor/tools"}}"#).unwrap();
    let cli = prefix.path().join("bin/opencode");
    link(&real, &cli);
    let steps = plan(
        &Target {
            agent: "opencode",
            cli: Some(&cli),
            npm_adapter: None,
        },
        &npm_none(),
    );
    assert_eq!(steps[0].args, ["upgrade", "vendor/tools/opencode"]);
}

#[tokio::test]
async fn a_brew_trust_failure_trusts_only_the_installed_formula_and_retries() {
    let upgrade = Step::new("/srv/brew/bin/brew", &["upgrade", "vendor/tools/opencode"]);
    let trust = Step::new(
        "/srv/brew/bin/brew",
        &["trust", "--formula", "vendor/tools/opencode"],
    );
    let refusal = Ran { ok: false, output:
        "Error: Refusing to load formula vendor/tools/opencode from untrusted tap vendor/tools. Run brew trust vendor/tools to trust it.".into() };
    let success = Ran {
        ok: true,
        output: String::new(),
    };
    let fake = Fake::new(vec![refusal.clone(), success.clone(), success.clone()]);
    run(
        &fake,
        "opencode",
        std::slice::from_ref(&upgrade),
        Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(
        *fake.ran.lock().unwrap(),
        [upgrade.clone(), trust.clone(), upgrade.clone()]
    );

    let fake = Fake::new(vec![refusal.clone(), success, refusal.clone()]);
    let error = run(
        &fake,
        "opencode",
        std::slice::from_ref(&upgrade),
        Duration::from_secs(60),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("brew upgrade vendor/tools/opencode failed:"),
        "{error}"
    );
    assert_eq!(
        *fake.ran.lock().unwrap(),
        [upgrade.clone(), trust.clone(), upgrade.clone()]
    );

    let fake = Fake::new(vec![
        refusal,
        Ran {
            ok: false,
            output: "Error: trust denied".into(),
        },
    ]);
    let error = run(
        &fake,
        "opencode",
        &[upgrade.clone(), Step::new("opencode", &["upgrade"])],
        Duration::from_secs(60),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("brew trust --formula vendor/tools/opencode failed: Error: trust denied"),
        "{error}"
    );
    assert_eq!(*fake.ran.lock().unwrap(), [upgrade, trust]);
}

#[test]
fn trust_recovery_ignores_unrelated_formulae_and_other_programs() {
    let refusal = "Error: Refusing to load formula other/tap/package from untrusted tap other/tap.";
    assert!(
        formula_trust(
            &Step::new("brew", &["upgrade", "vendor/tools/opencode"]),
            refusal
        )
        .is_none()
    );
    assert!(formula_trust(&Step::new("brew", &["upgrade", "opencode"]), refusal).is_none());
    assert!(
        formula_trust(
            &Step::new("opencode", &["upgrade", "other/tap/package"]),
            refusal
        )
        .is_none()
    );
}
