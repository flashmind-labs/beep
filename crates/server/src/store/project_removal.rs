//! Forgetting a project or one of its machines; only rows go, and names match without case.

use super::{
    Bumped, Store,
    chats::delete_chat,
    projects::{WORKER_COLUMNS, live_states, worker_from},
    terminals::{self, TerminalRow},
};
use crate::{account::user::UserKey, projects::model::ProjectId, store::WorkerRow};
use anyhow::Result;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// What removing projects or a machine left to do on the machines once committed.
#[derive(Debug, Default)]
pub(crate) struct RemovedProject {
    /// The `projects` rows that went, by id.
    pub(crate) projects: Vec<ProjectId>,
    /// Workers that were live, whose sessions are to be closed.
    pub(crate) workers: Vec<WorkerRow>,
    /// Its terminals, whose commands are to be stopped.
    pub(crate) terminals: Vec<TerminalRow>,
    /// Branch review sessions still reading, to be closed.
    pub(crate) reviewers: Vec<ReviewSession>,
    /// Some machine synced it.
    pub(crate) synced: bool,
}

/// What a removal's transaction took with it, for after it commits.
#[derive(Debug, Default)]
pub(super) struct Gone {
    pub(super) removed: RemovedProject,
    /// Files whose bytes are to be deleted.
    pub(super) files: Vec<String>,
    /// Update logs to announce.
    pub(super) bumped: Vec<Bumped>,
}

/// A branch reviewer's session on a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSession {
    /// The node's key.
    pub(crate) endpoint: String,
    pub(crate) agent: String,
    pub(crate) session: String,
}

/// What dropping one machine's missing folder of a project changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DroppedMachine {
    /// An import or a sync of that folder went.
    pub(crate) dropped: bool,
    /// The machine synced it.
    pub(crate) synced: bool,
    /// Imports and syncs of the project left on any machine.
    pub(crate) left: i64,
}

