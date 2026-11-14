use std::time::{Duration, Instant};

use beep_plugins::{Handler, Host, Placement, PluginState, Topic};

use super::*;

struct Quiet;

impl Handler for Quiet {
    fn shown(&self, _: &[Placement]) {}

    fn wanted(&self, _: &[Topic]) {}
}

fn id(text: &str) -> PluginId {
    text.parse().unwrap()
}

fn ask(host: &Host, logs: Option<&Path>, call: Value) -> Result<Value, String> {
    let text = answer(host, logs, &call.to_string())?;

    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn wait_until_running(host: &Host, plugin: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !host
        .status()
        .iter()
        .any(|status| status.id == id(plugin) && status.state == PluginState::Running)
    {
        assert!(Instant::now() < deadline, "{plugin} never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_agent_creates_lists_reads_and_calls_plugins_through_a_real_host() {
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    let logs = root.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let host = Host::start(plugins.clone(), Quiet, Policy::Managed, &[]).unwrap();
    host.set_visible("main", true);
    let dir = logs.as_path();
    let logs = Some(dir);

    // A new plugin waits for the user, and the agent is told so.
    let created = answer(
        &host,
        logs,
        &json!({ "action": "create", "plugin": "helper", "name": "Helper",
                 "description": "Does a thing.", "permissions": ["tools"] })
        .to_string(),
    )
    .unwrap();
    assert!(created.contains("the user must allow it"), "{created}");
    assert!(created.contains("plugins/README.md"), "{created}");
    assert!(plugins.join("helper/beep-plugin.json").is_file());
    assert!(plugins.join("helper/main.ts").is_file());

    let listed = ask(&host, logs, json!({ "action": "list" })).unwrap();
    assert_eq!(listed[0]["id"], "helper");
    assert_eq!(listed[0]["state"], "needs-approval");
    assert_eq!(listed[0]["source"]["kind"], "local");

    // It cannot be created twice, and an agent's call to an unapproved plugin fails by state.
    let again = ask(
        &host,
        logs,
        json!({ "action": "create", "plugin": "helper", "name": "H", "description": "" }),
    );
    assert!(again.unwrap_err().contains("already a plugin"));
    let refused = ask(
        &host,
        logs,
        json!({ "action": "call", "plugin": "helper", "tool": "echo" }),
    );
    assert!(refused.unwrap_err().contains("approval"));

    // The user allows a plugin with a tool, as Settings does.
    std::fs::write(
        plugins.join("helper/beep-plugin.json"),
        r#"{ "id": "helper", "name": "Helper", "version": "0.1.0", "permissions": ["tools"],
             "tools": [{ "name": "echo", "description": "Echoes.",
                         "input": { "type": "object", "required": ["text"] } }] }"#,
    )
    .unwrap();
    std::fs::write(
        plugins.join("helper/main.ts"),
        r#"beep.log("helper ready");
           beep.tool("echo", (input: { text: string }) => ({ echoed: input.text }));"#,
    )
    .unwrap();
    host.refresh(&id("helper"));
    host.approve(&id("helper")).unwrap();
    wait_until_running(&host, "helper");

    let called = answer(
        &host,
        logs,
        &json!({ "action": "call", "plugin": "helper", "tool": "echo", "arguments": { "text": "hi" } })
            .to_string(),
    )
    .unwrap();
    assert!(called.contains("untrusted"), "{called}");
    assert!(called.ends_with(r#"{"echoed":"hi"}"#), "{called}");
    let bad = ask(
        &host,
        logs,
        json!({ "action": "call", "plugin": "helper", "tool": "echo", "arguments": {} }),
    );
    assert!(bad.unwrap_err().contains("do not fit"));

    // Status reads the plugin's lines from the newest log file only.
    std::fs::write(
        dir.join("beep.2026-11-13.log"),
        "2026-11-13T09:00:00.000000Z  INFO plugin: helper: yesterday\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("beep.2026-11-14.log"),
        "2026-11-14T09:00:00.000000Z  INFO plugin: helper: one\n\
         2026-11-14T09:00:01.000000Z  INFO plugin: helper-two: not mine\n\
         2026-11-14T09:00:02.000000Z  WARN beep_desktop: plugin: helper: quoted\n\
         2026-11-14T09:00:03.000000Z ERROR plugin: helper: two\n\
         2026-11-14T09:00:04.000000Z  INFO plugin: helper: three\n",
    )
    .unwrap();
    let status = ask(
        &host,
        Some(dir),
        json!({ "action": "status", "plugin": "helper", "lines": 2 }),
    )
    .unwrap();
    assert_eq!(status["plugin"]["state"], "running");
    assert_eq!(status["plugin"]["tools"][0]["name"], "echo");
    assert_eq!(status["placements"], 0);
    assert_eq!(status["logFile"], "beep.2026-11-14.log");
    let lines: Vec<&str> = status["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap().rsplit(": ").next().unwrap())
        .collect();
    assert_eq!(lines, ["two", "three"]);

    let missing = ask(
        &host,
        Some(dir),
        json!({ "action": "status", "plugin": "nope" }),
    );
    assert!(missing.unwrap_err().contains("no plugin nope"));
}

#[test]
fn a_development_folder_runs_what_an_agent_creates() {
    let root = tempfile::tempdir().unwrap();
    let host = Host::start(root.path().join("plugins"), Quiet, Policy::Dev, &[]).unwrap();
    host.set_visible("main", true);

    let created = answer(
        &host,
        None,
        &json!({ "action": "create", "plugin": "devbox", "name": "Dev", "description": "x" })
            .to_string(),
    )
    .unwrap();

    assert!(created.contains("without approval"), "{created}");
    wait_until_running(&host, "devbox");
}

#[test]
fn bad_arguments_and_unknown_actions_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let host = Host::start(root.path().join("plugins"), Quiet, Policy::Dev, &[]).unwrap();

    for call in [
        json!({ "action": "delete" }),
        json!({ "action": "create", "plugin": "Bad Id", "name": "n", "description": "d" }),
        json!({ "action": "create", "plugin": "ab", "name": " ", "description": "d" }),
        json!({ "action": "call", "plugin": "ab", "tool": "Bad-Name" }),
    ] {
        assert!(ask(&host, None, call.clone()).is_err(), "{call}");
    }
}
