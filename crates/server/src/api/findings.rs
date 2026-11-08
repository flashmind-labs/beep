//! Findings: where the improvement watcher saw Beep fail the user.

use super::Reply;
use crate::{
    account::user::User,
    api::router::Cx,
    app::App,
    memory::watchers,
    protocol::{ApiError, Finding, FindingStatus, ListFindings, UpdateFinding},
    store::FindingRow,
};
use chrono::Utc;
use uuid::Uuid;

/// The most findings one list holds.
const LIMIT: i64 = 200;

pub(super) async fn list_findings(
    cx: Cx,
    ListFindings { status }: ListFindings,
) -> Reply<Vec<Finding>> {
    list(&cx.app, &cx.user, status).await
}

pub(super) async fn update_finding(
    cx: Cx,
    UpdateFinding { id, status }: UpdateFinding,
) -> Reply<Finding> {
    update(&cx.app, &cx.user, &id, status).await
}

async fn list(app: &App, user: &User, status: Option<FindingStatus>) -> Reply<Vec<Finding>> {
    let rows = app
        .store
        .findings(user.id(), status.map(stored), LIMIT)
        .await?;
    Ok(rows.into_iter().map(listed).collect())
}

async fn update(app: &App, user: &User, id: &str, status: FindingStatus) -> Reply<Finding> {
    let id = Uuid::parse_str(id.trim()).map_err(|_| ApiError::NotFound {})?;
    app.store
        .set_finding_status(user.id(), id, stored(status), Utc::now())
        .await?
        .map(listed)
        .ok_or(ApiError::NotFound {})
}

fn stored(status: FindingStatus) -> watchers::FindingStatus {
    match status {
        FindingStatus::Open {} => watchers::FindingStatus::Open,
        FindingStatus::Resolved {} => watchers::FindingStatus::Resolved,
        FindingStatus::Dismissed {} => watchers::FindingStatus::Dismissed,
    }
}

fn listed(row: FindingRow) -> Finding {
    let status = match row.status {
        watchers::FindingStatus::Open => FindingStatus::Open {},
        watchers::FindingStatus::Resolved => FindingStatus::Resolved {},
        watchers::FindingStatus::Dismissed => FindingStatus::Dismissed {},
    };
    let kind: &str = row.kind.into();
    let cause: &str = row.cause.into();
    Finding {
        id: row.id.to_string(),
        kind: kind.to_owned(),
        summary: row.summary,
        steps: row.steps,
        cause: cause.to_owned(),
        cause_detail: row.cause_detail,
        fix: row.fix,
        status,
        seen: u32::try_from(row.seen).unwrap_or(0),
        first_seen: row.first_seen.into(),
        last_seen: row.last_seen.into(),
        session_id: row.sighting.as_ref().map(|seen| seen.chat),
        first_message: row.sighting.as_ref().map_or(0, |seen| seen.first_seq),
        last_message: row.sighting.as_ref().map_or(0, |seen| seen.last_seq),
    }
}

#[cfg(test)]
#[path = "../tests/api_findings.rs"]
mod tests;
