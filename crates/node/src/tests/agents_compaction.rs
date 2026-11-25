use super::*;
use crate::agents::route;

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn usage(used: u64, size: u64) -> Option<AgentContext> {
    Some(AgentContext { used, size })
}

fn commands(names: &[&str]) -> Vec<AgentCommand> {
    names
        .iter()
        .map(|name| AgentCommand {
            name: (*name).to_owned(),
            ..AgentCommand::default()
        })
        .collect()
}

fn ended(compactor: &mut Compactor, usage: Option<AgentContext>, commands: &[AgentCommand]) {
    compactor.turn_ended(TurnEnd { usage, commands });
}

#[test]
fn at_the_threshold_it_compacts_once_before_the_next_prompt() {
    let mut compactor = Compactor::new(route("codex"), Policy::default());
    ended(
        &mut compactor,
        usage(80_000, 100_000),
        &commands(&["compact"]),
    );
    assert!(compactor.start(false));
    assert!(!compactor.start(false), "once");
    compactor.finished();
    assert!(
        !compactor.start(false),
        "not again until a turn ends past it"
    );
}

#[test]
fn not_while_a_question_waits() {
    let mut compactor = Compactor::new(route("pi"), Policy::default());
    ended(&mut compactor, usage(95, 100), &commands(&["/compact"]));
    assert!(!compactor.start(true));
    assert!(compactor.start(false), "still due once answered");
}

#[test]
fn a_harness_without_the_command_never_gets_it() {
    for agent in ["agy", "flsh"] {
        let mut compactor = Compactor::new(route(agent), Policy::default());
        ended(&mut compactor, usage(99, 100), &commands(&["compact"]));
        assert!(!compactor.start(false), "{agent} compacts on its own");
    }

    let mut compactor = Compactor::new(route("codex"), Policy::default());
    ended(&mut compactor, usage(99, 100), &commands(&["review"]));
    assert!(!compactor.start(false), "not advertised");

    for agent in ["claude", "opencode"] {
        let mut compactor = Compactor::new(route(agent), Policy::default());
        ended(&mut compactor, usage(99, 100), &commands(&["compact"]));
        assert!(!compactor.start(false), "{agent} has its own setting");
    }
}

#[test]
fn a_turn_back_under_the_threshold_clears_it() {
    let mut compactor = Compactor::new(route("codex"), Policy::default());
    ended(&mut compactor, usage(90, 100), &commands(&["compact"]));
    ended(&mut compactor, usage(10, 100), &commands(&["compact"]));
    assert!(!compactor.start(false));
}

fn setting(enabled: bool, percent: Option<u8>) -> Option<AutoCompact> {
    Some(AutoCompact {
        enabled,
        percent,
        tokens: None,
    })
}

#[test]
fn the_threshold_reads_from_the_setting() {
    assert_eq!(Policy::from(None), Policy::At(80));
    assert_eq!(Policy::from(setting(true, None)), Policy::At(80));
    assert_eq!(Policy::from(setting(true, Some(70))), Policy::At(70));
    assert_eq!(Policy::from(setting(false, Some(70))), Policy::Off);
    assert_eq!(Policy::from(setting(true, Some(150))), Policy::At(80));
    assert_eq!(Policy::from(setting(true, Some(0))), Policy::At(80));
    let mut off = Compactor::new(route("codex"), Policy::Off);
    ended(&mut off, usage(100, 100), &commands(&["compact"]));
    assert!(!off.start(false));
    assert!(Policy::At(70).due(usage(70, 100)));
    assert!(!Policy::At(70).due(usage(1, 0)), "no window, no compaction");
    assert!(Policy::At(100).due(usage(u64::MAX, u64::MAX)));
}

fn tokens(tokens: u64) -> Option<AutoCompact> {
    Some(AutoCompact {
        enabled: true,
        percent: None,
        tokens: Some(tokens),
    })
}

#[test]
fn a_token_count_wins_over_the_percent() {
    let both = AutoCompact {
        enabled: true,
        percent: Some(70),
        tokens: Some(350_000),
    };
    assert_eq!(Policy::from(Some(both)), Policy::AtTokens(350_000));
    assert_eq!(Policy::from(tokens(0)), Policy::At(80));
    let off = AutoCompact {
        enabled: false,
        ..both
    };
    assert_eq!(Policy::from(Some(off)), Policy::Off);
    let policy = Policy::AtTokens(350_000);
    assert!(policy.due(usage(350_000, 1_000_000)));
    assert!(!policy.due(usage(349_999, 1_000_000)));
    assert!(policy.due(usage(350_000, 0)), "the window does not matter");
    assert!(!policy.due(None));
}

