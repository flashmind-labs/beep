//! How a session gets compacted. An agent with its own auto-compaction is
//! configured through its environment (`Policy::env`). Only an agent with no
//! way to configure its own gets the node's `Compactor`: once a turn ends past
//! the threshold `Policy` sets, it sends `/compact` before the next prompt.
//! A token count instead of a percent goes to Codex as its own limit, and to
//! Claude Code and pi as the node's `/compact` (Claude Code's own trigger is
//! then turned off).
//! Agents that compact on their own with no setting (Gemini CLI, flsh) are
//! left to it.

use beep_proto::{AgentContext, node::harness::AutoCompact};
use std::collections::BTreeMap;

/// The prompt that asks an agent to compact its conversation.
pub(super) const COMPACT_PROMPT: &str = "/compact";
/// What the transcript shows for a compaction, as Claude Code's reader does.
pub(super) const COMPACTED_ENTRY: &str = "[conversation compacted]";

/// Whether and when the node compacts a harness's sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Policy {
    Off,
    /// At this percent of the window, 1 to 100.
    At(u8),
    /// At this many tokens in the conversation, whatever the window.
    AtTokens(u64),
}

impl Policy {
    /// The policy a harness's setting gives: the default when it sets
    /// none or a percent out of range.
    pub(super) fn from_setting(setting: Option<AutoCompact>) -> Self {
        let Some(setting) = setting else {
            return Self::default();
        };
        if !setting.enabled {
            return Self::Off;
        }
        if let Some(tokens) = setting.tokens {
            if tokens > 0 {
                return Self::AtTokens(tokens);
            }
            log::warn!("auto-compact at 0 tokens is not a limit; using the percent");
        }
        match setting.percent {
            None => Self::default(),
            Some(percent) if AutoCompact::valid_percent(percent) => Self::At(percent),
            Some(percent) => {
                log::warn!(
                    "auto-compact {percent}% is not 1 to 100; compacting at {}%",
                    AutoCompact::DEFAULT_PERCENT
                );
                Self::default()
            }
        }
    }

    /// Whether a session at `usage` is due a compaction.
    pub(super) fn due(self, usage: Option<AgentContext>) -> bool {
        let Some(usage) = usage else {
            return false;
        };
        let percent = match self {
            Self::Off => return false,
            Self::AtTokens(tokens) => return usage.used >= tokens,
            Self::At(percent) => percent,
        };
        if usage.size == 0 {
            return false;
        }
        // Widened, so neither side can overflow.
        u128::from(usage.used) * 100 >= u128::from(usage.size) * u128::from(percent)
    }

    /// The variable to add so the agent's own trigger matches, when `route`
    /// has one and the user has not set it.
    pub(super) fn env(self, route: Route, user: &BTreeMap<String, String>) -> Option<EnvVar> {
        let (name, value) = match (route, self) {
            (Route::Percent { percent_var, .. }, Self::At(percent)) => {
                (percent_var, percent.to_string())
            }
            // The node compacts by token count, so the agent's own trigger
            // would only get in its way.
            (Route::Percent { off_var, .. } | Route::Toggle { off_var }, Self::Off)
            | (Route::Percent { off_var, .. }, Self::AtTokens(_)) => (off_var, "1".to_owned()),
            (Route::TokenLimit { config_var, key }, Self::AtTokens(tokens)) => {
                (config_var, serde_json::json!({ key: tokens }).to_string())
            }
            _ => return None,
        };
        (!user.contains_key(name)).then_some(EnvVar { name, value })
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::At(AutoCompact::DEFAULT_PERCENT)
    }
}

/// A variable to set on an agent's process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EnvVar {
    pub(super) name: &'static str,
    pub(super) value: String,
}

