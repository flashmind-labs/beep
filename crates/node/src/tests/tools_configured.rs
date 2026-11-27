use super::*;

use beep_proto::node::mcp_servers::{Connection, RemoteTransport, Settings, Stdio};

fn save(name: &str, settings: Settings, previous: Option<Box<EditableServer>>) -> Change {
    Change::Save {
        previous,
        server: Box::new(EditableServer {
            name: name.into(),
            settings,
        }),
    }
}

fn editable(server: Server) -> Result<Box<EditableServer>> {
    Ok(Box::new(EditableServer {
        name: server.name,
        settings: server.settings.context("missing settings")?,
    }))
}

fn stdio(command: &str) -> Settings {
    Settings {
        connection: Connection::Stdio(Stdio {
            command: command.into(),
            args: Vec::new(),
            env: Default::default(),
            cwd: None,
        }),
        enabled: true,
        codex: None,
        claude_oauth: None,
    }
}

#[test]
fn codex_reads_native_entries_and_copies_one_with_its_advanced_settings() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("source.toml");
    let target = dir.path().join("target.toml");
    fs::write(
        &source,
        r#"# source settings
model = "example-model"

[mcp_servers.docs]
url = "https://tools.example.test/mcp"
enabled = false
bearer_token_env_var = "DOCS_TOKEN"
startup_timeout_ms = 2500
enabled_tools = ["read"]

[mcp_servers.docs.tools.read]
approval_mode = "prompt"

[mcp_servers.local]
command = "local-mcp"
"#,
    )?;
    fs::write(
        &target,
        r#"# keep this comment
model = "other-model"

[mcp_servers.docs]
url = "https://old.example.test/mcp"

[mcp_servers.keep]
command = "keep-mcp"
"#,
    )?;

    let stored = Document::parse(Harness::Codex, &read(&source)?)?.servers(Harness::Codex)?;
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].name, "docs");
    let previous = editable(stored[0].clone())?;
    let mut settings = previous.settings.clone();
    assert!(!settings.enabled);
    let options = settings.codex.as_mut().context("missing Codex settings")?;
    assert_eq!(options.startup_timeout_sec, Some(2.5));
    assert_eq!(
        options.enabled_tools.as_deref(),
        Some(&["read".to_owned()][..])
    );
    options.startup_timeout_sec = Some(8.5);
    options.enabled_tools = Some(Vec::new());
    let Connection::Remote(remote) = &mut settings.connection else {
        panic!("expected HTTP")
    };
    assert_eq!(remote.bearer_token_env_var.as_deref(), Some("DOCS_TOKEN"));
    remote
        .headers
        .insert("X-Example".to_owned(), SecretText::new("example"));
    change(
        &source,
        Harness::Codex,
        save("docs", settings, Some(previous)),
    )?;

    let entry = Document::parse(Harness::Codex, &read(&source)?)?
        .native("docs")?
        .context("missing native entry")?;
    assert!(entry.config.expose().contains("[tools.read]"));
    assert!(matches!(
        change(&target, Harness::Codex, Change::Apply(entry))?,
        Reply::Applied
    ));
    let written = read(&target)?;
    assert!(written.contains("# keep this comment"));
    let config: toml::Value = toml::from_str(&written)?;
    assert_eq!(config["model"].as_str(), Some("other-model"));
    assert_eq!(
        config["mcp_servers"]["docs"]["enabled"].as_bool(),
        Some(false)
    );
    assert_eq!(
        config["mcp_servers"]["docs"]["http_headers"]["X-Example"].as_str(),
        Some("example")
    );
    assert_eq!(
        config["mcp_servers"]["docs"]["bearer_token_env_var"].as_str(),
        Some("DOCS_TOKEN")
    );
    assert_eq!(
        config["mcp_servers"]["docs"]["startup_timeout_sec"].as_float(),
        Some(8.5)
    );
    assert!(
        config["mcp_servers"]["docs"]
            .get("startup_timeout_ms")
            .is_none()
    );
    assert!(
        config["mcp_servers"]["docs"]["enabled_tools"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(
        config["mcp_servers"]["docs"]["tools"]["read"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert_eq!(
        config["mcp_servers"]["keep"]["command"].as_str(),
        Some("keep-mcp")
    );
    Ok(())
}

#[test]
fn claude_edits_only_the_user_entry_and_preserves_project_config() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join(".claude.json");
    let original = serde_json::json!({
        "projects": { "/srv/project": { "mcpServers": { "docs": { "command": "project" } } } },
        "mcpServers": { "docs": { "type": "sse", "url": "https://tools.example.test/sse",
            "oauth": { "clientId": "docs-client", "extraSetting": true } }, "other": { "command": "other-mcp" } },
        "somePreference": true,
    });
    fs::write(&path, original.to_string())?;
    let doc = Document::parse(Harness::Claude, &read(&path)?)?;
    let previous = editable(
        doc.servers(Harness::Claude)?
            .into_iter()
            .find(|server| server.name == "docs")
            .context("missing server")?,
    )?;
    let mut settings = previous.settings.clone();
    let oauth = settings
        .claude_oauth
        .as_mut()
        .context("missing Claude OAuth settings")?;
    assert_eq!(oauth.client_id.as_deref(), Some("docs-client"));
    oauth.scopes = Some("read".to_owned());
    let Connection::Remote(remote) = &mut settings.connection else {
        panic!("expected SSE")
    };
    assert_eq!(remote.transport, RemoteTransport::Sse);
    remote.transport = RemoteTransport::Http;
    remote.url = SecretText::new("https://tools.example.test/mcp");
    remote
        .headers
        .insert("X-Example".into(), SecretText::new("value"));
    let reply = change(
        &path,
        Harness::Claude,
        save("renamed", settings, Some(previous)),
    )?;
    assert!(
        matches!(reply, Reply::Servers(servers) if servers.iter().map(|server| server.name.as_str()).eq(["other", "renamed"]))
    );

    let config: Value = serde_json::from_str(&read(&path)?)?;
    assert_eq!(config["projects"], original["projects"]);
    assert_eq!(config["somePreference"], original["somePreference"]);
    assert_eq!(
        config["mcpServers"]["renamed"]["headers"]["X-Example"],
        "value"
    );
    assert_eq!(
        config["mcpServers"]["renamed"]["oauth"],
        serde_json::json!({
            "clientId": "docs-client", "extraSetting": true, "scopes": "read"
        })
    );
    change(&path, Harness::Claude, Change::Remove("renamed".into()))?;
    let config: Value = serde_json::from_str(&read(&path)?)?;
    assert_eq!(
        config["mcpServers"],
        serde_json::json!({"other":{"command":"other-mcp"}})
    );
    Ok(())
}

#[test]
fn invalid_fields_leave_the_original_file_intact() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    let original = "mcp_servers = { current = { command = 'local-mcp' } }\n";
    fs::write(&path, original)?;
    let previous = editable(
        Document::parse(Harness::Codex, original)?
            .servers(Harness::Codex)?
            .pop()
            .context("missing server")?,
    )?;
    assert!(
        change(
            &path,
            Harness::Codex,
            save("foo", stdio(""), Some(previous))
        )
        .is_err()
    );
    assert_eq!(read(&path)?, original);
    change(&path, Harness::Codex, save("new", stdio("new-mcp"), None))?;
    let config: toml::Value = toml::from_str(&read(&path)?)?;
    assert_eq!(
        config["mcp_servers"]["new"]["command"].as_str(),
        Some("new-mcp")
    );
    Ok(())
}

#[test]
fn repairs_invalid_command_and_preserves_harness_specific_options() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    let original = r#"[mcp_servers.tool]
command = ""
env_vars = ["SPECIAL_ENV"]
startup_timeout_sec = 25
args = ["one two"]
"#;
    fs::write(&path, original)?;
    let previous = editable(
        Document::parse(Harness::Codex, original)?
            .servers(Harness::Codex)?
            .pop()
            .context("missing server")?,
    )?;
    let mut settings = previous.settings.clone();
    let Connection::Stdio(stdio) = &mut settings.connection else {
        panic!("expected stdio")
    };
    stdio.command = "example-mcp".into();
    stdio
        .env
        .insert("EXAMPLE".into(), SecretText::new("example"));
    let request = Request::Save {
        harness: Harness::Codex,
        previous: Some(previous),
        server: Box::new(EditableServer {
            name: "tool".into(),
            settings,
        }),
    };
    let frame = rmp_serde::to_vec(&request)?;
    let Request::Save {
        harness,
        previous,
        server,
    } = beep_proto::lenient::from_slice(&frame)?
    else {
        panic!("expected save request")
    };
    change(&path, harness, Change::Save { previous, server })?;
    let config: toml::Value = toml::from_str(&read(&path)?)?;
    let tool = &config["mcp_servers"]["tool"];
    assert_eq!(tool["command"].as_str(), Some("example-mcp"));
    assert_eq!(tool["env"]["EXAMPLE"].as_str(), Some("example"));
    assert_eq!(tool["env_vars"][0].as_str(), Some("SPECIAL_ENV"));
    assert_eq!(tool["args"][0].as_str(), Some("one two"));
    assert_eq!(tool["startup_timeout_sec"].as_integer(), Some(25));
    Ok(())
}

