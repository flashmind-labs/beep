//! Account events, the topics each user gets, their Composio triggers and
//! where each poll stopped.

use super::{Store, seconds};
use crate::{
    events::{AppEvent, EventApp, EventKind, Topic, Urgency, Verdict},
    user::UserKey,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::{collections::HashMap, str::FromStr, time::Duration};
use strum::{EnumString, IntoStaticStr};

/// Where an event is in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EventState {
    /// Stored, waiting for the next triage.
    Pending,
    /// Handed to the triage agent, which has not decided yet.
    Triaging,
    /// Decided on by the triage agent; its decision says what became of it.
    Triaged,
    /// Handed to the agent.
    Woken,
    /// Kept by the agent for a later digest.
    Held,
    /// Handed to the agent again in a digest.
    Digested,
}

impl EventState {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// A stored event as the agent is told about it.
#[derive(Debug, Clone)]
pub struct EventRow {
    pub id: i64,
    pub app: EventApp,
    pub kind: EventKind,
    pub summary: String,
    pub detail: Option<String>,
    pub ids: Value,
    pub occurred: DateTime<Utc>,
    pub state: EventState,
}

/// A triage decision as the log keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct TriageRow {
    pub id: i64,
    pub decided_at: DateTime<Utc>,
    pub verdict: Verdict,
    /// For an inform.
    pub urgency: Option<Urgency>,
    /// What the main agent is told, for an inform.
    pub summary: Option<String>,
    /// Why it informs or ignores, or why the run failed.
    pub reason: String,
    /// The events the summary is about.
    pub refs: Vec<i64>,
    /// Whether an inform reached the main agent.
    pub delivered: bool,
}

/// An inform waiting for the user to see it, as the apps list it.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateRow {
    pub id: i64,
    pub decided_at: DateTime<Utc>,
    pub summary: String,
    /// The app of the first event it covers; `None` when those are gone.
    pub app: Option<EventApp>,
}

/// A decision to log, before it has an id.
#[derive(Debug, Clone)]
pub struct NewTriage<'a> {
    pub verdict: Verdict,
    pub urgency: Option<Urgency>,
    pub summary: Option<&'a str>,
    pub reason: &'a str,
    pub refs: &'a [i64],
}

/// An event with the triage decision that covered it, if any yet.
#[derive(Debug, Clone)]
pub struct RecentEvent {
    pub event: EventRow,
    pub triage: Option<TriageRow>,
}

/// A Composio trigger instance made for one of a user's accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerRow {
    pub slug: String,
    pub account_id: String,
    pub trigger_id: String,
}

impl Store {
    /// Stores `event` and returns its id, or `None` when the same happening
    /// was stored before.
    pub(crate) async fn insert_event(
        &self,
        user: &UserKey,
        event: &AppEvent,
    ) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar(
            "INSERT INTO app_events
                 (user_id, app, kind, dedupe_key, summary, detail, ids, raw, source, occurred_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id, dedupe_key) DO NOTHING
             RETURNING id",
        )
        .bind(user.as_str())
        .bind(event.app.slug())
        .bind(event.kind.as_str())
        .bind(event.key.as_str())
        .bind(&event.summary)
        .bind(event.detail.as_deref())
        .bind(&event.ids)
        .bind(&event.raw)
        .bind(event.source.as_str())
        .bind(event.occurred)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Up to `limit` events in `state`, oldest first, only those received
    /// at least `age` ago.
    pub(crate) async fn events_in(
        &self,
        user: &UserKey,
        state: EventState,
        age: Duration,
        limit: i64,
    ) -> Result<Vec<EventRow>> {
        let rows = sqlx::query(
            "SELECT id, app, kind, summary, detail, ids, occurred_at, state FROM app_events
             WHERE user_id=$1 AND state=$2 AND received_at <= now() - make_interval(secs => $3)
             ORDER BY received_at, id LIMIT $4",
        )
        .bind(user.as_str())
        .bind(state.as_str())
        .bind(seconds(age))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(event_row).collect()
    }

