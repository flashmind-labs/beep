//! A chat's background work, for the apps to show while it runs.

use super::Reply;
use crate::{
    protocol::{ApiError, CancelWork, ListWork, WorkItem, WorkKind, WorkOutcome},
    router::Cx,
    subagents::work::{self, Listing, WorkId},
};
use std::time::SystemTime;

pub(super) async fn list_work(cx: Cx, ListWork { session_id }: ListWork) -> Reply<Vec<WorkItem>> {
    let listed = cx.app.subagents.work.list(cx.user.id(), session_id);
    Ok(listed.into_iter().map(item).collect())
}

pub(super) async fn cancel_work(cx: Cx, CancelWork { session_id, id }: CancelWork) -> Reply<bool> {
    let id: WorkId = id.parse().map_err(|_| ApiError::NotFound {})?;
    cx.app
        .subagents
        .work
        .cancel(cx.user.id(), session_id, id)
        .map_err(|_| ApiError::NotFound {})
}

fn item(listing: Listing) -> WorkItem {
    WorkItem {
        id: listing.id.to_string(),
        kind: match listing.kind {
            work::WorkKind::Agent => WorkKind::Agent {},
            work::WorkKind::Job => WorkKind::Job {},
        },
        task: listing.task,
        started_at: SystemTime::from(listing.started),
        step: listing.step,
        outcome: listing.outcome.map(|outcome| WorkOutcome {
            ok: outcome.ok,
            text: outcome.text,
        }),
    }
}
