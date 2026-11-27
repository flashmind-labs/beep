use super::*;
use crate::agents::adapters::installs::{Install, Installs};
use crate::known::known;
use crate::status::AgentState;
use beep_proto::node::AdapterInstall;
use std::collections::BTreeMap;

fn kind(name: &str) -> AgentKind {
    AgentKind {
        name: name.to_owned(),
        program: name.to_owned(),
        args: Vec::new(),
        env: BTreeMap::new(),
        compact: None,
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

#[test]
fn the_report_says_what_waits_on_an_adapter() {
    let installs = Installs::default();
    let claude = known("claude").expect("claude");
    let codex = known("codex").expect("codex");
    installs.set("claude", Install::Failed("needs Node.js: no curl".into()));
    installs.set("codex", Install::Running);
    assert_eq!(
        installs.pending(),
        [
            AdapterInstall {
                agent: "claude".into(),
                failed: Some("needs Node.js: no curl".into()),
            },
            AdapterInstall {
                agent: "codex".into(),
                failed: None,
            },
        ]
    );
    let report = installs.report(&[kind("agy")]);
    let states: Vec<(&str, &AgentState)> = report
        .iter()
        .map(|agent| (agent.name.as_str(), &agent.state))
        .collect();
    assert_eq!(
        states,
        [
            ("agy", &AgentState::Ready),
            (
                "claude",
                &AgentState::Failed {
                    reason: "needs Node.js: no curl".into()
                }
            ),
            ("codex", &AgentState::Installing),
        ]
    );
    // codex's adapter came; claude's still lacks one.
    installs.saw(&Detection {
        kinds: vec![kind("codex")],
        lacking: vec![claude],
        on_own_node: vec![codex.name],
    });
    let report = installs.report(&[kind("codex")]);
    assert_eq!(report[0].state, AgentState::OwnAdapter);
    assert!(matches!(report[1].state, AgentState::Failed { .. }));
}
