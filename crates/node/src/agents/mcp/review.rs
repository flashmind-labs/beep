//! `review` and `submit_review`: an agent has its changes reviewed by a
//! temporary reviewer it starts as a child (`agents::reviews`), and the
//! reviewer hands its findings in. Both run on the caller's own node.

use crate::agents::{
    family::{WAIT_LIMIT, WAIT_POLL},
    reviews::MAX_FINDINGS,
};
use crate::link::Offers;
use beep_proto::node::{
    agent::{ReviewOrder, ReviewRun, SubmittedReview},
    tools::{Reply, Request, Tool},
};
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, sleep};

/// Whether `tool` is one this module answers.
pub(crate) fn handles(tool: Tool) -> bool {
    matches!(tool, Tool::Review | Tool::SubmitReview)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewArgs {
    /// A review started before, to follow.
    id: Option<String>,
    base: Option<String>,
    focus: Option<String>,
    harness: Option<String>,
    model: Option<String>,
}

/// What `review` answers, as JSON.
#[derive(Debug, Serialize)]
struct ReviewView {
    id: String,
    /// `working`, `done` or `failed`.
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<SubmittedReview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

impl From<ReviewRun> for ReviewView {
    fn from(run: ReviewRun) -> Self {
        let state = match (&run.review, &run.error) {
            (Some(_), _) => "done",
            (None, Some(_)) => "failed",
            (None, None) => "working",
        };
        Self {
            id: run.id,
            state,
            review: run.review,
            error: run.error,
            note: None,
        }
    }
}

fn fail(text: impl Into<String>) -> Reply {
    Reply::Done {
        text: text.into(),
        failed: true,
    }
}

fn ok(text: impl Into<String>) -> Reply {
    Reply::Done {
        text: text.into(),
        failed: false,
    }
}

/// Answers `review` or `submit_review` for the session that called it.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    match request.tool {
        Tool::Review => match serde_json::from_str::<ReviewArgs>(&request.args) {
            Ok(args) => review(offers, &request.session, args).await,
            Err(e) => fail(format!("The arguments do not fit: {e}")),
        },
        Tool::SubmitReview => match serde_json::from_str::<SubmittedReview>(&request.args) {
            Ok(review) => submit(offers, &request.session, review),
            Err(e) => fail(format!("The arguments do not fit: {e}")),
        },
        other => fail(format!("{other} is not a review tool.")),
    }
}

async fn review(offers: &Offers, session: &str, args: ReviewArgs) -> Reply {
    let agents = &offers.agents;
    let Some((caller, who)) = agents.caller(session) else {
        return fail("This session is not one this node runs.");
    };
    if caller.reviewer {
        return fail("A reviewer does not start reviews. Call submit_review.");
    }
    let run = match args.id {
        Some(id) => match agents.review_for(&who, id.trim()) {
            Some(run) => run,
            None => return fail(format!("You have no review {}.", id.trim())),
        },
        None => {
            let info = caller.info();
            let order = ReviewOrder {
                cwd: info.cwd,
                agent: args
                    .harness
                    .map(|harness| harness.trim().to_owned())
                    .filter(|harness| !harness.is_empty())
                    .unwrap_or(info.agent),
                model: args.model,
                effort: None,
                base: args.base.unwrap_or_default(),
                focus: args.focus.unwrap_or_default(),
                parent: Some(caller.id.clone()),
            };
            match agents.start_review(order).await {
                Ok(run) => run,
                Err(e) => return fail(e),
            }
        }
    };
    answer(&waited(offers, &who, run).await)
}

/// How `run` stands, as `review` and `agent_status` answer it.
pub(super) fn standing(run: ReviewRun) -> Reply {
    answer(&run.into())
}

fn answer(view: &ReviewView) -> Reply {
    match serde_json::to_string(view) {
        Ok(text) => Reply::Done {
            text,
            failed: view.state == "failed",
        },
        Err(e) => fail(format!("Could not write the answer: {e}")),
    }
}

/// Waits for the review to end, at most `WAIT_LIMIT`, as `start_agent`'s
/// `wait` does: a tool call must come back before the agent's client gives up.
async fn waited(offers: &Offers, who: &crate::agents::family::Who, run: ReviewRun) -> ReviewView {
    let deadline = Instant::now() + WAIT_LIMIT;
    let mut run = run;
    while run.running() {
        if Instant::now() >= deadline {
            let mut view = ReviewView::from(run);
            view.note = Some(format!(
                "Still reviewing after {} seconds. Call review with this id to check again.",
                WAIT_LIMIT.as_secs()
            ));
            return view;
        }
        sleep(WAIT_POLL).await;
        match offers.agents.review_for(who, &run.id) {
            Some(next) => run = next,
            None => break,
        }
    }
    run.into()
}

fn submit(offers: &Offers, session: &str, review: SubmittedReview) -> Reply {
    if review.summary.trim().is_empty() {
        return fail("Give a `summary`.");
    }
    if review.findings.len() > MAX_FINDINGS {
        return fail(format!(
            "Give at most {MAX_FINDINGS} findings, the ones that matter most."
        ));
    }
    if let Some(bad) = review
        .findings
        .iter()
        .find(|finding| finding.file.trim().is_empty() || finding.summary.trim().is_empty())
    {
        return fail(format!(
            "Each finding needs a `file` and a `summary`: {bad:?}"
        ));
    }
    match offers.agents.submit_review(session, review) {
        Ok(()) => ok("Submitted. The review is done: end your turn."),
        Err(e) => fail(e),
    }
}

#[cfg(test)]
mod tests;
