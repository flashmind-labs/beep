//! The user's projects as Settings shows them: syncs, imports, pins and project groups.

use super::User;
use crate::{protocol::ProjectGroup, store::SaveGroup};
use anyhow::Result;
use beep_projects::credentials::Opened;
use beep_projects::imports::records::SyncRow;
use beep_projects::imports::{
    records::{ImportRow, NewImport},
    store::ImportStore,
};
use beep_projects::model::RepoName;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

impl User {
    pub async fn project_syncs(&self) -> Result<Vec<SyncRow>> {
        self.store.project_syncs(&self.key).await
    }

    /// Syncs a project on a machine, in a folder.
    pub async fn set_project_sync(&self, row: &SyncRow) -> Result<()> {
        self.store.set_project_sync(&self.key, row).await
    }

    /// Stops syncing `project` on `endpoint`; `false` when it did not.
    pub async fn remove_project_sync(&self, project: &str, endpoint: &str) -> Result<bool> {
        self.store
            .remove_project_sync(&self.key, project, endpoint)
            .await
    }

    /// Records a project imported onto a machine.
    pub async fn save_import(&self, import: NewImport<'_>) -> Result<ImportRow> {
        self.store.save_import(&self.key, import).await
    }

    /// Notes the fork the import of `project` on `endpoint` cloned, or none.
    pub(crate) async fn set_import_fork(
        &self,
        project: &str,
        endpoint: &str,
        fork: Option<&crate::projects::model::RepoName>,
    ) -> Result<()> {
        self.store
            .set_import_fork(&self.key, project, endpoint, fork)
            .await
    }

    /// The projects the user imported, by project, then machine.
    pub async fn imports(&self) -> Result<Vec<ImportRow>> {
        self.store.imports(&self.key).await
    }

    /// See `Store::begin_webhook`.
    pub(crate) async fn begin_webhook(
        &self,
        new: &crate::store::NewWebhook<'_>,
    ) -> Result<Vec<u64>> {
        self.store.begin_webhook(&self.key, new).await
    }

    /// See `Store::webhook_made`.
    pub(crate) async fn webhook_made(&self, id: Uuid, hook: u64) -> Result<()> {
        self.store.webhook_made(&self.key, id, hook).await
    }

    pub(crate) async fn orphan_webhooks(&self) -> Result<Vec<crate::store::OrphanWebhooks>> {
        self.store.orphan_webhooks(&self.key).await
    }

    /// See `Store::webhook_removed`.
    pub(crate) async fn webhook_removed(&self, host: &str, repo: &str, hook: u64) -> Result<()> {
        self.store
            .webhook_removed(&self.key, host, repo, hook)
            .await
    }

    /// Deletes every row of project `name`, chats included; `None` for no such project.
    pub(crate) async fn remove_project_rows(
        &self,
        name: &str,
    ) -> Result<Option<crate::store::RemovedProject>> {
        let removed = self.store.remove_project(&self.key, name).await?;
        if removed.is_some() {
            self.changed("");
        }
        Ok(removed)
    }

    /// Forgets `folder` on `endpoint` as a place of the project `name`.
    pub(crate) async fn drop_project_folder(
        &self,
        name: &str,
        endpoint: &str,
        folder: &str,
    ) -> Result<crate::store::DroppedMachine> {
        self.store
            .drop_project_folder(&self.key, name, endpoint, folder)
            .await
    }

    /// Records a found checkout as the project's folder there unless it has one.
    pub(crate) async fn link_import(&self, import: NewImport<'_>) -> Result<bool> {
        self.store.link_import(&self.key, import).await
    }

    /// Notes when `endpoint` was looked through for checkouts.
    pub(crate) async fn save_checkout_check(
        &self,
        endpoint: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        self.store
            .save_checkout_check(&self.key, endpoint, at)
            .await
    }

    /// When each machine was last looked through for checkouts, by key.
    pub(crate) async fn checkout_checks(&self) -> Result<HashMap<String, DateTime<Utc>>> {
        self.store.checkout_checks(&self.key).await
    }

    /// The user's git credentials (GitHub, Forgejo servers), opened.
    pub async fn git_credentials(
        &self,
        key: &crate::account::credentials::CredentialKey,
    ) -> Result<Vec<Opened>> {
        let opened = self.store.machine_credentials(&self.key, key).await?;
        Ok(opened
            .into_iter()
            .filter(|opened| opened.slot.always_to_machines())
            .collect())
    }

    pub async fn pinned_projects(&self) -> Result<Vec<String>> {
        self.store.pinned_projects(&self.key).await
    }

    /// Pins `project`, which stops it on every machine, or unpins it.
    pub async fn pin_project(&self, project: &str, pinned: bool) -> Result<()> {
        self.store.pin_project(&self.key, project, pinned).await
    }

    /// The projects whose sync is paused.
    pub async fn paused_syncs(&self) -> Result<Vec<String>> {
        self.store.paused_syncs(&self.key).await
    }

    /// Pauses `project`'s sync, or resumes it; `false` when it already was.
    pub async fn pause_sync(&self, project: &str, paused: bool) -> Result<bool> {
        self.store.pause_sync(&self.key, project, paused).await
    }

    /// Lets `allow` run without asking for the rest of `chat`.
    pub(crate) async fn allow_in_chat(
        &self,
        chat: Uuid,
        allow: &crate::store::ChatAllow,
    ) -> Result<()> {
        self.store.allow_in_chat(&self.key, chat, allow).await
    }

    /// What the user allowed for the rest of `chat`.
    pub(crate) async fn chat_allows(&self, chat: Uuid) -> Result<Vec<crate::store::ChatAllow>> {
        self.store.chat_allows(&self.key, chat).await
    }

