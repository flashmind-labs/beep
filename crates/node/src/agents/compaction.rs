//! When the node compacts a session (`Compactor`): once a turn ends past the
//! threshold `Policy` sets, it sends `/compact` before the next prompt. Agents
//! that compact on their own (Gemini CLI, flsh) are left to it.

use super::Known;
use beep_proto::{AgentContext, node::harness::AutoCompact};
use std::collections::BTreeMap;

/// Claude Code's own threshold, a percent of its window.
const CLAUDE_AUTOCOMPACT_VAR: &str = "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE";
/// Turns Claude Code's own compaction off.
const CLAUDE_DISABLE_VAR: &str = "DISABLE_AUTO_COMPACT";
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
        let (Self::At(percent), Some(usage)) = (self, usage) else {
            return false;
        };
        if usage.size == 0 {
            return false;
        }
        // Widened, so neither side can overflow.
        u128::from(usage.used) * 100 >= u128::from(usage.size) * u128::from(percent)
    }

    /// The variables to add for agent `agent` so its own trigger matches:
    /// Claude Code's, unless the user set it.
    pub(super) fn agent_env(self, agent: &str, env: &BTreeMap<String, String>) -> Option<EnvVar> {
        if agent != "claude" {
            return None;
        }
        let (name, value) = match self {
            Self::At(percent) => (CLAUDE_AUTOCOMPACT_VAR, percent.to_string()),
            Self::Off => (CLAUDE_DISABLE_VAR, "1".to_owned()),
        };
        (!env.contains_key(name)).then_some(EnvVar { name, value })
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

/// How an agent can be asked to compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// It compacts on its own trigger and takes no command for it.
    OwnTrigger,
    /// With `/compact`, when it advertises the command.
    Advertised,
    /// With `/compact`, which it takes without advertising it.
    Always,
}

impl Route {
    /// Whether `/compact` works, given the commands the agent advertised.
    pub(super) fn takes_compact(self, commands: &[String]) -> bool {
        match self {
            Self::OwnTrigger => false,
            Self::Always => true,
            Self::Advertised => commands
                .iter()
                .any(|name| name.trim_start_matches('/') == "compact"),
        }
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
    /// For the agent `known`; one the node does not know takes `/compact`
    /// when it advertises it.
    pub(super) fn new(known: Option<&Known>, policy: Policy) -> Self {
        Self {
            policy,
            route: known.map_or(Route::Advertised, |known| known.compact),
            due: false,
            running: false,
        }
    }

    /// A turn the user asked for ended.
    pub(super) fn turn_ended(&mut self, end: TurnEnd<'_>) {
        self.due = self.policy.due(end.usage) && self.route.takes_compact(end.commands);
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
    use crate::agents::known;

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
        let mut compactor = Compactor::new(known("codex"), Policy::default());
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
        let mut compactor = Compactor::new(known("pi"), Policy::default());
        ended(&mut compactor, usage(95, 100), &commands(&["/compact"]));
        assert!(!compactor.start(true));
        assert!(compactor.start(false), "still due once answered");
    }

    #[test]
    fn a_harness_without_the_command_never_gets_it() {
        for agent in ["gemini", "flsh"] {
            let mut compactor = Compactor::new(known(agent), Policy::default());
            ended(&mut compactor, usage(99, 100), &commands(&["compact"]));
            assert!(!compactor.start(false), "{agent} compacts on its own");
        }
        let mut compactor = Compactor::new(known("codex"), Policy::default());
        ended(&mut compactor, usage(99, 100), &commands(&["review"]));
        assert!(!compactor.start(false), "not advertised");
        let mut compactor = Compactor::new(known("opencode"), Policy::default());
        ended(&mut compactor, usage(99, 100), &[]);
        assert!(compactor.start(false), "OpenCode takes it unadvertised");
    }

    #[test]
    fn a_turn_back_under_the_threshold_clears_it() {
        let mut compactor = Compactor::new(known("codex"), Policy::default());
        ended(&mut compactor, usage(90, 100), &commands(&["compact"]));
        ended(&mut compactor, usage(10, 100), &commands(&["compact"]));
        assert!(!compactor.start(false));
    }

    fn setting(enabled: bool, percent: Option<u8>) -> Option<AutoCompact> {
        Some(AutoCompact { enabled, percent })
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
        let mut off = Compactor::new(known("codex"), Policy::Off);
        ended(&mut off, usage(100, 100), &commands(&["compact"]));
        assert!(!off.start(false));
        assert!(Policy::At(70).due(usage(70, 100)));
        assert!(!Policy::At(70).due(usage(1, 0)), "no window, no compaction");
        assert!(Policy::At(100).due(usage(u64::MAX, u64::MAX)));
    }

    #[test]
    fn claude_gets_its_own_threshold_unless_the_user_set_one() {
        let set = Policy::At(70).agent_env("claude", &env(&[]));
        assert_eq!(
            set,
            Some(EnvVar {
                name: CLAUDE_AUTOCOMPACT_VAR,
                value: "70".into()
            })
        );
        let own = env(&[(CLAUDE_AUTOCOMPACT_VAR, "60")]);
        assert_eq!(Policy::At(70).agent_env("claude", &own), None);
        let disabled = Policy::Off.agent_env("claude", &env(&[]));
        assert_eq!(
            disabled,
            Some(EnvVar {
                name: CLAUDE_DISABLE_VAR,
                value: "1".into()
            })
        );
        let own = env(&[(CLAUDE_DISABLE_VAR, "0")]);
        assert_eq!(Policy::Off.agent_env("claude", &own), None);
        assert_eq!(Policy::At(70).agent_env("codex", &env(&[])), None);
    }
}