/// What an agent takes to compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// It compacts on its own trigger and takes no setting for it.
    OwnTrigger,
    /// Its own setting takes a percent of the window, and a variable that
    /// turns it off.
    Percent {
        percent_var: &'static str,
        off_var: &'static str,
    },
    /// Its own setting only turns on (its default) or off.
    Toggle { off_var: &'static str },
    /// With `/compact`, when it advertises the command.
    Advertised,
    /// As `Advertised` for a percent. A token count is its own setting,
    /// `key` in the JSON object in `config_var`.
    TokenLimit {
        config_var: &'static str,
        key: &'static str,
    },
}

impl Route {
    /// Whether the node sends `/compact` under `policy`, given the commands
    /// the agent advertised.
    pub(super) fn takes_compact(self, policy: Policy, commands: &[String]) -> bool {
        let tokens = matches!(policy, Policy::AtTokens(_));
        let node_compacts = match self {
            Self::Advertised => true,
            // Its own setting has no token count, so the node counts.
            Self::Percent { .. } => tokens,
            Self::TokenLimit { .. } => !tokens,
            Self::OwnTrigger | Self::Toggle { .. } => false,
        };
        node_compacts
            && commands
                .iter()
                .any(|name| name.trim_start_matches('/') == "compact")
    }
}

/// The node's compaction state for one session.
#[derive(Debug)]
pub(super) struct Compactor {
    policy: Policy,
    route: Route,
    /// A turn ended past the threshold; `/compact` goes before the next
    /// prompt.
    due: bool,
    /// The node's `/compact` is running.
    running: bool,
}

/// What a session looks like when its turn ends.
#[derive(Debug, Clone, Copy)]
pub(super) struct TurnEnd<'a> {
    pub(super) usage: Option<AgentContext>,
    pub(super) commands: &'a [String],
}

impl Compactor {
    pub(super) fn new(route: Route, policy: Policy) -> Self {
        Self {
            policy,
            route,
            due: false,
            running: false,
        }
    }

    /// A turn the user asked for ended.
    pub(super) fn turn_ended(&mut self, end: TurnEnd<'_>) {
        self.due =
            self.policy.due(end.usage) && self.route.takes_compact(self.policy, end.commands);
    }

    /// Whether to send `/compact` now, before the next prompt: never while
    /// a question waits or a compaction already runs. True marks it sent.
    pub(super) fn start(&mut self, question_open: bool) -> bool {
        if !self.due || self.running || question_open {
            return false;
        }
        self.due = false;
        self.running = true;
        true
    }

    /// The node's `/compact` finished, however it went.
    pub(super) fn finished(&mut self) {
        self.running = false;
    }
}

#[cfg(test)]
mod tests {
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

    fn commands(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn ended(compactor: &mut Compactor, usage: Option<AgentContext>, commands: &[String]) {
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
        for agent in ["gemini", "flsh"] {
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
        assert_eq!(Policy::from_setting(None), Policy::At(80));
        assert_eq!(Policy::from_setting(setting(true, None)), Policy::At(80));
        assert_eq!(
            Policy::from_setting(setting(true, Some(70))),
            Policy::At(70)
        );
        assert_eq!(Policy::from_setting(setting(false, Some(70))), Policy::Off);
        assert_eq!(
            Policy::from_setting(setting(true, Some(150))),
            Policy::At(80)
        );
        assert_eq!(Policy::from_setting(setting(true, Some(0))), Policy::At(80));
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
        assert_eq!(Policy::from_setting(Some(both)), Policy::AtTokens(350_000));
        assert_eq!(Policy::from_setting(tokens(0)), Policy::At(80));
        let off = AutoCompact {
            enabled: false,
            ..both
        };
        assert_eq!(Policy::from_setting(Some(off)), Policy::Off);
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
        for agent in ["codex", "pi", "gemini", "flsh", "unknown"] {
            assert_eq!(Policy::Off.env(route(agent), &env(&[])), None);
            assert_eq!(Policy::At(70).env(route(agent), &env(&[])), None);
        }
    }
}
