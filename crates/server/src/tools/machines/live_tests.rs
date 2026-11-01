//! The machine tools against real nodes in throwaway containers. Ignored by
//! default: it needs Docker, a node image and a built `beep-node`. Run it
//! with
//!
//!     BEEP_LIVE_NODE_IMAGE=beep-node:test BEEP_LIVE_NODE_BIN=target/debug/beep-node \
//!     cargo test -p beep-server live_tests -- --ignored --nocapture

use super::{Machines, exec::ExecTool, process::ProcessTool};
use crate::{
    nodes,
    protocol::DeviceKind,
    store::NewKey,
    tools::SendFileTool,
    watchers::testing::{Harness, Scripted},
};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
use serde_json::{Value, json};
use std::{path::Path, process::Command, time::Duration};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A container that is removed when the test ends, passed or not.
struct Container(String);

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

/// Starts a node in a container named `name`, signed in to `server` with
/// a fresh key linked to the user. `workspace` makes it their environment.
async fn start_node(
    harness: &Harness,
    server: &str,
    name: &str,
    workspace: bool,
    state_root: &Path,
) -> (Container, String) {
    let image = std::env::var("BEEP_LIVE_NODE_IMAGE").expect("BEEP_LIVE_NODE_IMAGE names an image");
    let bin = std::env::var("BEEP_LIVE_NODE_BIN").expect("BEEP_LIVE_NODE_BIN is a beep-node");
    let bin = std::fs::canonicalize(bin).expect("the beep-node binary exists");
    let dir = state_root.join(name);
    let state = beep_node::state::State::create(&dir, server, name).expect("the node's state");
    harness
        .alice
        .link_key(NewKey {
            endpoint: state.key.public(),
            name,
            kind: DeviceKind::Desktop {},
        })
        .await
        .expect("the node key links");
    let mut args = vec![
        "run".to_owned(),
        "-d".into(),
        "--name".into(),
        name.into(),
        "--network".into(),
        "host".into(),
        "--user".into(),
        "1000:1000".into(),
        "--entrypoint".into(),
        "/usr/local/bin/beep-node-live".into(),
        "-v".into(),
        format!("{}:/usr/local/bin/beep-node-live:ro", bin.display()),
        "-v".into(),
        format!("{}:/state", dir.display()),
        image,
        "--dir".into(),
        "/state".into(),
    ];
    if workspace {
        args.extend(["--workspace".into(), "/home/user".into()]);
    } else {
        // Shares the home folder over files/1, as a Mac's node.toml would.
        std::fs::write(
            dir.join("node.toml"),
            format!("server = \"{server}\"\nname = \"{name}\"\nfiles = [\"/home/user\"]\n"),
        )
        .expect("node.toml");
    }
    args.push("run".into());
    let started = Command::new("docker")
        .args(&args)
        .output()
        .expect("docker runs");
    assert!(
        started.status.success(),
        "docker run failed: {}",
        String::from_utf8_lossy(&started.stderr)
    );
    (Container(name.to_owned()), state.key.public().to_string())
}

async fn call(tool: &dyn Tool, args: Value) -> ToolResult {
    let cancel = CancellationToken::new();
    tool.execute(ToolContext::new("live", args, None, &cancel))
        .await
        .expect("the tool returns a result")
}

