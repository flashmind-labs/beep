//! Review with agent: a temporary reviewer the node of a checkout starts, a
//! child of the session whose changes it reviews when there is one, and the
//! review it hands in. The window reads the review, never the reviewer.

use crate::{
    error::Result,
    fleet::{Fleet, Machines},
};
use beep_mesh::machine::sessions::{self, NodeWork};
use beep_proto::{
    ApiError,
    node::agent::{Effort, ReviewOrder, ReviewRun},
};
use iroh::EndpointId;

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
    parent: Option<String>,
    model: Option<String>,
    effort: Option<Effort>,
    fast: Option<bool>,
) -> Result<ReviewRun> {
    let machine = member(&fleet, &machine_id)?;
    let client = fleet.client(machine).await?;
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
    done(
        &fleet,
        machine,
        sessions::start_review(&client, order).await?,
    )
}

/// How the review `id` stands.
#[tauri::command]
pub async fn agent_review(
    fleet: Machines<'_>,
    machine_id: String,
    id: String,
) -> Result<ReviewRun> {
    let machine = member(&fleet, &machine_id)?;
    let client = fleet.client(machine).await?;
    done(&fleet, machine, sessions::review(&client, &id).await?)
}

/// The reviews of `session`'s work, its own `review` calls among them.
#[tauri::command]
pub async fn agent_reviews(
    fleet: Machines<'_>,
    machine_id: String,
    session: String,
) -> Result<Vec<ReviewRun>> {
    let Some(machine) = fleet.member(&machine_id) else {
        return Ok(Vec::new());
    };
    let client = fleet.client(machine).await?;
    Ok(sessions::reviews(&client, &session).await?)
}
