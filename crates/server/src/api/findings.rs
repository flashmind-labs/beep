//! Findings: where Beep failed users, as the improvement watcher found it.
//! Admins only.

use super::Reply;
use crate::{
    app::App,
    protocol::{ApiError, Finding, FindingStatus, ListFindings, UpdateFinding},
    router::Cx,
    store::FindingRow,
    user::User,
    watchers,
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

fn admin(app: &App, user: &User) -> Reply<()> {
    if app.models.is_admin(user.id()) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized {})
    }
}

async fn list(app: &App, user: &User, status: Option<FindingStatus>) -> Reply<Vec<Finding>> {
    admin(app, user)?;
    let rows = app
        .store
        .findings(user.id(), status.map(stored), LIMIT)
        .await?;
    Ok(rows.into_iter().map(listed).collect())
}

async fn update(app: &App, user: &User, id: &str, status: FindingStatus) -> Reply<Finding> {
    admin(app, user)?;
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
mod tests {
    use super::*;
    use crate::{
        store::{NewFinding, Sighting, test_chat},
        watchers::{
            Cause, FindingKind,
            testing::{Harness, Scripted},
        },
    };

    #[tokio::test]
    async fn only_admins_read_and_settle_findings() {
        let h = Harness::new(Scripted::default()).await;
        let chat = test_chat("finding");
        let last = h.turn(&h.alice, chat, "no, I meant tomorrow").await;
        let new = NewFinding {
            fingerprint: "correction:system-prompt:relative-dates",
            kind: FindingKind::Correction,
            summary: "Booked today instead of tomorrow.",
            steps: "1. Ask to book something tomorrow.",
            cause: Cause::Prompt,
            cause_detail: "Relative dates are not resolved.",
            fix: "Resolve relative dates first.",
        };
        let seen = Sighting {
            user: "alice",
            chat,
            first_seq: last - 1,
            last_seq: last,
        };
        let id = h
            .app
            .store
            .record_finding(&new, &seen, Utc::now())
            .await
            .unwrap();

        assert_eq!(
            list(&h.app, &h.bob, None).await.unwrap_err(),
            ApiError::Unauthorized {}
        );
        assert_eq!(
            update(&h.app, &h.bob, &id.to_string(), FindingStatus::Dismissed {})
                .await
                .unwrap_err(),
            ApiError::Unauthorized {}
        );

        let listed = list(&h.app, &h.alice, Some(FindingStatus::Open {}))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, "correction");
        assert_eq!(listed[0].cause, "prompt");
        assert_eq!(listed[0].session_id, Some(chat));
        assert_eq!(
            (listed[0].first_message, listed[0].last_message),
            (last - 1, last)
        );

        let resolved = update(
            &h.app,
            &h.alice,
            &id.to_string(),
            FindingStatus::Resolved {},
        )
        .await
        .unwrap();
        assert_eq!(resolved.status, FindingStatus::Resolved {});
        assert!(
            list(&h.app, &h.alice, Some(FindingStatus::Open {}))
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            update(&h.app, &h.alice, "not-an-id", FindingStatus::Open {})
                .await
                .unwrap_err(),
            ApiError::NotFound {}
        );
    }
}