    /// `project`'s terminals, oldest first.
    pub(crate) async fn terminals(&self, project: &str) -> Result<Vec<crate::store::TerminalRow>> {
        self.store.terminals(&self.key, project).await
    }

    /// Every terminal, of any project or none, oldest first.
    pub(crate) async fn all_terminals(&self) -> Result<Vec<crate::store::TerminalRow>> {
        self.store.all_terminals(&self.key).await
    }

    pub(crate) async fn terminal(&self, id: Uuid) -> Result<Option<crate::store::TerminalRow>> {
        self.store.terminal(&self.key, id).await
    }

    pub(crate) async fn add_terminal(&self, row: &crate::store::TerminalRow) -> Result<()> {
        self.store.add_terminal(&self.key, row).await
    }

    pub(crate) async fn remove_terminal(&self, id: Uuid) -> Result<bool> {
        self.store.remove_terminal(&self.key, id).await
    }

    pub(crate) async fn add_review(
        &self,
        review: &crate::store::ReviewRow,
        shards: &[crate::store::ShardRow],
    ) -> Result<()> {
        self.store.add_review(&self.key, review, shards).await
    }

    pub(crate) async fn review(&self, id: Uuid) -> Result<Option<crate::store::ReviewRow>> {
        self.store.review(&self.key, id).await
    }

    /// `project`'s newest review of each branch, newest first.
    pub(crate) async fn reviews(
        &self,
        project: &str,
        limit: u32,
    ) -> Result<Vec<crate::store::ReviewRow>> {
        self.store.reviews(&self.key, project, limit).await
    }

    /// The shards of one of the user's reviews, read through `review`.
    pub(crate) async fn review_shards(
        &self,
        review: &crate::store::ReviewRow,
    ) -> Result<Vec<crate::store::ShardRow>> {
        self.store.review_shards(review.id).await
    }

    pub(crate) async fn review_findings(
        &self,
        review: &crate::store::ReviewRow,
    ) -> Result<Vec<crate::store::ReviewFindingRow>> {
        self.store.review_findings(review.id).await
    }

    pub(crate) async fn report_shard(
        &self,
        review: &crate::store::ReviewRow,
        shard: u32,
        findings: &[crate::store::ReviewFindingRow],
        next: crate::store::ShardState,
    ) -> Result<bool> {
        self.store
            .report_shard(review.id, shard, findings, next)
            .await
    }

    pub(crate) async fn judge_shard(
        &self,
        review: &crate::store::ReviewRow,
        shard: u32,
        judged: &[crate::store::Judged],
    ) -> Result<bool> {
        self.store.judge_shard(review.id, shard, judged).await
    }

    pub(crate) async fn fail_shard(
        &self,
        review: &crate::store::ReviewRow,
        shard: u32,
        from: crate::store::ShardState,
        why: &str,
    ) -> Result<bool> {
        self.store.fail_shard(review.id, shard, from, why).await
    }

    pub(crate) async fn dismiss_finding(
        &self,
        review: Uuid,
        finding: u32,
        dismissed: bool,
    ) -> Result<bool> {
        self.store
            .dismiss_finding(&self.key, review, finding, dismissed)
            .await
    }

    /// The newest `limit` versions of the user's instructions, newest first.
    pub(crate) async fn instructions_versions(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::store::InstructionsRow>> {
        self.store.instructions_versions(&self.key, limit).await
    }

    /// Saves the user's instructions as the version after `base`.
    pub(crate) async fn save_instructions(
        &self,
        base: Option<u32>,
        text: &str,
        note: &str,
    ) -> Result<crate::store::Saved> {
        self.store
            .save_instructions(&self.key, base, text, note)
            .await
    }

    /// The projects pinned to the top of the sidebar, oldest pin first.
    pub async fn projects_on_top(&self) -> Result<Vec<String>> {
        self.store.projects_on_top(&self.key).await
    }

    /// Pins `project` to the top of the sidebar, or unpins it.
    pub async fn pin_project_to_top(&self, project: &str, pinned: bool) -> Result<()> {
        self.store
            .pin_project_to_top(&self.key, project, pinned)
            .await
    }

    /// The user's project groups, oldest first.
    pub async fn project_groups(&self) -> Result<Vec<ProjectGroup>> {
        self.store.project_groups(&self.key).await
    }

    /// Makes a project group, or changes group `group.id`; `None` when there is no such group.
    pub(crate) async fn save_project_group(
        &self,
        group: SaveGroup<'_>,
    ) -> Result<Option<ProjectGroup>> {
        self.store.save_project_group(&self.key, group).await
    }

    /// Forgets the user's project group `id`; false when there is none.
    pub async fn delete_project_group(&self, id: Uuid) -> Result<bool> {
        self.store.delete_project_group(&self.key, id).await
    }

    /// The user's API keys, opened.
    pub(crate) async fn api_keys(
        &self,
        key: &crate::account::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::ApiKey>> {
        self.store.api_keys(&self.key, key).await
    }
}

#[async_trait::async_trait]
impl ImportStore for User {
    async fn imports(&self) -> Result<Vec<ImportRow>> {
        self.store.imports(&self.key).await
    }

    async fn save_import(&self, import: NewImport<'_>) -> Result<ImportRow> {
        self.store.save_import(&self.key, import).await
    }

    async fn link_import(&self, import: NewImport<'_>) -> Result<bool> {
        self.store.link_import(&self.key, import).await
    }

    async fn set_import_fork(
        &self,
        project: &str,
        endpoint: &str,
        fork: Option<&RepoName>,
    ) -> Result<()> {
        self.store
            .set_import_fork(&self.key, project, endpoint, fork)
            .await
    }
}
