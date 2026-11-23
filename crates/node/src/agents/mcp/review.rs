//! `review` and `submit_review`: an agent has its changes reviewed by a
//! temporary reviewer it starts as a child (`agents::reviews`), and the
//! reviewer hands its findings in. Both run on the caller's own node.

use super::reply::{fail, json_done, ok, parse};
use crate::agents::{
    family::{WAIT_LIMIT, WAIT_POLL},
    reviews::MAX_FINDINGS,
};
use crate::link::Offers;
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{ReviewOrder, ReviewRun, SubmittedReview},
    tools::{Reply, Request},
};
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, sleep};

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
    id: SessionId,
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

/// Answers `review` for the session that called it.
pub(super) async fn review(offers: &Offers, request: &Request) -> Reply {
    run_review(offers, request).await.unwrap_or_else(fail)
}

/// Answers `submit_review` for the session that called it.
pub(super) fn submit(offers: &Offers, request: &Request) -> Reply {
    submit_review(offers, request).unwrap_or_else(fail)
}

/// How `run` stands, as `review` and `agent_status` answer it.
pub(super) fn standing(run: ReviewRun) -> Reply {
    answer(&ReviewView::from(run))
}

async fn run_review(offers: &Offers, request: &Request) -> Result<Reply, String> {
    let args: ReviewArgs = parse(&request.args)?;
    let agents = &offers.agents;
    let (caller, who) = agents
        .caller(&request.session)
        .ok_or("This session is not one this node runs.")?;

    if caller.reviewer {
        return Err("A reviewer does not start reviews. Call submit_review.".to_owned());
    }

    let run = match args.id {
        Some(id) => {
            let trimmed = id.trim();
            agents
                .review_for(&who, &Into::<SessionId>::into(trimmed))
                .ok_or_else(|| format!("You have no review {trimmed}."))?
        }
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
                fast: None,
                base: args.base.unwrap_or_default(),
                focus: args.focus.unwrap_or_default(),
                parent: Some(caller.id.clone()),
            };
            agents.start_review(order).await?
        }
    };

    Ok(answer(&waited(offers, &who, run).await))
}

fn answer(view: &ReviewView) -> Reply {
    json_done(view, view.state == "failed")
}

/// Waits for the review to end, at most `WAIT_LIMIT`, as `start_agent`'s
/// `wait` does: a tool call must come back before the agent's client gives up.
async fn waited(
    offers: &Offers,
    who: &crate::agents::family::Who,
    mut run: ReviewRun,
) -> ReviewView {
    let deadline = Instant::now() + WAIT_LIMIT;

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

    ReviewView::from(run)
}

fn submit_review(offers: &Offers, request: &Request) -> Result<Reply, String> {
    let review: SubmittedReview = parse(&request.args)?;

    if review.summary.trim().is_empty() {
        return Err("Give a `summary`.".to_owned());
    }

    if review.findings.len() > MAX_FINDINGS {
        return Err(format!(
            "Give at most {MAX_FINDINGS} findings, the ones that matter most."
        ));
    }

    if let Some(bad) = review
        .findings
        .iter()
        .find(|finding| finding.file.trim().is_empty() || finding.summary.trim().is_empty())
    {
        return Err(format!(
            "Each finding needs a `file` and a `summary`: {bad:?}"
        ));
    }

    offers.agents.submit_review(&request.session, review)?;

    Ok(ok("Submitted. The review is done: end your turn."))
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_review.rs"]
mod tests;