#[test]
fn claude_and_pi_get_the_node_compact_for_a_token_count() {
    let policy = Policy::AtTokens(100);

    for agent in ["claude", "pi"] {
        let mut compactor = Compactor::new(route(agent), policy);
        ended(&mut compactor, usage(100, 1_000), &commands(&["compact"]));
        assert!(compactor.start(false), "{agent}");
    }

    let mut compactor = Compactor::new(route("claude"), policy);
    ended(&mut compactor, usage(99, 1_000), &commands(&["compact"]));
    assert!(!compactor.start(false), "under the count");
    // Claude Code's own trigger is off then, unless the user set it.
    let claude = route("claude");
    assert_eq!(
        policy.env(claude, &env(&[])),
        Some(EnvVar {
            name: "DISABLE_AUTO_COMPACT",
            value: "1".into()
        })
    );
    let own = env(&[("DISABLE_AUTO_COMPACT", "0")]);
    assert_eq!(policy.env(claude, &own), None);
    assert_eq!(policy.env(route("pi"), &env(&[])), None);
}

#[test]
fn codex_gets_a_token_count_as_its_own_limit() {
    let policy = Policy::AtTokens(350_000);
    let codex = route("codex");
    let set = policy.env(codex, &env(&[])).unwrap();
    assert_eq!(set.name, "CODEX_CONFIG");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&set.value).unwrap(),
        serde_json::json!({ "model_auto_compact_token_limit": 350_000 })
    );
    let own = env(&[("CODEX_CONFIG", "{}")]);
    assert_eq!(policy.env(codex, &own), None, "the user's own wins");
    let mut compactor = Compactor::new(codex, policy);
    ended(
        &mut compactor,
        usage(999_999, 1_000_000),
        &commands(&["compact"]),
    );
    assert!(!compactor.start(false), "Codex counts for itself");
    assert_eq!(Policy::At(70).env(codex, &env(&[])), None);
}

#[test]
fn opencode_ignores_a_token_count() {
    let opencode = route("opencode");
    assert_eq!(Policy::AtTokens(100).env(opencode, &env(&[])), None);
    let mut compactor = Compactor::new(opencode, Policy::AtTokens(100));
    ended(&mut compactor, usage(500, 1_000), &commands(&["compact"]));
    assert!(!compactor.start(false));
}

#[test]
fn claude_gets_its_own_threshold_unless_the_user_set_one() {
    let claude = route("claude");
    let set = Policy::At(70).env(claude, &env(&[]));
    assert_eq!(
        set,
        Some(EnvVar {
            name: "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
            value: "70".into()
        })
    );
    let own = env(&[("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", "60")]);
    assert_eq!(Policy::At(70).env(claude, &own), None);
    let disabled = Policy::Off.env(claude, &env(&[]));
    assert_eq!(
        disabled,
        Some(EnvVar {
            name: "DISABLE_AUTO_COMPACT",
            value: "1".into()
        })
    );
    let own = env(&[("DISABLE_AUTO_COMPACT", "0")]);
    assert_eq!(Policy::Off.env(claude, &own), None);
}

#[test]
fn opencode_only_turns_off() {
    let opencode = route("opencode");
    assert_eq!(Policy::At(70).env(opencode, &env(&[])), None);
    assert_eq!(
        Policy::Off.env(opencode, &env(&[])),
        Some(EnvVar {
            name: "OPENCODE_DISABLE_AUTOCOMPACT",
            value: "1".into()
        })
    );
    let own = env(&[("OPENCODE_DISABLE_AUTOCOMPACT", "")]);
    assert_eq!(Policy::Off.env(opencode, &own), None);
}

#[test]
fn the_slash_command_fallback_sets_no_variable() {
    for agent in ["codex", "pi", "flsh", "unknown"] {
        assert_eq!(Policy::Off.env(route(agent), &env(&[])), None);
        assert_eq!(Policy::At(70).env(route(agent), &env(&[])), None);
    }
}
