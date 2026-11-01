//! The user's account events, topics, triggers and poll cursors.

use super::User;
use crate::{
    events::{AppEvent, Topic},
    store::{EventRow, EventState, NewTriage, RecentEvent, TriageRow, TriggerRow, UpdateRow},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::time::Duration;

/// Whether each topic is on for the user, defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topics(Vec<(Topic, bool)>);

impl Topics {
    pub fn is_on(&self, topic: Topic) -> bool {
        self.0
            .iter()
            .find(|(t, _)| *t == topic)
            .is_some_and(|(_, on)| *on)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Topic, bool)> + '_ {
        self.0.iter().copied()
    }
}

impl User {
    /// Stores `event`; `None` when the same happening came before.
    pub(crate) async fn store_event(&self, event: &AppEvent) -> Result<Option<i64>> {
        self.store.insert_event(&self.key, event).await
    }

    pub async fn events_in(
        &self,
        state: EventState,
        age: Duration,
        limit: i64,
    ) -> Result<Vec<EventRow>> {
        self.store.events_in(&self.key, state, age, limit).await
    }

    pub(crate) async fn move_events(
        &self,
        ids: &[i64],
        from: EventState,
        to: EventState,
        run_id: Option<&str>,
    ) -> Result<u64> {
        self.store
            .move_events(&self.key, ids, from, to, run_id)
            .await
    }

    /// Puts events back in the queue whose triage started more than `after`
    /// ago and never settled.
    pub(crate) async fn requeue_stale_triaging(&self, after: Duration) -> Result<u64> {
        self.store.requeue_stale_triaging(&self.key, after).await
    }

    /// Logs a triage decision about `events` and marks them triaged.
    pub(crate) async fn log_triage(&self, triage: &NewTriage<'_>, events: &[i64]) -> Result<i64> {
        self.store.insert_triage(&self.key, triage, events).await
    }

    pub async fn undelivered_informs(
        &self,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<TriageRow>> {
        self.store
            .undelivered_informs(&self.key, since, limit)
            .await
    }

    /// Informs the user has not seen, decided after `since`, newest first.
    pub(crate) async fn waiting_updates(
        &self,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<UpdateRow>> {
        self.store.waiting_updates(&self.key, since, limit).await
    }

    pub(crate) async fn mark_delivered(&self, ids: &[i64], run_id: Option<&str>) -> Result<()> {
        self.store.mark_delivered(&self.key, ids, run_id).await
    }

    pub async fn recent_events(
        &self,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<RecentEvent>> {
        self.store.recent_events(&self.key, since, limit).await
    }

    pub(crate) async fn prune_events(&self, before: DateTime<Utc>) -> Result<u64> {
        self.store.prune_events(&self.key, before).await
    }

    /// Every topic and whether the user gets it.
    pub async fn event_topics(&self) -> Result<Topics> {
        let chosen = self.store.event_topics(&self.key).await?;
        Ok(Topics(
            Topic::ALL
                .into_iter()
                .map(|topic| {
                    let on = chosen.get(&topic).copied().unwrap_or(topic.default_on());
                    (topic, on)
                })
                .collect(),
        ))
    }

    pub async fn set_event_topic(&self, topic: Topic, enabled: bool) -> Result<()> {
        self.store.set_event_topic(&self.key, topic, enabled).await
    }

    pub(crate) async fn event_triggers(&self) -> Result<Vec<TriggerRow>> {
        self.store.event_triggers(&self.key).await
    }

    pub(crate) async fn save_event_trigger(&self, row: &TriggerRow) -> Result<()> {
        self.store.save_event_trigger(&self.key, row).await
    }

    pub(crate) async fn delete_event_trigger(&self, row: &TriggerRow) -> Result<()> {
        self.store.delete_event_trigger(&self.key, row).await
    }

    pub(crate) async fn polled_at(&self, source: &str) -> Result<Option<DateTime<Utc>>> {
        self.store.polled_at(&self.key, source).await
    }

    pub(crate) async fn mark_polled(&self, source: &str, at: DateTime<Utc>) -> Result<()> {
        self.store.mark_polled(&self.key, source, at).await
    }
}
