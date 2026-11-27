use super::*;

#[test]
fn model_rules_follow_switches_and_revert_to_harness_setting() {
    let mut compactor = Compactor::new(crate::known::route("codex"), Policy::AtTokens(800))
        .with_models(vec![
            ModelCompact {
                model: "model-small".into(),
                compact: AutoCompact {
                    enabled: true,
                    percent: None,
                    tokens: Some(100),
                },
            },
            ModelCompact {
                model: "model-off".into(),
                compact: AutoCompact {
                    enabled: false,
                    percent: None,
                    tokens: None,
                },
            },
        ]);
    let commands = [AgentCommand {
        name: "compact".into(),
        ..AgentCommand::default()
    }];
    for (model, due) in [
        ("model-small", true),
        ("model-off", false),
        ("other", false),
    ] {
        compactor.set_model(Some(model));
        compactor.turn_ended(TurnEnd {
            usage: Some(AgentContext {
                used: 100,
                size: 1000,
            }),
            commands: &commands,
        });
        assert_eq!(compactor.start(false), due, "{model}");
        compactor.finished();
    }
    compactor.turn_ended(TurnEnd {
        usage: Some(AgentContext {
            used: 800,
            size: 1000,
        }),
        commands: &commands,
    });
    assert!(
        compactor.start(false),
        "the harness token fallback is enforced between turns too"
    );
}
