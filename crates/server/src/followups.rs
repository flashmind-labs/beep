//! Follow-up replies shown under the assistant's reply. Jev decides
//! whether the reply invites one, then four plain completions, one per
//! angle, each write one.
use crate::{
    decision::{Criteria, DecisionTool},
    memories::Turn,
    starters::{Generator, unfence},
    tools::replies_output,
};
use futures::future::join_all;

/// How likely a follow-up has to be useful before one is written.
const THRESHOLD: f64 = 0.5;

const QUESTION: &str = "Would the user likely want tappable follow-ups under this reply: a question to dig deeper into the topic, or something the assistant could do next with what it just said?";

const CRITERIA: Criteria = Criteria {
    yes: "The reply explains a topic, answers a factual question, recommends or compares things, or finds something that leads to an action like a reminder, directions, a calendar event or a search.",
    no: "Small talk, thanks or goodbyes, a finished task with nothing left to do, or a reply that already asks the user a question.",
};

const SYSTEM: &str = "You write one follow-up, shown as a tappable chip under an assistant's \
reply. Answer with only that message, as the user would send it next, in the user's language, at \
most 40 characters. No quotes, no explanation. Do not ask about what the reply already says.";

/// One follow-up is written per angle, all at once, so they differ.
const ANGLES: [&str; 4] = [
    "Ask a question that digs deeper into the main point of the reply.",
    "Ask about a different side of the topic the reply did not cover.",
    "Ask the assistant to do something with the reply: search the web, set a reminder, add a calendar event, show a place on the map or check mail. Pick the one that fits best.",
    "Ask for something practical: an example, a comparison, or what to do next.",
];

/// The `suggest_replies` result to add under the reply, or `None` when the
/// turn does not call for one or writing it failed.
pub async fn write(
    decision: &DecisionTool,
    generator: Generator<'_>,
    state: String,
) -> Option<String> {
    let probability = match decision.probability_with(&state, QUESTION, &CRITERIA).await {
        Ok(probability) => probability,
        Err(error) => {
            log::warn!("follow-up score failed: {error}");
            return None;
        }
    };
    log::info!("follow-up score {probability:.2}");
    if probability < THRESHOLD {
        return None;
    }
    let answers = join_all(ANGLES.map(|angle| {
        let state = state.clone();
        async move { generator.text(&format!("{SYSTEM} {angle}"), state).await }
    }))
    .await;
    let replies = answers
        .into_iter()
        .filter_map(|answer| {
            answer
                .inspect_err(|error| log::warn!("unable to write a follow-up: {error}"))
                .ok()
        })
        .filter_map(|answer| reply(&answer))
        .collect();
    let output = replies_output(replies);
    if output.is_none() {
        log::warn!("fewer than two usable follow-ups");
    }
    output
}

/// What both Jev and the writer read: the last earlier turn for context,
/// then the latest exchange.
pub fn render(earlier: &[Turn], latest: &Turn) -> String {
    match earlier.last() {
        Some(turn) => format!("Earlier:\n{turn}\n\nNow:\n{latest}"),
        None => latest.to_string(),
    }
}

/// The follow-up in a writer's answer: its first non-empty line, without a
/// list marker or quotes.
fn reply(answer: &str) -> Option<String> {
    let line = unfence(answer)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let line = strip_marker(line)
        .trim_matches(['"', '\'', '“', '”'])
        .trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// `line` without a leading `-`, `*`, `•`, `1.` or `1)`.
fn strip_marker(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix(['-', '*', '•']) {
        return rest.trim_start();
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    match line[digits..].strip_prefix(['.', ')']) {
        Some(rest) if digits > 0 => rest.trim_start(),
        _ => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_reply_out_of_the_answer() {
        assert_eq!(
            reply("How are they priced?").as_deref(),
            Some("How are they priced?")
        );
        assert_eq!(reply("\n\"Show me one\"\n").as_deref(), Some("Show me one"));
        assert_eq!(reply("1. Who built it?").as_deref(), Some("Who built it?"));
        assert_eq!(
            reply("3 ideas for dinner").as_deref(),
            Some("3 ideas for dinner")
        );
        assert_eq!(
            reply("- Remind me tomorrow").as_deref(),
            Some("Remind me tomorrow")
        );
        assert!(reply("  \n").is_none());
    }
}