    /// Logs a triage decision about `events` and moves those still being
    /// triaged to triaged, under it. Returns the decision's id.
    pub(crate) async fn insert_triage(
        &self,
        user: &UserKey,
        triage: &NewTriage<'_>,
        events: &[i64],
    ) -> Result<i64> {
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO event_triage (user_id, decision, urgency, summary, reason, refs)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(user.as_str())
        .bind(triage.verdict.as_str())
        .bind(triage.urgency.map(Urgency::as_str))
        .bind(triage.summary)
        .bind(triage.reason)
        .bind(triage.refs)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE app_events SET state=$4, state_at=now(), triage_id=$5
             WHERE user_id=$1 AND id = ANY($2) AND state=$3",
        )
        .bind(user.as_str())
        .bind(events)
        .bind(EventState::Triaging.as_str())
        .bind(EventState::Triaged.as_str())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Informs not yet handed to the main agent, decided after `since`,
    /// oldest first.
    pub(crate) async fn undelivered_informs(
        &self,
        user: &UserKey,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<TriageRow>> {
        let rows = sqlx::query(
            "SELECT id, decided_at, decision, urgency, summary, reason, refs, delivered_at
             FROM event_triage
             WHERE user_id=$1 AND decision=$2 AND delivered_at IS NULL AND decided_at > $3
             ORDER BY decided_at, id LIMIT $4",
        )
        .bind(user.as_str())
        .bind(Verdict::Inform.as_str())
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(triage_row).collect()
    }

    /// Informs not yet delivered, decided after `since`, newest first,
    /// each with the app it came from.
    pub(crate) async fn waiting_updates(
        &self,
        user: &UserKey,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<UpdateRow>> {
        let rows = sqlx::query(
            "SELECT t.id, t.decided_at, COALESCE(t.summary, '') AS summary,
                    (SELECT e.app FROM app_events e
                     WHERE e.user_id=t.user_id AND e.id = ANY(t.refs)
                     ORDER BY e.id LIMIT 1) AS app
             FROM event_triage t
             WHERE t.user_id=$1 AND t.decision=$2 AND t.delivered_at IS NULL
                 AND t.decided_at > $3
             ORDER BY t.decided_at DESC, t.id DESC LIMIT $4",
        )
        .bind(user.as_str())
        .bind(Verdict::Inform.as_str())
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(update_row).collect()
    }

    /// Marks informs as handed to the main agent, in run `run_id` if a wake.
    pub(crate) async fn mark_delivered(
        &self,
        user: &UserKey,
        ids: &[i64],
        run_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE event_triage SET delivered_at=now(), run_id=$3
             WHERE user_id=$1 AND id = ANY($2) AND delivered_at IS NULL",
        )
        .bind(user.as_str())
        .bind(ids)
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Puts events a triage was working on when the server stopped back in
    /// the queue, for every user.
    pub(crate) async fn requeue_triaging(&self) -> Result<u64> {
        Ok(sqlx::query("UPDATE app_events SET state=$2 WHERE state=$1")
            .bind(EventState::Triaging.as_str())
            .bind(EventState::Pending.as_str())
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    /// Puts `user`'s events back in the queue that went to triage more than
    /// `after` ago: their run died without settling them.
    pub(crate) async fn requeue_stale_triaging(
        &self,
        user: &UserKey,
        after: Duration,
    ) -> Result<u64> {
        Ok(sqlx::query(
            "UPDATE app_events SET state=$3, state_at=now()
             WHERE user_id=$1 AND state=$2 AND state_at < now() - $4 * interval '1 second'",
        )
        .bind(user.as_str())
        .bind(EventState::Triaging.as_str())
        .bind(EventState::Pending.as_str())
        .bind(seconds(after))
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    /// Events received since `since`, newest first, each with the triage
    /// decision that covered it.
    pub(crate) async fn recent_events(
        &self,
        user: &UserKey,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<RecentEvent>> {
        let rows = sqlx::query(
            "SELECT e.id, e.app, e.kind, e.summary, e.detail, e.ids, e.occurred_at, e.state,
                    t.id AS t_id, t.decided_at, t.decision, t.urgency, t.summary AS t_summary,
                    t.reason, t.refs, t.delivered_at
             FROM app_events e LEFT JOIN event_triage t ON t.id = e.triage_id
             WHERE e.user_id=$1 AND e.received_at >= $2
             ORDER BY e.received_at DESC, e.id DESC LIMIT $3",
        )
        .bind(user.as_str())
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let triage_id: Option<i64> = row.try_get("t_id")?;
                let triage = match triage_id {
                    Some(id) => Some(triage_fields(row, id, "t_summary")?),
                    None => None,
                };
                Ok(RecentEvent {
                    event: event_row(row)?,
                    triage,
                })
            })
            .collect()
    }

    /// Moves those of `ids` still in `from` to `to`, and returns how many moved.
    pub(crate) async fn move_events(
        &self,
        user: &UserKey,
        ids: &[i64],
        from: EventState,
        to: EventState,
        run_id: Option<&str>,
    ) -> Result<u64> {
        Ok(sqlx::query(
            "UPDATE app_events SET state=$4, state_at=now(), run_id=COALESCE($5, run_id)
             WHERE user_id=$1 AND id = ANY($2) AND state=$3",
        )
        .bind(user.as_str())
        .bind(ids)
        .bind(from.as_str())
        .bind(to.as_str())
        .bind(run_id)
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    /// Forgets events received and triage decisions made before `before`.
    pub(crate) async fn prune_events(&self, user: &UserKey, before: DateTime<Utc>) -> Result<u64> {
        sqlx::query("DELETE FROM event_triage WHERE user_id=$1 AND decided_at < $2")
            .bind(user.as_str())
            .bind(before)
            .execute(&self.pool)
            .await?;
        Ok(
            sqlx::query("DELETE FROM app_events WHERE user_id=$1 AND received_at < $2")
                .bind(user.as_str())
                .bind(before)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    /// The topics the user chose, on or off. Others keep their default.
    pub(crate) async fn event_topics(&self, user: &UserKey) -> Result<HashMap<Topic, bool>> {
        let rows = sqlx::query("SELECT topic, enabled FROM event_topics WHERE user_id=$1")
            .bind(user.as_str())
            .fetch_all(&self.pool)
            .await?;
        let mut topics = HashMap::new();
        for row in rows {
            let topic: String = row.try_get("topic")?;
            // A topic a later version dropped is ignored.
            if let Ok(topic) = Topic::from_str(&topic) {
                topics.insert(topic, row.try_get("enabled")?);
            }
        }
        Ok(topics)
    }

    pub(crate) async fn set_event_topic(
        &self,
        user: &UserKey,
        topic: Topic,
        enabled: bool,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO event_topics (user_id, topic, enabled) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, topic) DO UPDATE SET enabled=excluded.enabled",
        )
        .bind(user.as_str())
        .bind(topic.as_str())
        .bind(enabled)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn save_event_trigger(&self, user: &UserKey, row: &TriggerRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO event_triggers (user_id, slug, account_id, trigger_id)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, slug, account_id) DO UPDATE SET trigger_id=excluded.trigger_id",
        )
        .bind(user.as_str())
        .bind(&row.slug)
        .bind(&row.account_id)
        .bind(&row.trigger_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn event_triggers(&self, user: &UserKey) -> Result<Vec<TriggerRow>> {
        let rows = sqlx::query(
            "SELECT slug, account_id, trigger_id FROM event_triggers WHERE user_id=$1
             ORDER BY slug, account_id",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(TriggerRow {
                    slug: row.try_get("slug")?,
                    account_id: row.try_get("account_id")?,
                    trigger_id: row.try_get("trigger_id")?,
                })
            })
            .collect()
    }

    pub(crate) async fn delete_event_trigger(
        &self,
        user: &UserKey,
        row: &TriggerRow,
    ) -> Result<()> {
        sqlx::query("DELETE FROM event_triggers WHERE user_id=$1 AND slug=$2 AND account_id=$3")
            .bind(user.as_str())
            .bind(&row.slug)
            .bind(&row.account_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Where the poll of `source` last stopped.
    pub(crate) async fn polled_at(
        &self,
        user: &UserKey,
        source: &str,
    ) -> Result<Option<DateTime<Utc>>> {
        Ok(
            sqlx::query_scalar("SELECT polled_at FROM event_polls WHERE user_id=$1 AND source=$2")
                .bind(user.as_str())
                .bind(source)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub(crate) async fn mark_polled(
        &self,
        user: &UserKey,
        source: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO event_polls (user_id, source, polled_at) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, source) DO UPDATE SET polled_at=excluded.polled_at",
        )
        .bind(user.as_str())
        .bind(source)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn event_row(row: &PgRow) -> Result<EventRow> {
    let app: String = row.try_get("app")?;
    let kind: String = row.try_get("kind")?;
    let state: String = row.try_get("state")?;
    Ok(EventRow {
        id: row.try_get("id")?,
        app: EventApp::from_str(&app).with_context(|| format!("event app {app}"))?,
        kind: EventKind::from_str(&kind).with_context(|| format!("event kind {kind}"))?,
        summary: row.try_get("summary")?,
        detail: row.try_get("detail")?,
        ids: row.try_get("ids")?,
        occurred: row.try_get("occurred_at")?,
        state: EventState::from_str(&state).with_context(|| format!("event state {state}"))?,
    })
}

fn update_row(row: &PgRow) -> Result<UpdateRow> {
    let app: Option<String> = row.try_get("app")?;
    Ok(UpdateRow {
        id: row.try_get("id")?,
        decided_at: row.try_get("decided_at")?,
        summary: row.try_get("summary")?,
        app: app
            .map(|app| EventApp::from_str(&app).with_context(|| format!("event app {app}")))
            .transpose()?,
    })
}

fn triage_row(row: &PgRow) -> Result<TriageRow> {
    triage_fields(row, row.try_get("id")?, "summary")
}

/// A triage decision from `row`, whose summary column is `summary`.
fn triage_fields(row: &PgRow, id: i64, summary: &str) -> Result<TriageRow> {
    let verdict: String = row.try_get("decision")?;
    let urgency: Option<String> = row.try_get("urgency")?;
    let delivered: Option<DateTime<Utc>> = row.try_get("delivered_at")?;
    Ok(TriageRow {
        id,
        decided_at: row.try_get("decided_at")?,
        verdict: Verdict::from_str(&verdict).with_context(|| format!("verdict {verdict}"))?,
        urgency: urgency
            .map(|urgency| {
                Urgency::from_str(&urgency).with_context(|| format!("urgency {urgency}"))
            })
            .transpose()?,
        summary: row.try_get(summary)?,
        reason: row.try_get("reason")?,
        refs: row.try_get("refs")?,
        delivered: delivered.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::{DedupeKey, Source},
        store::TestDb,
    };
    use serde_json::json;

    fn mail(id: &str, source: Source) -> AppEvent {
        AppEvent {
            app: EventApp::Gmail,
            kind: EventKind::NewMail,
            key: DedupeKey::new(&["gmail", id]),
            summary: format!("From someone: {id}"),
            detail: None,
            ids: json!({"message_id": id}),
            raw: json!({"id": id}),
            source,
            occurred: Utc::now(),
        }
    }

    #[tokio::test]
    async fn the_same_happening_is_stored_once_however_it_arrives() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        let first = store
            .insert_event(&alice, &mail("m1", Source::Trigger))
            .await;
        assert!(first.unwrap().is_some());
        let again = store.insert_event(&alice, &mail("m1", Source::Poll)).await;
        assert!(again.unwrap().is_none());
        // Another user's copy is their own.
        let theirs = store.insert_event(&bob, &mail("m1", Source::Poll)).await;
        assert!(theirs.unwrap().is_some());

        let pending = store
            .events_in(&alice, EventState::Pending, Duration::ZERO, 10)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, EventKind::NewMail);
        assert_eq!(pending[0].ids["message_id"], "m1");

        let ids = [pending[0].id];
        let moved = store
            .move_events(
                &alice,
                &ids,
                EventState::Pending,
                EventState::Woken,
                Some("r1"),
            )
            .await
            .unwrap();
        assert_eq!(moved, 1);
        let moved = store
            .move_events(&bob, &ids, EventState::Woken, EventState::Held, None)
            .await
            .unwrap();
        assert_eq!(moved, 0, "another user's events stay");
        let moved = store
            .move_events(&alice, &ids, EventState::Pending, EventState::Held, None)
            .await
            .unwrap();
        assert_eq!(moved, 0, "only events still pending move");
    }

    #[tokio::test]
    async fn a_triage_that_never_settled_goes_back_in_the_queue() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let id = store
            .insert_event(&alice, &mail("m1", Source::Trigger))
            .await
            .unwrap()
            .unwrap();
        store
            .move_events(
                &alice,
                &[id],
                EventState::Pending,
                EventState::Triaging,
                None,
            )
            .await
            .unwrap();
        let hour = Duration::from_secs(60 * 60);
        assert_eq!(
            store.requeue_stale_triaging(&alice, hour).await.unwrap(),
            0,
            "a triage still in its time is left alone"
        );
        assert_eq!(
            store
                .requeue_stale_triaging(&UserKey::for_test("bob"), Duration::ZERO)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .requeue_stale_triaging(&alice, Duration::ZERO)
                .await
                .unwrap(),
            1
        );
        let pending = store
            .events_in(&alice, EventState::Pending, Duration::ZERO, 10)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
    }

    /// Logs an inform about a new event of `user`'s, as a triage does.
    async fn inform(store: &Store, user: &UserKey, mail_id: &str, summary: &str) -> i64 {
        let id = store
            .insert_event(user, &mail(mail_id, Source::Trigger))
            .await
            .unwrap()
            .unwrap();
        store
            .move_events(user, &[id], EventState::Pending, EventState::Triaging, None)
            .await
            .unwrap();
        let triage = NewTriage {
            verdict: Verdict::Inform,
            urgency: Some(Urgency::Later),
            summary: Some(summary),
            reason: "worth knowing",
            refs: &[id],
        };
        store.insert_triage(user, &triage, &[id]).await.unwrap()
    }

    #[tokio::test]
    async fn waiting_updates_are_the_users_newest_first_until_delivered() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
        let first = inform(&store, &alice, "m1", "The build is ready").await;
        let second = inform(&store, &alice, "m2", "The invoice came").await;
        let theirs = inform(&store, &bob, "m3", "Bob's news").await;
        let since = Utc::now() - chrono::Duration::days(2);

        let waiting = store.waiting_updates(&alice, since, 20).await.unwrap();
        let ids: Vec<i64> = waiting.iter().map(|row| row.id).collect();
        assert_eq!(ids, [second, first]);
        assert_eq!(waiting[0].summary, "The invoice came");
        assert_eq!(waiting[0].app, Some(EventApp::Gmail));
        assert_eq!(
            store.waiting_updates(&alice, since, 1).await.unwrap().len(),
            1
        );
        assert!(
            store
                .waiting_updates(&alice, Utc::now(), 20)
                .await
                .unwrap()
                .is_empty(),
            "older than `since` is left out"
        );

        // Another user's id is not alice's to dismiss.
        store
            .mark_delivered(&alice, &[first, theirs], None)
            .await
            .unwrap();
        let waiting = store.waiting_updates(&alice, since, 20).await.unwrap();
        assert_eq!(
            waiting.iter().map(|row| row.id).collect::<Vec<_>>(),
            [second]
        );
        let bobs = store.waiting_updates(&bob, since, 20).await.unwrap();
        assert_eq!(bobs.iter().map(|row| row.id).collect::<Vec<_>>(), [theirs]);
    }

    #[tokio::test]
    async fn topics_triggers_and_polls_are_kept_per_user() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        store
            .set_event_topic(&alice, Topic::GithubCi, false)
            .await
            .unwrap();
        let topics = store.event_topics(&alice).await.unwrap();
        assert_eq!(topics, HashMap::from([(Topic::GithubCi, false)]));
        assert!(
            store
                .event_topics(&UserKey::for_test("bob"))
                .await
                .unwrap()
                .is_empty()
        );

        let row = TriggerRow {
            slug: "GMAIL_NEW_GMAIL_MESSAGE".into(),
            account_id: "ca_1".into(),
            trigger_id: "ti_1".into(),
        };
        store.save_event_trigger(&alice, &row).await.unwrap();
        assert_eq!(
            store.event_triggers(&alice).await.unwrap(),
            std::slice::from_ref(&row)
        );
        store.delete_event_trigger(&alice, &row).await.unwrap();
        assert!(store.event_triggers(&alice).await.unwrap().is_empty());

        assert!(store.polled_at(&alice, "github").await.unwrap().is_none());
        let at = Utc::now();
        store.mark_polled(&alice, "github", at).await.unwrap();
        let polled = store.polled_at(&alice, "github").await.unwrap().unwrap();
        assert!((polled - at).num_milliseconds().abs() < 5);
    }
}
