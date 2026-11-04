//! The user's project tasks, workers, checkouts and thread chats.

use super::User;
use crate::{
    projects::model::{
        IssueHost, IssueRef, MachinePath, ProjectId, RepoName, Roles, StartRequest, TaskId,
        TaskState, Visibility, WorkerId, WorkerState,
    },
    protocol::PendingApproval,
    store::{
        CheckoutRow, IssueAuthor, NewTask, NewThreadChat, NewWorker, OpenCounts, ProjectRow,
        StartRefused, TaskRow, ThreadChatRow, TrackedIssue, WorkerRow,
    },
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use uuid::Uuid;

/// The user's projects, their tasks and workers.
#[allow(dead_code, reason = "the project agent uses these in the next tasks")]
impl User {
    pub(crate) async fn ensure_project(&self, name: &RepoName) -> Result<ProjectRow> {
        self.store.ensure_project(&self.key, name).await
    }

    pub(crate) async fn projects(&self) -> Result<Vec<ProjectRow>> {
        self.store.projects(&self.key).await
    }

    pub(crate) async fn project(&self, id: ProjectId) -> Result<Option<ProjectRow>> {
        self.store.project(&self.key, id).await
    }

    /// The project whose agent talks in `chat`, if it is one.
    pub(crate) async fn project_in(&self, chat: Uuid) -> Result<Option<ProjectRow>> {
        self.store.project_in(&self.key, chat).await
    }

    /// The chat of the project's agent, made on first use.
    pub(crate) async fn ensure_project_agent(&self, project: ProjectId) -> Result<Uuid> {
        self.store.ensure_project_agent(&self.key, project).await
    }

    /// Every project's open tasks and live workers for the Projects overview, by name.
    pub(crate) async fn project_overviews(&self) -> Result<Vec<crate::store::OverviewRow>> {
        self.store.project_overviews(&self.key).await
    }

    pub(crate) async fn project_open_counts(&self, project: ProjectId) -> Result<OpenCounts> {
        self.store.open_counts(project).await
    }

    /// The chats of the project agents made so far.
    pub(crate) async fn project_chats(&self) -> Result<Vec<Uuid>> {
        self.store.project_chats(&self.key).await
    }

    /// The cards `chats` wait on, with their chats.
    pub(crate) async fn open_approvals(
        &self,
        chats: &[Uuid],
    ) -> Result<Vec<(Uuid, PendingApproval)>> {
        self.store.open_approvals(&self.key, chats).await
    }

    pub(crate) async fn project_task(&self, id: TaskId) -> Result<Option<TaskRow>> {
        self.store.task(&self.key, id).await
    }

    /// Tasks waiting for a slot to start, oldest first.
    pub(crate) async fn queued_tasks(&self) -> Result<Vec<TaskRow>> {
        self.store.queued_tasks(&self.key).await
    }

    pub(crate) async fn start_worker(
        &self,
        worker: &NewWorker,
    ) -> Result<Result<WorkerRow, StartRefused>> {
        self.store.start_worker(&self.key, worker).await
    }

    pub(crate) async fn live_workers_on(&self, machine: &EndpointId) -> Result<Vec<WorkerRow>> {
        self.store.live_workers_on(&self.key, machine).await
    }

    pub(crate) async fn live_workers(&self) -> Result<Vec<WorkerRow>> {
        self.store.live_workers(&self.key).await
    }

    pub(crate) async fn project_worker(&self, id: WorkerId) -> Result<Option<WorkerRow>> {
        self.store.worker(&self.key, id).await
    }

    // The calls below take a project or task id the caller already read
    // through one of the user's own lookups above.

    pub(crate) async fn checkouts(&self, project: ProjectId) -> Result<Vec<CheckoutRow>> {
        self.store.checkouts(project).await
    }

    pub(crate) async fn add_checkout(
        &self,
        project: ProjectId,
        checkout: &CheckoutRow,
    ) -> Result<()> {
        self.store.add_checkout(project, checkout).await
    }

    pub(crate) async fn set_home(
        &self,
        project: ProjectId,
        machine: &EndpointId,
        path: &MachinePath,
    ) -> Result<()> {
        self.store.set_home(project, machine, path).await
    }

    pub(crate) async fn set_pin(&self, project: ProjectId, pin: Option<&EndpointId>) -> Result<()> {
        self.store.set_pin(project, pin).await
    }

    pub(crate) async fn set_project_setup(
        &self,
        project: ProjectId,
        roles: &Roles,
        visibility: Visibility,
        default_branch: &str,
    ) -> Result<()> {
        self.store
            .set_project_setup(project, roles, visibility, default_branch)
            .await
    }

    pub(crate) async fn create_tasks(
        &self,
        project: ProjectId,
        tasks: &[NewTask],
    ) -> Result<Vec<TaskRow>> {
        self.store.create_tasks(project, tasks).await
    }

    pub(crate) async fn track_issue(
        &self,
        project: ProjectId,
        task: &NewTask,
        host: &IssueHost,
        issue: &IssueRef,
        author: IssueAuthor,
    ) -> Result<TrackedIssue> {
        self.store
            .track_issue(project, task, host, issue, author)
            .await
    }

    pub(crate) async fn note_issue_comment(&self, task: TaskId, once: bool) -> Result<bool> {
        self.store.note_issue_comment(task, once).await
    }

    /// The chat about one thread of `project`, made on the first ask.
    pub(crate) async fn open_thread_chat(
        &self,
        project: ProjectId,
        new: &NewThreadChat<'_>,
    ) -> Result<ThreadChatRow> {
        self.store.open_thread_chat(&self.key, project, new).await
    }

    pub(crate) async fn thread_chats(&self, project: ProjectId) -> Result<Vec<ThreadChatRow>> {
        self.store.thread_chats(project).await
    }

    /// The thread `chat` is about, when it is a thread chat.
    pub(crate) async fn thread_in(&self, chat: Uuid) -> Result<Option<ThreadChatRow>> {
        self.store.thread_in(&self.key, chat).await
    }

    pub(crate) async fn note_thread_reply(
        &self,
        project: ProjectId,
        repo: &str,
        number: u64,
    ) -> Result<bool> {
        self.store.note_thread_reply(project, repo, number).await
    }

    pub(crate) async fn project_tasks(&self, project: ProjectId) -> Result<Vec<TaskRow>> {
        self.store.tasks(project).await
    }

    pub(crate) async fn task_by_slug(
        &self,
        project: ProjectId,
        slug: &str,
    ) -> Result<Option<TaskRow>> {
        self.store.task_by_slug(project, slug).await
    }

    pub(crate) async fn set_task_state(&self, task: TaskId, state: TaskState) -> Result<()> {
        self.store.set_task_state(task, state).await
    }

    /// Moves the task from `from` to `to` unless it moved since it was read.
    pub(crate) async fn move_task_state(
        &self,
        task: TaskId,
        from: TaskState,
        to: TaskState,
    ) -> Result<bool> {
        self.store.move_task_state(task, from, to).await
    }

    pub(crate) async fn set_task_issue(
        &self,
        task: TaskId,
        host: &IssueHost,
        issue: &IssueRef,
    ) -> Result<()> {
        self.store.set_task_issue(task, host, issue).await
    }

    pub(crate) async fn set_task_start(
        &self,
        task: TaskId,
        start: Option<&StartRequest>,
    ) -> Result<()> {
        self.store.set_task_start(task, start).await
    }

    pub(crate) async fn set_task_branch(&self, task: TaskId, branch: &str) -> Result<()> {
        self.store.set_task_branch(task, branch).await
    }

    pub(crate) async fn set_task_pr(&self, task: TaskId, url: &str) -> Result<()> {
        self.store.set_task_pr(task, url).await
    }

    pub(crate) async fn set_task_after(&self, task: TaskId, after: Option<TaskId>) -> Result<()> {
        self.store.set_task_after(task, after).await
    }

    pub(crate) async fn update_task_text(
        &self,
        task: TaskId,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<()> {
        self.store.update_task_text(task, title, body).await
    }

    pub(crate) async fn set_worker_state(
        &self,
        worker: WorkerId,
        state: WorkerState,
    ) -> Result<bool> {
        self.store.set_worker_state(worker, state).await
    }

    pub(crate) async fn worker_turn(
        &self,
        worker: WorkerId,
        turns: u32,
        last_output: &str,
        at: DateTime<Utc>,
    ) -> Result<bool> {
        self.store.worker_turn(worker, turns, last_output, at).await
    }

    pub(crate) async fn set_worker_asking(&self, worker: WorkerId, asking: bool) -> Result<()> {
        self.store.set_worker_asking(worker, asking).await
    }

    pub(crate) async fn set_pr_seen(&self, task: TaskId, fingerprint: &str) -> Result<()> {
        self.store.set_pr_seen(task, fingerprint).await
    }

    /// The project's live workers and those that ended since `since`.
    pub(crate) async fn project_workers(
        &self,
        project: ProjectId,
        since: DateTime<Utc>,
    ) -> Result<Vec<WorkerRow>> {
        self.store.workers(project, since).await
    }

    pub(crate) async fn set_worker_session(&self, worker: WorkerId, session: &str) -> Result<()> {
        self.store.set_worker_session(worker, session).await
    }
}