/// The body of [`Store::remove_project`], in its transaction; false when nothing of that name exists.
pub(super) async fn remove_project_rows(
    tx: &mut PgConnection,
    user: &UserKey,
    name: &str,
    gone: &mut Gone,
) -> Result<bool> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE user_id=$1 AND lower(name)=lower($2)
         ORDER BY id FOR UPDATE",
    )
    .bind(user.as_str())
    .bind(name)
    .fetch_all(&mut *tx)
    .await?;
    let workers = sqlx::query(&format!(
        "SELECT {WORKER_COLUMNS} FROM project_workers w
         WHERE w.project_id = ANY($1) AND w.state = ANY($2) ORDER BY w.started_at"
    ))
    .bind(&ids)
    .bind(live_states())
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(worker_from)
    .collect::<Result<Vec<_>>>()?;
    let chats: Vec<Uuid> = sqlx::query_scalar(
        "SELECT chat_id FROM project_agents WHERE project_id = ANY($1)
         UNION SELECT chat_id FROM project_thread_chats WHERE project_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    for chat in chats {
        let deleted = delete_chat(&mut *tx, chat).await?;
        gone.files.extend(deleted.files);
        gone.bumped.extend(deleted.bumped);
    }
    sqlx::query("DELETE FROM projects WHERE id = ANY($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    let imports =
        sqlx::query("DELETE FROM project_imports WHERE user_id=$1 AND lower(project)=lower($2)")
            .bind(user.as_str())
            .bind(name)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    let synced =
        sqlx::query("DELETE FROM project_sync WHERE user_id=$1 AND lower(project)=lower($2)")
            .bind(user.as_str())
            .bind(name)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    for table in ["pinned_projects", "projects_on_top", "paused_syncs"] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE user_id=$1 AND lower(project)=lower($2)"
        ))
        .bind(user.as_str())
        .bind(name)
        .execute(&mut *tx)
        .await?;
    }
    let terminals = sqlx::query(&format!(
        "DELETE FROM terminals WHERE user_id=$1 AND lower(project)=lower($2)
         RETURNING {}",
        terminals::COLUMNS
    ))
    .bind(user.as_str())
    .bind(name)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(terminals::row_from)
    .collect::<Result<Vec<_>>>()?;
    let reviewers = sqlx::query(
        "SELECT r.endpoint_id, s.agent, s.session FROM branch_review_shards s
         JOIN branch_reviews r ON r.id = s.review_id
         WHERE r.user_id=$1 AND lower(r.project)=lower($2) AND s.state = 'reading'
         ORDER BY r.id, s.shard",
    )
    .bind(user.as_str())
    .bind(name)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(|row| {
        Ok(ReviewSession {
            endpoint: row.try_get(0)?,
            agent: row.try_get(1)?,
            session: row.try_get(2)?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    let reviews =
        sqlx::query("DELETE FROM branch_reviews WHERE user_id=$1 AND lower(project)=lower($2)")
            .bind(user.as_str())
            .bind(name)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if ids.is_empty() && imports == 0 && synced == 0 && terminals.is_empty() && reviews == 0 {
        return Ok(false);
    }
    let removed = &mut gone.removed;
    removed.projects.extend(ids.into_iter().map(ProjectId));
    removed.workers.extend(workers);
    removed.terminals.extend(terminals);
    removed.reviewers.extend(reviewers);
    removed.synced |= synced > 0;
    Ok(true)
}

impl Store {
    /// Deletes every row of project `name` and its chats; `None` when nothing of that name exists.
    pub(crate) async fn remove_project(
        &self,
        user: &UserKey,
        name: &str,
    ) -> Result<Option<RemovedProject>> {
        let mut tx = self.pool.begin().await?;
        let mut gone = Gone::default();
        if !remove_project_rows(&mut tx, user, name, &mut gone).await? {
            return Ok(None);
        }
        tx.commit().await?;
        self.announce(&gone.bumped);
        self.blobs.delete(&gone.files).await?;
        Ok(Some(gone.removed))
    }

    /// Forgets `folder` on `endpoint` for project `name` (import, sync, checkouts); says what went and what is left.
    pub(crate) async fn drop_project_folder(
        &self,
        user: &UserKey,
        name: &str,
        endpoint: &str,
        folder: &str,
    ) -> Result<DroppedMachine> {
        let mut tx = self.pool.begin().await?;
        let imports = sqlx::query(
            "DELETE FROM project_imports
             WHERE user_id=$1 AND lower(project)=lower($2) AND endpoint_id=$3 AND folder=$4",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(endpoint)
        .bind(folder)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let synced = sqlx::query(
            "DELETE FROM project_sync
             WHERE user_id=$1 AND lower(project)=lower($2) AND endpoint_id=$3 AND folder=$4",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(endpoint)
        .bind(folder)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query(
            "DELETE FROM project_checkouts
             WHERE machine=$3 AND path=$4 AND project_id IN
                 (SELECT id FROM projects WHERE user_id=$1 AND lower(name)=lower($2))",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(endpoint)
        .bind(folder)
        .execute(&mut *tx)
        .await?;
        let left: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM project_imports
                     WHERE user_id=$1 AND lower(project)=lower($2))
                  + (SELECT count(*) FROM project_sync
                     WHERE user_id=$1 AND lower(project)=lower($2))",
        )
        .bind(user.as_str())
        .bind(name)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(DroppedMachine {
            dropped: imports + synced > 0,
            synced: synced > 0,
            left,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        projects::{model::WorkerAgent, threads::ThreadKind},
        store::{CheckoutRow, NewImport, NewTask, NewThreadChat, NewWorker, SyncRow, TestDb},
    };
    use beep_projects::model::{IssueHost, MachinePath, RepoName};
    use chrono::Utc;
    use iroh::{EndpointId, SecretKey};

    /// One project of a user's, with a row in every table that keeps one.
    struct Made {
        project: ProjectId,
        chats: Vec<Uuid>,
        machine: EndpointId,
    }

    async fn make(store: &Store, user: &UserKey, name: &str) -> Made {
        let machine = SecretKey::generate().public();
        let endpoint = machine.to_string();
        let repo = RepoName::parse(name).expect("a repo name");
        let project = store.ensure_project(user, &repo).await.expect("made").id;
        let agent = store
            .ensure_project_agent(user, project)
            .await
            .expect("a chat");
        let thread = store
            .open_thread_chat(
                user,
                project,
                &NewThreadChat {
                    repo: name,
                    number: 7,
                    kind: ThreadKind::Issue,
                    title: "A bug",
                },
            )
            .await
            .expect("a thread chat");
        store
            .note_thread_reply(project, name, 7)
            .await
            .expect("a reply noted");
        store
            .add_checkout(
                project,
                &CheckoutRow {
                    machine,
                    path: MachinePath::parse("/srv/code").expect("a path"),
                    home: true,
                },
            )
            .await
            .expect("a checkout");
        // Rows older apps' Not now left behind.
        sqlx::query(
            "INSERT INTO project_dismissed_issues (project_id, host, number) VALUES ($1, $2, 3)",
        )
        .bind(project.0)
        .bind(IssueHost::Github.stored())
        .execute(&store.pool)
        .await
        .expect("dismissed");
        let task = store
            .create_tasks(
                project,
                &[NewTask {
                    title: "Fix it".into(),
                    body: String::new(),
                    depends_on: None,
                    needs: Vec::new(),
                    start: None,
                }],
            )
            .await
            .expect("a task")[0]
            .id;
        store
            .start_worker(
                user,
                &NewWorker {
                    task,
                    machine,
                    agent: WorkerAgent::Claude,
                    folder: MachinePath::parse("/srv/code/.worktrees/fix-it").expect("a path"),
                    persona: None,
                    persona_commit: None,
                },
            )
            .await
            .expect("stored")
            .expect("started");
        store
            .save_import(
                user,
                NewImport {
                    project: name,
                    endpoint: &endpoint,
                    folder: "/srv/code",
                    url: "https://github.com/acme/widget",
                },
            )
            .await
            .expect("imported");
        // Pinning drops syncs, so it goes first.
        store.pin_project(user, name, true).await.expect("pinned");
        store
            .set_project_sync(
                user,
                &SyncRow {
                    project: name.into(),
                    endpoint: endpoint.clone(),
                    folder: "/srv/code".into(),
                },
            )
            .await
            .expect("synced");
        store
            .pin_project_to_top(user, name, true)
            .await
            .expect("on top");
        store
            .add_terminal(
                user,
                &TerminalRow {
                    id: Uuid::new_v4(),
                    project: name.into(),
                    endpoint: endpoint.clone(),
                    task: 4,
                    pty: false,
                    name: "dev".into(),
                    command: "npm run dev".into(),
                    cwd: "/srv/code".into(),
                    started_at: Utc::now(),
                },
            )
            .await
            .expect("a terminal");
        let review = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO branch_reviews (id, user_id, project, branch, base, endpoint_id, folder,
                 head, merge_base, head_at, changed_lines, files)
             VALUES ($1, $2, $3, 'fix', 'main', $4, '/srv/code', 'abc', 'def', now(), 1, 1)",
        )
        .bind(review)
        .bind(user.as_str())
        .bind(name)
        .bind(&endpoint)
        .execute(&store.pool)
        .await
        .expect("a review");
        sqlx::query(
            "INSERT INTO branch_review_shards (review_id, shard, area, paths, files, lines,
                 agent, session, state)
             VALUES ($1, 0, 'all', '{}', 1, 1, 'claude', 'review-session', 'reading')",
        )
        .bind(review)
        .execute(&store.pool)
        .await
        .expect("a shard");
        Made {
            project,
            chats: vec![agent, thread.chat],
            machine,
        }
    }

    /// The rows of `made` left, in every table.
    async fn left(store: &Store, user: &UserKey, name: &str, made: &Made) -> i64 {
        let by_project = [
            "projects WHERE id",
            "project_checkouts WHERE project_id",
            "project_agents WHERE project_id",
            "project_tasks WHERE project_id",
            "project_workers WHERE project_id",
            "project_dismissed_issues WHERE project_id",
            "project_thread_chats WHERE project_id",
            "project_thread_replies WHERE project_id",
        ];
        let by_name = [
            "project_imports",
            "project_sync",
            "pinned_projects",
            "projects_on_top",
            "paused_syncs",
            "terminals",
            "branch_reviews",
        ];
        let mut total = 0;
        for table in by_project {
            let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}=$1"))
                .bind(made.project.0)
                .fetch_one(&store.pool)
                .await
                .expect("counted");
            total += n;
        }
        for table in by_name {
            let n: i64 = sqlx::query_scalar(&format!(
                "SELECT count(*) FROM {table} WHERE user_id=$1 AND project=$2"
            ))
            .bind(user.as_str())
            .bind(name)
            .fetch_one(&store.pool)
            .await
            .expect("counted");
            total += n;
        }
        let shards: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM branch_review_shards s JOIN branch_reviews r ON r.id = s.review_id
             WHERE r.user_id=$1",
        )
        .bind(user.as_str())
        .fetch_one(&store.pool)
        .await
        .expect("counted");
        let chats: i64 = sqlx::query_scalar("SELECT count(*) FROM chats WHERE id = ANY($1)")
            .bind(&made.chats)
            .fetch_one(&store.pool)
            .await
            .expect("counted");
        total + shards + chats
    }

    #[tokio::test]
    async fn removing_a_project_leaves_no_row_and_spares_other_users() {
        let db = TestDb::new().await.expect("a test database");
        let store = db.store().await.expect("a store");
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let mine = make(&store, &alice, "acme/widget").await;
        let theirs = make(&store, &bob, "acme/widget").await;
        let before = left(&store, &bob, "acme/widget", &theirs).await;
        assert!(before >= 17, "every table has a row: {before}");
        assert_eq!(left(&store, &alice, "acme/widget", &mine).await, before);

        let removed = store
            .remove_project(&alice, "Acme/Widget")
            .await
            .expect("removed")
            .expect("it was there");
        assert_eq!(removed.projects, [mine.project]);
        assert_eq!(removed.workers.len(), 1);
        assert_eq!(removed.workers[0].machine, mine.machine);
        assert_eq!(removed.terminals.len(), 1);
        assert_eq!(removed.reviewers.len(), 1);
        assert_eq!(removed.reviewers[0].session, "review-session");
        assert!(removed.synced);
        assert_eq!(left(&store, &alice, "acme/widget", &mine).await, 0);
        assert_eq!(left(&store, &bob, "acme/widget", &theirs).await, before);

        assert!(
            store
                .remove_project(&alice, "acme/widget")
                .await
                .expect("read")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_dropped_folder_says_what_is_left() {
        let db = TestDb::new().await.expect("a test database");
        let store = db.store().await.expect("a store");
        let alice = UserKey::for_test("alice");
        let mine = make(&store, &alice, "acme/widget").await;
        let endpoint = mine.machine.to_string();
        let other = SecretKey::generate().public().to_string();
        store
            .save_import(
                &alice,
                NewImport {
                    project: "acme/widget",
                    endpoint: &other,
                    folder: "/home/user/widget",
                    url: "https://github.com/acme/widget",
                },
            )
            .await
            .expect("imported");
        // The folder moved since the look: nothing goes.
        let moved = store
            .drop_project_folder(&alice, "acme/widget", &endpoint, "/srv/old")
            .await
            .expect("dropped");
        assert!(!moved.dropped);
        let dropped = store
            .drop_project_folder(&alice, "acme/widget", &endpoint, "/srv/code")
            .await
            .expect("dropped");
        assert_eq!(
            dropped,
            DroppedMachine {
                dropped: true,
                synced: true,
                left: 1,
            }
        );
        assert!(
            store
                .checkouts(mine.project)
                .await
                .expect("read")
                .is_empty()
        );
        let last = store
            .drop_project_folder(&alice, "acme/widget", &other, "/home/user/widget")
            .await
            .expect("dropped");
        assert_eq!(last.left, 0);
    }
}
