//! Follow-up replies shown under the assistant's reply. Jev decides
//! whether the reply invites one, then one plain completion writes them as a
//! Markdown list, one follow-up per line.
use crate::{
    decision::{Criteria, DecisionTool},
    memories::Turn,
    starters::{Generator, list_items},
    tools::replies_output,
};

/// How likely a follow-up has to be useful before one is written.
const THRESHOLD: f64 = 0.8;

const QUESTION: &str = "Would the user likely want tappable follow-ups under this reply: a question to dig deeper into the topic, or something the assistant could do next with what it just said?";

const CRITERIA: Criteria = Criteria {
    yes: "The reply explains a topic, answers a factual question, recommends or compares things, or finds something that leads to an action like a reminder, directions, a calendar event or a search.",
    no: "Small talk, thanks or goodbyes, a finished task with nothing left to do, or a reply that already asks the user a question.",
};

const SYSTEM: &str = "You write follow-ups, shown as tappable chips under an assistant's \
reply. Answer with a Markdown list of 2 or 3 items, one per line starting with \"- \", and \
nothing else. Write 3 only when each one is clearly worth tapping. Each item is a message the \
user would send next, in the user's language, at most 32 characters, no quotes. Make them \
differ: dig deeper into the main point, ask the assistant to do something with it (search the \
web, set a reminder, add a calendar event, show a place on the map, check mail), or ask what to \
do next. Do not ask about what the reply already says, and do not drift to other topics from \
earlier in the chat.";

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
    let answer = match generator.text(SYSTEM, state).await {
        Ok(answer) => answer,
        Err(error) => {
            log::warn!("unable to write follow-ups: {error}");
            return None;
        }
    };
    let replies = list_items(&answer)
        .into_iter()
        .map(|item| item.trim_matches(['"', '\'', '“', '”']).trim().to_owned())
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
