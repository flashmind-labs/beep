use super::*;

fn known(name: &str) -> &'static Known {
    crate::known::known(name).expect("a known agent")
}

/// `found` for a machine that has exactly `programs`.
fn has(programs: &'static [&'static str]) -> impl Fn(&str) -> bool {
    move |program: &str| programs.contains(&program)
}

fn own() -> adapters::Installed {
    adapters::Installed {
        node: PathBuf::from("/srv/node/agent-adapters/node/bin/node"),
        script: PathBuf::from("/srv/node/agent-adapters/claude/node_modules/a/dist/index.js"),
        current: true,
    }
}

#[test]
fn a_native_cli_with_no_adapter_and_no_npx_lacks_its_adapter_or_runs_direct() {
    let natives = has(&["claude", "codex", "pi", "agy", "hermes"]);

    assert_eq!(
        launch(known("pi"), &natives, None),
        Some(Launch::LacksAdapter)
    );
    // Claude Code and Codex run on the node's own bridge meanwhile.
    let Some(Launch::Direct(claude)) = launch(known("claude"), &natives, None) else {
        panic!("Claude Code runs without its adapter");
    };
    assert_eq!(
        claude.bridge,
        Some(Bridge::Claude {
            wsl: None,
            home: None
        })
    );
    let Some(Launch::Direct(codex)) = launch(known("codex"), &natives, None) else {
        panic!("Codex runs without its adapter");
    };
    assert_eq!(codex.bridge, Some(Bridge::Codex { wsl: None }));
    // Antigravity speaks ACP itself.
    assert!(matches!(
        launch(known("agy"), &natives, None),
        Some(Launch::Ready(_))
    ));
    assert!(matches!(
        launch(known("hermes"), &natives, None),
        Some(Launch::Ready(_))
    ));
    assert_eq!(launch(known("hermes"), &has(&["npx"]), None), None);
    // No CLI, nothing to install.
    assert_eq!(launch(known("opencode"), &natives, None), None);
    assert_eq!(launch(known("claude"), &has(&["npx"]), None), None);
}

#[test]
fn the_machines_own_adapter_wins_then_the_nodes_then_npx() {
    let own = own();
    let with_bin = has(&["claude", "claude-agent-acp", "npx"]);
    let Some(Launch::Ready(kind)) = launch(known("claude"), &with_bin, Some(&own)) else {
        panic!("the machine's adapter");
    };
    assert_eq!(kind.program, "claude-agent-acp");

    let with_npx = has(&["claude", "npx"]);
    let Some(Launch::OnOwnNode(kind)) = launch(known("claude"), &with_npx, Some(&own)) else {
        panic!("the node's adapter");
    };
    assert_eq!(PathBuf::from(&kind.program), own.node);
    assert_eq!(kind.args, vec![own.script.to_string_lossy().into_owned()]);

    let Some(Launch::ViaNpx(kind)) = launch(known("claude"), &with_npx, None) else {
        panic!("npx");
    };
    assert_eq!(kind.program, "npx");
}

#[test]
fn npx_fallback_is_offered_while_the_direct_adapter_is_installed() {
    let codex = known("codex");
    let mut detection = Detection {
        kinds: Vec::new(),
        lacking: Vec::new(),
        on_own_node: Vec::new(),
    };
    detection.record(codex, launch(codex, &has(&["codex", "npx"]), None), None);
    assert_eq!(detection.kinds.len(), 1);
    assert_eq!(detection.kinds[0].program, "npx");
    assert_eq!(detection.lacking.len(), 1);
    assert_eq!(detection.lacking[0].name, "codex");
}
