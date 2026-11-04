//! The user's cron routines and the grants they hold.

use super::User;
use crate::{
    protocol::Cron,
    store::{GrantRow, NewGrant},
};
use anyhow::Result;
use flashmind::cron::CronJob;
use uuid::Uuid;

impl User {
    /// The user's scheduled jobs, newest first.
    pub async fn list_crons(&self) -> Result<Vec<Cron>> {
        self.store.list_crons(&self.key).await
    }

    /// Schedules a repeating job and returns its id.
    pub async fn create_cron(&self, label: &str, schedule: &str, prompt: &str) -> Result<String> {
        self.store
            .create_cron(&self.key, label, schedule, prompt)
            .await
    }

    /// Changes a job; `false` when the user has no such job.
    pub async fn update_cron(
        &self,
        id: &str,
        enabled: Option<bool>,
        label: Option<&str>,
        schedule: Option<&str>,
    ) -> Result<bool> {
        self.store
            .update_cron(&self.key, id, enabled, label, schedule)
            .await
    }

    /// Flips whether a job is enabled and returns the new state, or `None` when the user has no such job.
    pub async fn toggle_cron(&self, id: &str) -> Result<Option<bool>> {
        self.store.toggle_cron(&self.key, id).await
    }

    /// One of the user's jobs as the scheduler sees it.
    pub async fn cron_job(&self, id: Uuid) -> Result<Option<CronJob>> {
        self.store.cron_job(id, Some(self.key.as_str())).await
    }

    /// Deletes a job; `false` when the user has no such job.
    pub async fn delete_cron(&self, id: &str) -> Result<bool> {
        self.store.delete_cron(&self.key, id).await
    }

    /// Saves a grant the user approved and returns its id.
    pub(crate) async fn create_grant(&self, grant: NewGrant<'_>) -> Result<Uuid> {
        self.store.create_grant(&self.key, grant).await
    }

    /// The user's grants in force, newest first.
    pub(crate) async fn live_grants(&self) -> Result<Vec<GrantRow>> {
        self.store.live_grants(&self.key).await
    }

    /// The grants in force for calls of `tool` in runs of `routine`.
    pub(crate) async fn routine_grants(&self, routine: Uuid, tool: &str) -> Result<Vec<GrantRow>> {
        self.store.routine_grants(&self.key, routine, tool).await
    }

    /// Records a call of `run` as a use of grant `id`; false when the grant is spent.
    pub(crate) async fn use_grant(&self, id: Uuid, run: &str, call_id: &str) -> Result<bool> {
        self.store.use_grant(&self.key, id, run, call_id).await
    }

    /// Ends one of the user's grants; false when they have none by `id`.
    pub(crate) async fn revoke_grant(&self, id: Uuid) -> Result<bool> {
        self.store.revoke_grant(&self.key, id).await
    }
}
