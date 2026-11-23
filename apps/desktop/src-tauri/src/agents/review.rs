//! Review with agent: a temporary reviewer the node of a checkout starts, a
//! child of the session whose changes it reviews when there is one, and the
//! review it hands in. The window reads the review, never the reviewer.

use crate::{
    error::{DesktopError, Result},
    fleet::{Fleet, Machines},
};
use beep_mesh::machine::sessions::{self, NodeWork};
use beep_proto::SessionId;
use beep_proto::{
    ApiError,
    node::agent::{Effort, ReviewOrder, ReviewRun},
};
use iroh::EndpointId;
use std::time::Duration;
use tokio::time::timeout;

const REVIEW_CALL_LIMIT: Duration = Duration::from_secs(5);
const RUNNING_REVIEWS_FRESH: Duration = Duration::from_secs(2);
const IDLE_REVIEWS_FRESH: Duration = Duration::from_secs(10);

/// The mesh machine `machine_id` names: reviews run only there.
fn member(fleet: &Fleet, machine_id: &str) -> Result<EndpointId> {
    fleet.member(machine_id).ok_or_else(|| {
        ApiError::bad_request("Reviews run on the machines of your mesh only").into()
    })
}

fn done<T>(fleet: &Fleet, machine: EndpointId, work: NodeWork<T>) -> Result<T> {
    match work {
        NodeWork::Done(done) => Ok(done),
        NodeWork::Refused(message) => {
            Err(ApiError::bad_request(format!("{}: {message}", fleet.name_of(machine))).into())
        }
    }
}

/// Starts a reviewer of the changes in `cwd` against `base` (`HEAD` for
/// the uncommitted ones), as a child of `parent` when the changes are a
/// session's.
// The arguments are the fields of the window's `invoke` call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_review_start(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
    cwd: String,
    base: String,
    parent: Option<SessionId>,
    model: Option<String>,
    effort: Option<Effort>,
    fast: Option<bool>,
) -> Result<ReviewRun> {
    let machine = member(&fleet, &machine_id)?;
    let client = timeout(REVIEW_CALL_LIMIT, fleet.client(machine))
        .await
        .map_err(|_| {
            DesktopError::TimedOut(format!(
                "{} did not answer in time.",
                fleet.name_of(machine)
            ))
        })??;
    let parent_key = parent.clone();
    let order = ReviewOrder {
        cwd,
        agent,
        model,
        effort,
        fast,
        base,
        focus: String::new(),
        parent,
    };
    let started = done(
        &fleet,
        machine,
        timeout(REVIEW_CALL_LIMIT, sessions::start_review(&client, order))
            .await
            .map_err(|_| {
                DesktopError::TimedOut(format!(
                    "{} did not answer in time.",
                    fleet.name_of(machine)
                ))
            })??,
    )?;

    if let Some(parent) = parent_key {
        fleet.projections().reviews.forget(&(machine, parent));
    }

    Ok(started)
}

/// How the review `id` stands.
#[tauri::command]
pub async fn agent_review(
    fleet: Machines<'_>,
    machine_id: String,
    id: SessionId,
) -> Result<ReviewRun> {
    let machine = member(&fleet, &machine_id)?;
    let client = timeout(REVIEW_CALL_LIMIT, fleet.client(machine))
        .await
        .map_err(|_| {
            DesktopError::TimedOut(format!(
                "{} did not answer in time.",
                fleet.name_of(machine)
            ))
        })??;
    let run = done(
        &fleet,
        machine,
        timeout(REVIEW_CALL_LIMIT, sessions::review(&client, &id))
            .await
            .map_err(|_| {
                DesktopError::TimedOut(format!(
                    "{} did not answer in time.",
                    fleet.name_of(machine)
                ))
            })??,
    )?;

    if let Some(parent) = &run.parent {
        fleet
            .projections()
            .reviews
            .edit(&(machine, parent.clone()), |list| {
                if let Some(existing) = list.iter_mut().find(|r| r.id == run.id) {
                    *existing = run.clone();
                } else {
                    list.push(run.clone());
                }
            });
    }

    Ok(run)
}

/// The reviews of `session`'s work, its own `review` calls among them.
#[tauri::command]
pub async fn agent_reviews(
    fleet: Machines<'_>,
    machine_id: String,
    session: SessionId,
) -> Result<Vec<ReviewRun>> {
    let Some(machine) = fleet.member(&machine_id) else {
        return Ok(Vec::new());
    };
    let fleet_arc = fleet.inner().clone();
    let session_key = session.clone();
    let fresh = |runs: &Vec<ReviewRun>| {
        if runs.iter().any(|run| run.review.is_none()) {
            RUNNING_REVIEWS_FRESH
        } else {
            IDLE_REVIEWS_FRESH
        }
    };
    let read = fleet
        .projections()
        .reviews
        .read_fast_with_fresh(
            (machine, session_key),
            fresh,
            PartialEq::eq,
            || {},
            move || async move {
                let client = fleet_arc.client(machine).await?;
                Ok(sessions::reviews(&client, &session).await?)
            },
        )
        .await;
    Ok(match read {
        Some(Ok(runs)) => runs,
        Some(Err(_)) | None => Vec::new(),
    })
}
