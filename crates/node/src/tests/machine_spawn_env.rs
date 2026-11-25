use super::*;
use std::process::Stdio;

async fn seen(scope: Scope) -> String {
    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "printf '%s,%s' \"$BEEP_TEST_CREDENTIAL\" \"$GH_TOKEN\"",
        ])
        .env("BEEP_TEST_CREDENTIAL", "inherited")
        .env_remove("GH_TOKEN")
        .stdout(Stdio::piped());
    apply(&mut command, scope);
    let out = command.output().await.unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[tokio::test]
async fn each_process_gets_only_its_credentials() {
    let _env = TEST_LOCK.lock().await;
    set_vars(vec![
        EnvSecret {
            name: "BEEP_TEST_CREDENTIAL".into(),
            value: SecretText::new("from-beep"),
        },
        EnvSecret {
            name: GITHUB_ENV_VAR.into(),
            value: SecretText::new("gh-token"),
        },
    ]);
    let shell = seen(Scope::Shell).await;
    let agent = seen(Scope::Agent(None)).await;
    let probe = seen(Scope::Probe).await;
    set_vars(Vec::new());
    assert_eq!(shell, "from-beep,gh-token");
    assert_eq!(agent, "inherited,gh-token");
    assert_eq!(probe, "inherited,");
}

async fn agent_token(scope: Scope) -> String {
    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "printf '%s,%s' \"$CLAUDE_CODE_OAUTH_TOKEN\" \"$CODEX_API_KEY\"",
        ])
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .env_remove("CODEX_API_KEY")
        .stdout(Stdio::piped());
    apply(&mut command, scope);
    let out = command.output().await.unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[tokio::test]
async fn an_agent_gets_only_its_own_token() {
    let _env = TEST_LOCK.lock().await;
    set_vars(vec![
        EnvSecret {
            name: AgentAccount::Claude.env_var().into(),
            value: SecretText::new("claude-token"),
        },
        EnvSecret {
            name: AgentAccount::Codex.env_var().into(),
            value: SecretText::new("codex-key"),
        },
    ]);
    let claude = agent_token(Scope::Agent(Some(AgentAccount::Claude))).await;
    let codex = agent_token(Scope::Agent(Some(AgentAccount::Codex))).await;
    let other = agent_token(Scope::Agent(None)).await;
    let shell = agent_token(Scope::Shell).await;
    set_vars(Vec::new());
    assert_eq!(claude, "claude-token,");
    assert_eq!(codex, ",codex-key");
    assert_eq!(other, ",");
    assert_eq!(shell, "claude-token,codex-key");
}

#[tokio::test]
async fn a_named_account_replaces_the_default_token() {
    let _env = TEST_LOCK.lock().await;
    let work: AccountName = "Work".parse().unwrap();
    set_vars(vec![EnvSecret {
        name: AgentAccount::Claude.env_var().into(),
        value: SecretText::new("personal-token"),
    }]);
    set_accounts(vec![AgentLogin {
        agent: AgentAccount::Claude,
        name: work.clone(),
        env: EnvSecret {
            name: AgentAccount::Claude.env_var().into(),
            value: SecretText::new("work-token"),
        },
    }]);
    let default = agent_token(Scope::for_agent("claude", None)).await;
    let named = agent_token(Scope::for_agent("claude", Some(&work))).await;
    let held = has_account(AgentAccount::Claude, &work);
    let codex_held = has_account(AgentAccount::Codex, &work);
    set_vars(Vec::new());
    set_accounts(Vec::new());
    assert_eq!(default, "personal-token,");
    assert_eq!(named, "work-token,");
    assert!(held);
    assert!(!codex_held);
}
