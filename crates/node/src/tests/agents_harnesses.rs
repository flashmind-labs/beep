use super::*;
use crate::agents::compaction::Policy;
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
fn the_direct_driver_runs_claude_code_on_the_nodes_own_bridge() {
    let config = HarnessConfig {
        program: Some("/opt/claude/bin/claude".into()),
        home: Some("/srv/claude-home".into()),
        args: vec!["--effort".into(), "high".into()],
        driver: HarnessDriver::Direct,
        ..HarnessConfig::new("claude")
    };
    let settings = Settings::default();
    settings.set(AgentSettings {
        harnesses: vec![config.clone(), HarnessConfig { agent: "codex".into(), ..config }],
        ..AgentSettings::default()
    });

    let started = settings.configured(&kind("claude")).unwrap();
    let exe = std::env::current_exe().unwrap();
    assert_eq!(started.program, exe.display().to_string());
    // The program and arguments set for the adapter do not apply.
    assert_eq!(started.args[..2], ["claude-acp", "--cli"]);
    assert_ne!(started.args[2], "/opt/claude/bin/claude");
    assert_eq!(started.args[3..], ["--"]);
    assert_eq!(started.env["CLAUDE_CONFIG_DIR"], "/srv/claude-home");
    // Only Claude Code has a direct driver.
    assert_eq!(settings.configured(&kind("codex")).unwrap().program, "/opt/claude/bin/claude");
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
