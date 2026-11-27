//! `review` and `submit_review`: an agent has its changes reviewed by a
//! temporary reviewer it starts as a child (`agents::reviews`), and the
//! reviewer hands its findings in. Both run on the caller's own node.

use super::Node;
use crate::agents::reply::{fail, json_done, ok, parse};
use crate::agents::{Agents, family::WAIT_LIMIT, reviews::MAX_FINDINGS};
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{ReviewOrder, ReviewRun, SubmittedReview},
    tools::{Reply, Request},
};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

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
pub(crate) struct ReviewView {
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
pub(super) async fn review(node: &Node, request: &Request) -> Reply {
    run_review(node, request).await.unwrap_or_else(fail)
}

/// Answers `submit_review` for the session that called it.
pub(super) fn submit(node: &Node, request: &Request) -> Reply {
    submit_review(node, request).unwrap_or_else(fail)
}

/// How `run` stands, as `review` and `agent_status` answer it.
pub(super) fn standing(run: ReviewRun) -> Reply {
    answer(&ReviewView::from(run))
}

async fn run_review(node: &Node, request: &Request) -> Result<Reply, String> {
    let args: ReviewArgs = parse(&request.args)?;
    let agents = &node.agents;
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
                .reviews
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

    Ok(answer(&waited(&node.agents, &who, run).await))
}

fn answer(view: &ReviewView) -> Reply {
    json_done(view, view.state == "failed")
}

/// Waits for the review to end, at most `WAIT_LIMIT`, as `start_agent`'s
/// `wait` does: a tool call must come back before the agent's client gives up.
pub(crate) async fn waited(
    agents: &Agents,
    who: &crate::agents::family::Who,
    run: ReviewRun,
) -> ReviewView {
    match timeout(WAIT_LIMIT, agents.reviews.review_ended(who, &run.id)).await {
        Ok(ended) => ReviewView::from(ended.unwrap_or(run)),
        Err(_) => {
            let mut view = ReviewView::from(agents.reviews.review_for(who, &run.id).unwrap_or(run));
            view.note = Some(format!(
                "Still reviewing after {} seconds. Call review with this id to check again.",
                WAIT_LIMIT.as_secs()
            ));
            view
        }
    }
}

fn submit_review(node: &Node, request: &Request) -> Result<Reply, String> {
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

    node.agents
        .reviews
        .submit_review(&request.session, review)?;

    Ok(ok("Submitted. The review is done: end your turn."))
}

#[cfg(test)]
#[path = "../tests/tools_review.rs"]
mod tests;
