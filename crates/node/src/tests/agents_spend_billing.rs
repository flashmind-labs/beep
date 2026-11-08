use super::*;

#[test]
fn claude_settings_tell_by_their_keys() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let user = dir.path().join("user");
    fs::create_dir_all(&user).expect("a folder");
    let files = AccountFiles {
        claude: Some(user.clone()),
        ..AccountFiles::default()
    };
    assert_eq!(
        Accounts::read(&files).resolve(Agent::Claude, Billing::Unknown, None, None),
        Billing::Unknown
    );
    fs::write(
        user.join("settings.json"),
        r#"{"env":{"ANTHROPIC_API_KEY":"example-value"}}"#,
    )
    .expect("settings");
    let mut accounts = Accounts::read(&files);
    assert_eq!(
        accounts.resolve(Agent::Claude, Billing::Unknown, None, None),
        Billing::Metered
    );
    let project = dir.path().join("project");
    fs::create_dir_all(project.join(".claude")).expect("a folder");
    fs::write(
        project.join(".claude/settings.local.json"),
        r#"{"forceLoginMethod":"claudeai"}"#,
    )
    .expect("settings");
    let cwd = project.to_str().expect("a path");
    assert_eq!(
        accounts.resolve(Agent::Claude, Billing::Unknown, None, Some(cwd)),
        Billing::Subscription
    );
    assert_eq!(
        accounts.resolve(Agent::Claude, Billing::Metered, None, Some(cwd)),
        Billing::Metered,
        "what the log said wins"
    );
}

#[test]
fn codex_reads_its_auth_mode_and_provider() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let auth = dir.path().join("auth.json");
    fs::write(
        &auth,
        r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"x"}}"#,
    )
    .expect("auth");
    let mut accounts = Accounts::read(&AccountFiles {
        codex: Some(auth.clone()),
        ..AccountFiles::default()
    });
    assert_eq!(
        accounts.resolve(Agent::Codex, Billing::Unknown, Some("openai"), None),
        Billing::Subscription
    );
    assert_eq!(
        accounts.resolve(Agent::Codex, Billing::Unknown, Some("openrouter"), None),
        Billing::Metered
    );
    fs::write(&auth, r#"{"auth_mode":"apikey"}"#).expect("auth");
    let mut accounts = Accounts::read(&AccountFiles {
        codex: Some(auth),
        ..AccountFiles::default()
    });
    assert_eq!(
        accounts.resolve(Agent::Codex, Billing::Unknown, None, None),
        Billing::Metered
    );
}

#[test]
fn providers_go_by_their_auth_type_then_their_name() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let auth = dir.path().join("auth.json");
    fs::write(
        &auth,
        r#"{"anthropic":{"type":"oauth","access":"x"},"openrouter":{"type":"api_key","key":"x"}}"#,
    )
    .expect("auth");
    let mut accounts = Accounts::read(&AccountFiles {
        pi: Some(auth),
        ..AccountFiles::default()
    });
    let pi = |accounts: &mut Accounts, provider| {
        accounts.resolve(Agent::Pi, Billing::Unknown, Some(provider), None)
    };
    assert_eq!(pi(&mut accounts, "anthropic"), Billing::Subscription);
    assert_eq!(pi(&mut accounts, "openrouter"), Billing::Metered);
    assert_eq!(pi(&mut accounts, "github-copilot"), Billing::Subscription);
    assert_eq!(pi(&mut accounts, "openai"), Billing::Metered);
    assert_eq!(
        accounts.resolve(Agent::Flsh, Billing::Unknown, None, None),
        Billing::Unknown
    );
    assert_eq!(
        flsh_provider("openrouter:example/model"),
        Some("openrouter")
    );
}