#[test]
fn a_stale_edit_keeps_newer_fields_and_returns_the_current_servers() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    let original = "[mcp_servers.tool]\ncommand = 'first-mcp'\n";
    let previous = editable(
        Document::parse(Harness::Codex, original)?
            .servers(Harness::Codex)?
            .pop()
            .context("missing server")?,
    )?;
    let mut draft = previous.settings.clone();
    draft.enabled = false;
    let current = "[mcp_servers.tool]\ncommand = 'newer-mcp'\n";
    fs::write(&path, current)?;

    let Reply::Conflict(mut servers) =
        change(&path, Harness::Codex, save("tool", draft, Some(previous)))?
    else {
        bail!("expected a conflict");
    };
    assert_eq!(read(&path)?, current);

    let latest = editable(servers.pop().context("missing server")?)?;
    let mut draft = latest.settings.clone();
    draft.enabled = false;
    fs::write(
        &path,
        format!(
            "{current}env_vars = ['SPECIAL_ENV']\n\n[mcp_servers.neighbor]\ncommand = 'neighbor-mcp'\n"
        ),
    )?;
    assert!(matches!(
        change(&path, Harness::Codex, save("tool", draft, Some(latest)))?,
        Reply::Servers(_)
    ));
    let config: toml::Value = toml::from_str(&read(&path)?)?;
    let servers = &config["mcp_servers"];
    assert_eq!(servers["tool"]["command"].as_str(), Some("newer-mcp"));
    assert_eq!(servers["tool"]["enabled"].as_bool(), Some(false));
    assert_eq!(servers["tool"]["env_vars"][0].as_str(), Some("SPECIAL_ENV"));
    assert_eq!(
        servers["neighbor"]["command"].as_str(),
        Some("neighbor-mcp")
    );
    Ok(())
}