#[tokio::test]
#[ignore = "needs Docker, BEEP_LIVE_NODE_IMAGE and BEEP_LIVE_NODE_BIN"]
async fn exec_process_and_copy_on_real_nodes() {
    let harness = Harness::new(Scripted::default()).await;
    let app = harness.app.clone();
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the server endpoint binds");
    let handler = nodes::NodeHandler::new(app.clone(), endpoint.id());
    let router = Router::builder(endpoint)
        .accept(nodes::NODE_ALPN, handler)
        .spawn();
    let server = router.endpoint().id().to_string();
    let states = tempfile::tempdir().expect("a state folder");

    let (_home, home_key) =
        start_node(&harness, &server, "beep-live-home", true, states.path()).await;
    let (_other, other_key) =
        start_node(&harness, &server, "beep-live-other", false, states.path()).await;
    for key in [&home_key, &other_key] {
        timeout(Duration::from_secs(60), async {
            while app.nodes.online(key).is_none() {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .expect("the node came online");
    }

    let machines = Machines {
        app: app.clone(),
        user: harness.alice.clone(),
    };
    let exec = ExecTool {
        machines: machines.clone(),
    };
    let process = ProcessTool {
        machines: machines.clone(),
    };

    let result = call(&exec, json!({ "command": "curl -sI https://example.com" })).await;
    println!("exec curl:\n{}", result.output());
    assert!(result.is_success(), "{}", result.output());
    assert!(
        result.output().starts_with("[exit code: 0]\nHTTP/"),
        "{}",
        result.output()
    );

    let result = call(&exec, json!({ "command": "pwd; id -un" })).await;
    println!("exec pwd:\n{}", result.output());
    assert_eq!(result.output(), "[exit code: 0]\n/home/user\nuser");

    let result = call(
        &exec,
        json!({ "command": "sleep 300", "run_in_background": true }),
    )
    .await;
    println!("exec background:\n{}", result.output());
    assert!(result.is_success(), "{}", result.output());
    let id: u32 = result
        .output()
        .split_whitespace()
        .nth(3)
        .and_then(|id| id.parse().ok())
        .expect("a process id");

    let listed = call(&process, json!({ "action": "list" })).await;
    println!("process list:\n{}", listed.output());
    assert_eq!(listed.output(), format!("{id}: running (sleep 300)"));
    let seen = call(&exec, json!({ "command": "pgrep -x sleep" })).await;
    assert!(seen.is_success(), "the sleep runs: {}", seen.output());

    let stopped = call(&process, json!({ "action": "stop", "id": id })).await;
    println!("process stop:\n{}", stopped.output());
    assert_eq!(stopped.output(), format!("Process {id}: stopped"));
    let gone = call(&exec, json!({ "command": "pgrep -x sleep" })).await;
    println!("pgrep after stop:\n{}", gone.output());
    assert_eq!(gone.output(), "[exit code: 1]\n", "the sleep was killed");

    // A file made on the environment copies to the other machine and back.
    let made = call(
        &exec,
        json!({ "command": "head -c 9000000 /dev/urandom > big.bin && sha256sum big.bin | cut -d' ' -f1" }),
    )
    .await;
    assert!(made.is_success(), "{}", made.output());
    let sum = made.output().lines().nth(1).expect("a checksum").to_owned();
    let send = SendFileTool {
        user: harness.alice.clone(),
        session_id: Uuid::new_v4(),
        workspace: None,
        machines: Some(machines.clone()),
        paper: false,
    };
    let folder = call(
        &exec,
        json!({ "command": "mkdir -p ~/in", "in": "beep-live-other" }),
    )
    .await;
    assert!(folder.is_success(), "{}", folder.output());
    let copied = call(
        &send,
        json!({ "path": "big.bin", "to": "beep-live-other", "dst_folder": "~/in" }),
    )
    .await;
    println!("send_file copy:\n{}", copied.output());
    assert!(copied.is_success(), "{}", copied.output());
    assert!(
        copied.output().starts_with("Sent 8.6 MiB"),
        "{}",
        copied.output()
    );
    let again = call(
        &send,
        json!({ "path": "big.bin", "to": "beep-live-other", "dst_folder": "~/in" }),
    )
    .await;
    assert!(
        again.output().contains("big (2).bin"),
        "a second send keeps both: {}",
        again.output()
    );
    let there = call(
        &exec,
        json!({ "command": "sha256sum ~/in/big.bin | cut -d' ' -f1", "in": "beep-live-other" }),
    )
    .await;
    println!("checksum on the other machine:\n{}", there.output());
    assert_eq!(there.output(), format!("[exit code: 0]\n{sum}"));
    let back = call(
        &send,
        json!({ "from": "beep-live-other", "path": "~/in/big.bin", "to": "beep-live-home" }),
    )
    .await;
    println!("send_file copy back:\n{}", back.output());
    assert!(back.is_success(), "{}", back.output());

    router.shutdown().await.expect("the router stops");
}

#[tokio::test]
#[ignore = "needs Docker, BEEP_LIVE_NODE_IMAGE, BEEP_LIVE_NODE_BIN and github.com"]
async fn import_project_on_a_real_node() {
    let harness = Harness::new(Scripted::default()).await;
    let app = harness.app.clone();
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the server endpoint binds");
    let handler = nodes::NodeHandler::new(app.clone(), endpoint.id());
    let router = Router::builder(endpoint)
        .accept(nodes::NODE_ALPN, handler)
        .spawn();
    let server = router.endpoint().id().to_string();
    let states = tempfile::tempdir().expect("a state folder");
    let (_home, key) = start_node(&harness, &server, "beep-live-import", true, states.path()).await;
    timeout(Duration::from_secs(60), async {
        while app.nodes.online(&key).is_none() {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("the node came online");
    let machines = Machines {
        app: app.clone(),
        user: harness.alice.clone(),
    };
    let import = crate::imports::ImportProjectTool {
        machines: machines.clone(),
    };
    let exec = ExecTool { machines };

    let first = call(
        &import,
        json!({ "repo": "https://github.com/octocat/Hello-World" }),
    )
    .await;
    println!("import:\n{}", first.output());
    assert!(first.is_success(), "{}", first.output());
    assert!(
        first
            .output()
            .starts_with("Cloned octocat/Hello-World on beep-live-import in /home/user/projects/octocat/Hello-World"),
        "{}",
        first.output()
    );
    let checked = call(
        &exec,
        json!({ "command": "git -C projects/octocat/Hello-World remote get-url origin; ls -A projects/octocat" }),
    )
    .await;
    println!("checkout:\n{}", checked.output());
    assert_eq!(
        checked.output(),
        "[exit code: 0]\nhttps://github.com/octocat/Hello-World.git\nHello-World"
    );

    let again = call(&import, json!({ "repo": "octocat/Hello-World" })).await;
    println!("import again:\n{}", again.output());
    assert!(
        again
            .output()
            .starts_with("Found an existing checkout of octocat/Hello-World"),
        "{}",
        again.output()
    );

    let missing = call(&import, json!({ "repo": "octocat/no-such-repo-beep-test" })).await;
    println!("import missing:\n{}", missing.output());
    assert_eq!(
        missing.output(),
        "Could not find octocat/no-such-repo-beep-test on GitHub. If it is private, add a \
         token for GitHub in Settings > Credentials."
    );
    let listed = crate::imports::imported(&harness.alice)
        .await
        .expect("imports list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].project, "octocat/Hello-World");

    router.shutdown().await.expect("the router stops");
}
