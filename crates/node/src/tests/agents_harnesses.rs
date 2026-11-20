use super::*;
use crate::agents::{Bridge, compaction::Policy};
use beep_proto::{
    SecretText,
    node::{EnvSecret, harness::AutoCompact},
};
use std::collections::BTreeMap;

fn kind(name: &str) -> AgentKind {
    AgentKind {
        name: name.into(),
        program: "codex-acp".into(),
        args: vec!["--acp".into()],
        env: BTreeMap::from([("INITIAL_AGENT_MODE".into(), "agent-full-access".into())]),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
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
        compact: Some(AutoCompact {
            enabled: true,
            percent: Some(65),
            tokens: None,
        }),
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
    assert_eq!(Policy::from(started.compact), Policy::At(65));
    // Another agent runs as found.
    assert_eq!(settings.configured(&kind("pi")).unwrap(), kind("pi"));
}

#[test]
fn the_direct_driver_runs_claude_code_and_codex_on_the_nodes_own_bridges() {
    let config = HarnessConfig {
        program: Some("/opt/claude/bin/claude".into()),
        home: Some("/srv/claude-home".into()),
        args: vec!["--effort".into(), "high".into()],
        driver: HarnessDriver::Direct,
        ..HarnessConfig::new("claude")
    };
    let settings = Settings::default();
    settings.set(AgentSettings {
        harnesses: vec![
            config.clone(),
            HarnessConfig { agent: "codex".into(), ..config.clone() },
            HarnessConfig { agent: "gemini".into(), ..config },
        ],
        ..AgentSettings::default()
    });

    let started = settings.configured(&kind("claude")).unwrap();
    assert_eq!(started.bridge, Some(Bridge::Claude { wsl: None, home: None }));
    // The program and arguments set for the adapter do not apply: the
    // bridge starts the CLI found here.
    assert_ne!(started.program, "/opt/claude/bin/claude");
    assert!(started.args.is_empty());
    assert_eq!(started.env["CLAUDE_CONFIG_DIR"], "/srv/claude-home");
    // Codex keeps the variables the node gives it, such as its mode.
    let codex = settings.configured(&kind("codex")).unwrap();
    assert_eq!(codex.bridge, Some(Bridge::Codex { wsl: None }));
    assert!(codex.args.is_empty());
    assert_eq!(codex.env["INITIAL_AGENT_MODE"], "agent-full-access");
    // Only Claude Code and Codex have a direct driver.
    let gemini = settings.configured(&kind("gemini")).unwrap();
    assert_eq!((gemini.program.as_str(), gemini.bridge), ("/opt/claude/bin/claude", None));
}

#[test]
fn an_agent_found_without_its_adapter_stays_on_the_bridge_until_an_adapter_is_set() {
    let bridged = AgentKind {
        program: "/usr/local/bin/codex".into(),
        bridge: Some(Bridge::Codex { wsl: None }),
        ..kind("codex")
    };
    let config = HarnessConfig {
        args: vec!["--flag".into()],
        driver: HarnessDriver::Acp,
        ..HarnessConfig::new("codex")
    };
    let settings = Settings::default();
    settings.set(AgentSettings { harnesses: vec![config.clone()], ..AgentSettings::default() });

    // The adapter's arguments do not go to the CLI.
    assert_eq!(settings.configured(&bridged).unwrap(), bridged);

    let adapter = HarnessConfig { program: Some("/opt/codex-acp".into()), ..config };
    settings.set(AgentSettings { harnesses: vec![adapter], ..AgentSettings::default() });
    let started = settings.configured(&bridged).unwrap();
    assert_eq!((started.program.as_str(), started.bridge), ("/opt/codex-acp", None));
    assert_eq!(started.args.last().map(String::as_str), Some("--flag"));
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

#[tokio::test]
async fn version_check_configures_tls_without_a_global_provider() {
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());
    let latest = Latest::default();
    let (versions, checked_at) = latest.versions(&[], true).await;
    assert!(versions.is_empty());
    assert!(checked_at > 0);
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());
}
#[test]
fn configuring_a_harness_passes_its_default_model_to_sessions() {
    let mut config = HarnessConfig::new("codex");
    config.default_model = Some("model-large".into());
    assert_eq!(
        configure(&kind("codex"), &config).default_model,
        config.default_model
    );
}

#[cfg(unix)]
#[tokio::test]
async fn claude_sign_in_respects_signed_out_status() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("claude");
    tokio::fs::write(
        &cli,
        "#!/bin/sh\nprintf '%s\\n' '{\"loggedIn\":false,\"authMethod\":\"none\"}'\nexit 1\n",
    )
    .await
    .unwrap();
    tokio::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700))
        .await
        .unwrap();

    let read = sign_in("claude", &cli, &kind("claude")).await;

    assert_eq!(read.signed_in, Some(false));
    assert_eq!(read.account, None);
    assert_eq!(read.plan, None);
}

#[test]
fn the_hermes_assistant_is_no_harness() {
    let hermes = AgentKind::from(crate::agents::HermesLink {
        host: "hermes.local".into(),
        port: 9119,
        token: SecretText::new("placeholder"),
    });

    let found = found(&[kind("custom"), hermes]);

    assert!(found.iter().any(|found| found.agent == "custom"));
    assert!(found.iter().all(|found| found.agent != crate::agents::hermes_direct::NAME));
}