#[test]
fn antigravity_edits_its_own_fields_and_keeps_the_rest() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("mcp_config.json");
    // As `agy mcp add` writes them.
    let original = serde_json::json!({ "mcpServers": {
        "api": { "disabled": true, "headers": { "Authorization": "Bearer token" },
            "serverUrl": "https://tools.example.test/mcp", "extraSetting": true },
        "files": { "args": ["-y", "files-mcp"], "command": "npx", "disabled": false, "env": { "KEY": "value" } },
    } });
    fs::write(&path, original.to_string())?;
    let servers = Document::parse(Harness::Agy, &read(&path)?)?.servers(Harness::Agy)?;
    let [api, files] = servers.try_into().ok().context("two servers")?;
    let previous = editable(api)?;
    let mut settings = previous.settings.clone();
    assert!(!settings.enabled);
    let Connection::Remote(remote) = &mut settings.connection else {
        panic!("expected a URL")
    };
    assert_eq!(remote.transport, RemoteTransport::Http);
    assert_eq!(remote.headers["Authorization"].expose(), "Bearer token");
    remote.url = SecretText::new("https://tools.example.test/v2");
    settings.enabled = true;
    let files = editable(files)?;
    assert!(files.settings.enabled);
    assert!(
        matches!(&files.settings.connection, Connection::Stdio(stdio) if stdio.command == "npx" && stdio.args.len() == 2)
    );

    change(
        &path,
        Harness::Agy,
        save("api", settings.clone(), Some(previous)),
    )?;
    let mut sse = settings;
    let Connection::Remote(remote) = &mut sse.connection else {
        panic!("expected a URL")
    };
    remote.transport = RemoteTransport::Sse;
    assert!(change(&path, Harness::Agy, save("other", sse, None)).is_err());
    change(&path, Harness::Agy, save("local", stdio("local-mcp"), None))?;

    let config: Value = serde_json::from_str(&read(&path)?)?;
    assert_eq!(
        config["mcpServers"]["api"],
        serde_json::json!({
            "headers": { "Authorization": "Bearer token" }, "serverUrl": "https://tools.example.test/v2", "extraSetting": true
        })
    );
    assert_eq!(
        config["mcpServers"]["files"],
        original["mcpServers"]["files"]
    );
    assert_eq!(
        config["mcpServers"]["local"],
        serde_json::json!({ "command": "local-mcp" })
    );
    Ok(())
}
