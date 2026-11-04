//! What goes with a removed machine, in the transaction that revokes its key; only rows go.

use super::{
    project_removal::{Gone, remove_project_rows},
    projects::{WORKER_COLUMNS, live_states, worker_from},
};
use crate::account::user::UserKey;
use anyhow::Result;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// Rows kept per machine with no more to them than `user_id` and the column naming it.
const MACHINE_ROWS: [(&str, &str); 6] = [
    ("terminals", "endpoint_id"),
    ("branch_reviews", "endpoint_id"),
    ("agent_usage", "endpoint_id"),
    ("agent_session_usage", "endpoint_id"),
    ("harness_settings", "machine_id"),
    ("checkout_checks", "endpoint_id"),
];

/// Deletes the machine's sessions and project folders, and each project left with no folder.
pub(super) async fn remove_machine_rows(
    tx: &mut PgConnection,
    user: &UserKey,
    endpoint: &str,
) -> Result<Gone> {
    let mut gone = Gone::default();
    let mut names: Vec<String> = Vec::new();
    for table in ["project_imports", "project_sync"] {
        let found: Vec<String> = sqlx::query_scalar(&format!(
            "DELETE FROM {table} WHERE user_id=$1 AND endpoint_id=$2 RETURNING project"
        ))
        .bind(user.as_str())
        .bind(endpoint)
        .fetch_all(&mut *tx)
        .await?;
        names.extend(found);
    }
    let checkouts = sqlx::query(
        "DELETE FROM project_checkouts c USING projects p
         WHERE c.project_id = p.id AND p.user_id=$1 AND c.machine=$2
         RETURNING p.name, c.project_id, c.home",
    )
    .bind(user.as_str())
    .bind(endpoint)
    .fetch_all(&mut *tx)
    .await?;
    let mut lost_home: Vec<Uuid> = Vec::new();
    for row in &checkouts {
        names.push(row.try_get(0)?);
        if row.try_get(2)? {
            lost_home.push(row.try_get(1)?);
        }
    }

    let orphans: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT lower(n) FROM unnest($2::text[]) n WHERE
             NOT EXISTS (SELECT 1 FROM project_imports i
                         WHERE i.user_id=$1 AND lower(i.project)=lower(n))
         AND NOT EXISTS (SELECT 1 FROM project_sync s
                         WHERE s.user_id=$1 AND lower(s.project)=lower(n))
         AND NOT EXISTS (SELECT 1 FROM project_checkouts c JOIN projects p ON p.id = c.project_id
                         WHERE p.user_id=$1 AND lower(p.name)=lower(n))",
    )
    .bind(user.as_str())
    .bind(&names)
    .fetch_all(&mut *tx)
    .await?;
    for name in &orphans {
        remove_project_rows(tx, user, name, &mut gone).await?;
    }

    // A project that stays gets its home back on a checkout it still has.
    sqlx::query(
        "UPDATE project_checkouts c SET home=true
         FROM (SELECT DISTINCT ON (project_id) project_id, machine, path FROM project_checkouts
               WHERE project_id = ANY($1) ORDER BY project_id, machine, path) first
         WHERE c.project_id = first.project_id AND c.machine = first.machine
             AND c.path = first.path",
    )
    .bind(&lost_home)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE projects SET pin=NULL WHERE user_id=$1 AND pin=$2")
        .bind(user.as_str())
        .bind(endpoint)
        .execute(&mut *tx)
        .await?;

    // Workers of the projects that stay: those of a removed project went with it.
    let live = sqlx::query(&format!(
        "SELECT {WORKER_COLUMNS} FROM project_workers w
         WHERE w.user_id=$1 AND w.machine=$2 AND w.state = ANY($3) ORDER BY w.started_at"
    ))
    .bind(user.as_str())
    .bind(endpoint)
    .bind(live_states())
    .fetch_all(&mut *tx)
    .await?;
    for row in &live {
        gone.removed.workers.push(worker_from(row)?);
    }
    sqlx::query("DELETE FROM project_workers WHERE user_id=$1 AND machine=$2")
        .bind(user.as_str())
        .bind(endpoint)
        .execute(&mut *tx)
        .await?;

    for (table, column) in MACHINE_ROWS {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE user_id=$1 AND {column}=$2"
        ))
        .bind(user.as_str())
        .bind(endpoint)
        .execute(&mut *tx)
        .await?;
    }
    // A session it started elsewhere alerts the user's computers from now on.
    sqlx::query("DELETE FROM agent_session_starts WHERE user_id=$1 AND (machine=$2 OR device=$2)")
        .bind(user.as_str())
        .bind(endpoint)
        .execute(&mut *tx)
        .await?;
    // `agent_images` rows go with their files.
    let files: Vec<String> = sqlx::query_scalar(
        "DELETE FROM files WHERE user_id=$1 AND id IN
             (SELECT file_id FROM agent_images WHERE user_id=$1 AND machine_id=$2)
         RETURNING id",
    )
    .bind(user.as_str())
    .bind(endpoint)
    .fetch_all(&mut *tx)
    .await?;
    gone.files.extend(files);
    Ok(gone)
}
