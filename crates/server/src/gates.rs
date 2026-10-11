//! Checks a finished reply before the turn ends. Jev reads the reply with
//! the calls the turn made, and each failed check tells the agent what to
//! fix in one more pass: a map card it left out, an action it claimed but
//! never took, a follow-up it promised with nothing scheduled.

use crate::decision::{Criteria, DecisionTool};
use std::fmt::Write;

/// Jev's probability from which a check fails. High on purpose: a false
/// alarm costs the user a second pass and a stray card.
const THRESHOLD: f64 = 0.6;

/// The tools that put a destination on the map.
const MAP_TOOLS: &[&str] = &["open_maps"];
/// The tools that make Beep do something later.
const SCHEDULE_TOOLS: &[&str] = &["cron_create", "cron_edit", "schedule_once", "place_routine"];

/// A tool call the turn made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub success: bool,
}

/// What a check reads.
pub struct Draft<'a> {
    /// What the user asked, or the routine's note.
    pub asked: &'a str,
    pub reply: &'a str,
    pub calls: &'a [Call],
}

impl Draft<'_> {
    fn called(&self, tools: &[&str]) -> bool {
        self.calls
            .iter()
            .any(|call| call.success && tools.contains(&call.name.as_str()))
    }

    fn state(&self) -> String {
        let mut text = format!(
            "Beep is an assistant in a phone app. It answers with text and can show cards, \
             such as a map, by calling tools.\n\nAsked:\n{}\n\nTool calls this turn:\n",
            self.asked.trim()
        );
        if self.calls.is_empty() {
            text.push_str("- none\n");
        }
        for call in self.calls {
            let outcome = if call.success { "succeeded" } else { "failed" };
            let _ = writeln!(text, "- {} ({outcome})", call.name);
        }
        let _ = write!(text, "\nBeep's reply:\n{}", self.reply.trim());
        text
    }
}

struct Check {
    key: &'static str,
    question: &'static str,
    criteria: Criteria,
    /// What the agent is told when the check fails.
    fix: &'static str,
    /// Whether the turn already settled the check without asking Jev.
    settled: fn(&Draft) -> bool,
}

const CHECKS: [Check; 3] = [
    Check {
        key: "map",
        question: "Does Beep's reply send the user to one specific place to go to now or soon, \
such as a shop, restaurant, station, venue or address?",
        criteria: Criteria {
            yes: "The reply names a particular destination the user is meant to go to.",
            no: "The reply names no destination, only mentions places in passing, or lists \
several options without picking one.",
        },
        fix: "Your reply sends the user somewhere but shows no map. Call open_maps with \
action place, or directions, for that destination.",
        settled: |draft| draft.called(MAP_TOOLS),
    },
    Check {
        key: "claimed",
        question: "Does Beep's reply say that Beep has already done something, such as sent, \
saved, added, booked, set up, changed or deleted something, that none of the succeeded tool \
calls listed did?",
        criteria: Criteria {
            yes: "The reply claims an action that no succeeded tool call in the list carried out.",
            no: "Every action the reply claims matches a succeeded tool call, or the reply \
claims none.",
        },
        fix: "Your reply says you did something that no tool call did. Do it now with the \
right tool, or tell the user plainly that it is not done.",
        settled: |_| false,
    },
    Check {
        key: "promised",
        question: "Does Beep's reply promise that Beep will do something later on its own, \
such as remind the user, check on something, follow up or let them know?",
        criteria: Criteria {
            yes: "The reply promises a later reminder, check or message from Beep.",
            no: "The reply promises nothing Beep would do later on its own.",
        },
        fix: "Your reply promises to do something later, but nothing is scheduled. Set it \
up now with schedule_once, cron_create or place_routine, or tell the user you will not.",
        settled: |draft| draft.called(SCHEDULE_TOOLS),
    },
];

/// Told before the fixes, so the second pass adds to the reply instead of
/// writing it again.
const PREFACE: &str = "Your reply needs a fix. The user already has it, so do not \
repeat it. Make the calls needed, then add at most one \
short sentence, and only when the fix changes what you told them.";

/// A reply that failed checks.
#[derive(Debug)]
pub struct Verdict {
    /// The keys of the checks it failed: `map`, `claimed` or `promised`.
    pub failed: Vec<&'static str>,
    /// What the agent is told in its second pass.
    pub note: String,
}

/// The checks `draft` fails, or `None` when it passes. A check Jev cannot
/// answer passes: the reply is already written.
pub async fn review(decision: &DecisionTool, draft: &Draft<'_>) -> Option<Verdict> {
    if draft.reply.trim().is_empty() {
        return None;
    }
    let open: Vec<&'static Check> = CHECKS
        .iter()
        .filter(|check| !(check.settled)(draft))
        .collect();
    if open.is_empty() {
        return None;
    }
    let questions: Vec<_> = open
        .iter()
        .map(|check| (check.key, check.question, &check.criteria))
        .collect();
    let answers = match decision.probabilities(&draft.state(), &questions).await {
        Ok(answers) => answers,
        Err(error) => {
            log::warn!("gates: Jev failed, letting the reply through: {error:#}");
            return None;
        }
    };
    let failed: Vec<&'static Check> = open
        .into_iter()
        .filter(|check| answers.get(check.key).is_some_and(|p| *p >= THRESHOLD))
        .collect();
    if failed.is_empty() {
        return None;
    }
    let keys: Vec<_> = failed.iter().map(|check| check.key).collect();
    log::info!("gates: sending the reply back; failed={}", keys.join(","));
    Some(Verdict {
        failed: keys,
        note: note(&failed),
    })
}

fn note(failed: &[&Check]) -> String {
    let mut text = PREFACE.to_owned();
    for check in failed {
        let _ = write!(text, "\n- {}", check.fix);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, success: bool) -> Call {
        Call {
            name: name.into(),
            success,
        }
    }

    #[test]
    fn a_shown_map_or_a_schedule_settles_its_check() {
        let calls = [call("open_maps", true), call("schedule_once", false)];
        let draft = Draft {
            asked: "Where should I go?",
            reply: "Head to the Gucci flagship.",
            calls: &calls,
        };
        let settled: Vec<_> = CHECKS
            .iter()
            .filter(|check| (check.settled)(&draft))
            .map(|check| check.key)
            .collect();
        assert_eq!(settled, ["map"]);
    }

    #[test]
    fn the_state_lists_every_call_with_its_outcome() {
        let calls = [call("open_maps", true), call("web_search", false)];
        let draft = Draft {
            asked: "  Where now? ",
            reply: "Go to Isetan.\n",
            calls: &calls,
        };
        let state = draft.state();
        assert!(state.contains("Asked:\nWhere now?\n"));
        assert!(state.contains("- open_maps (succeeded)\n- web_search (failed)\n"));
        assert!(state.ends_with("Beep's reply:\nGo to Isetan."));
        let none = Draft {
            calls: &[],
            ..draft
        };
        assert!(none.state().contains("Tool calls this turn:\n- none\n"));
    }

    #[test]
    fn the_note_names_each_failed_fix() {
        let note = note(&[&CHECKS[0], &CHECKS[2]]);
        assert!(note.starts_with(PREFACE));
        assert!(note.contains("open_maps"));
        assert!(note.contains("schedule_once"));
        assert!(!note.contains("no tool call did"));
    }
}
