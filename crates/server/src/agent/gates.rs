//! Checks a finished reply before the turn ends and sends the agent back once to fix failed checks.

use crate::agent::decision::{Criteria, DecisionTool};
use std::fmt::Write;

/// Jev's probability from which a check fails; high, since false alarms cost a pass.
const THRESHOLD: f64 = 0.6;

/// The tools that put a destination on the map.
pub const MAP_TOOLS: &[&str] = &["open_maps"];
/// The tools that make Beep do something later.
pub const SCHEDULE_TOOLS: &[&str] = &["cron_create", "cron_edit", "schedule_once", "place_routine"];

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
    /// The user's scheduled tasks, so mentioning one is not an unkept promise.
    pub scheduled: &'a [String],
    /// Whether the run has a map card to show.
    pub can_map: bool,
    /// Whether the run can schedule anything.
    pub can_schedule: bool,
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
        text.push_str("\nAlready scheduled before this turn:\n");
        if self.scheduled.is_empty() {
            text.push_str("- nothing\n");
        }
        for task in self.scheduled {
            let _ = writeln!(text, "- {task}");
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

const CHECKS: [Check; 4] = [
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
        settled: |draft| !draft.can_map || draft.called(MAP_TOOLS),
    },
    Check {
        key: "claimed",
        question: "Does Beep's reply say that Beep has already done something, such as sent, \
saved, added, booked, set up, changed or deleted something, that none of the succeeded tool \
calls listed did?",
        criteria: Criteria {
            yes: "The reply claims an action that no succeeded tool call in the list carried out.",
            no: "Every action the reply claims matches a succeeded tool call or something \
already scheduled, or the reply claims none.",
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
            yes: "The reply promises a later reminder, check or message from Beep that \
nothing already scheduled covers.",
            no: "The reply promises nothing Beep would do later on its own, or only \
mentions something already scheduled.",
        },
        fix: "Your reply promises to do something later, but nothing is scheduled. Set it \
up now with schedule_once, cron_create or place_routine, or tell the user you will not.",
        settled: |draft| !draft.can_schedule || draft.called(SCHEDULE_TOOLS),
    },
    Check {
        key: UNFINISHED,
        question: "Is Beep partway through a job it does itself with tools, such as moving, \
archiving, labelling or reading many emails or items, and does its reply say some of them \
are still left for Beep to handle?",
        criteria: Criteria {
            yes: "The reply says Beep handled only part of the items (more pages remain, some \
may still be there, it has not checked the rest, or it offers to keep going) and nothing \
stops Beep from going on now.",
            no: "Beep's job is done, the reply is advice or directions for the user to act \
on, or Beep stops for something only the user can settle, such as a confirmation before \
deleting, an app that is not connected or a real choice.",
        },
        fix: "You stopped before the task was done. Keep going now: fetch the next page or \
search again, act on it, and repeat until nothing is left. Then say in one line what you \
did in total.",
        settled: |_| false,
    },
];

/// The check for a reply that stops partway; it may send the agent back more than once.
pub const UNFINISHED: &str = "unfinished";

/// Told before the fixes, so the second pass adds to the reply.
const PREFACE: &str = "Your reply may need a fix. The user already has it, so do not \
repeat it or confirm it. Make the calls needed. Add one short sentence only when the fix \
changes a fact you told them; otherwise write nothing.";

/// A reply that failed checks.
#[derive(Debug)]
pub struct Verdict {
    /// The keys of the failed checks.
    pub failed: Vec<&'static str>,
    /// What the agent is told in its second pass.
    pub note: String,
}

impl Verdict {
    /// Whether the fix is only a missing card, so the second pass's text is dropped.
    pub fn card_only(&self) -> bool {
        self.failed == ["map"]
    }

    /// Whether the agent was sent back to finish the user's task.
    pub fn unfinished(&self) -> bool {
        self.failed.contains(&UNFINISHED)
    }
}

/// The checks `draft` fails, or `None`; an unanswered check passes.
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
            scheduled: &[],
            can_map: true,
            can_schedule: true,
        };
        let settled: Vec<_> = CHECKS
            .iter()
            .filter(|check| (check.settled)(&draft))
            .map(|check| check.key)
            .collect();
        assert_eq!(settled, ["map"]);
    }

    #[test]
    fn a_routine_is_not_asked_to_schedule() {
        let draft = Draft {
            asked: "Check whether .bitcoin is on sale.",
            reply: "Not on sale yet, I will keep checking.",
            calls: &[],
            scheduled: &[],
            can_map: true,
            can_schedule: false,
        };
        let promised = CHECKS.iter().find(|check| check.key == "promised");
        assert!((promised.expect("promised check").settled)(&draft));
    }
}
