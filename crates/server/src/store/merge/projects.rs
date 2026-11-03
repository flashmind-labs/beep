//! A merged account's projects. A project is known by its name, compared
//! without case, in every table that names it. When the account joined has
//! a project of the same name, both stay: the moved one, in every table, is
//! renamed `<name> (2)`, or the next number free in either account.

use anyhow::Result;
use sqlx::PgConnection;
use std::collections::HashSet;

/// The tables that name a project, and the column that names it. Their
/// rows move with the project; a project's tasks, checkouts and chats hang
/// off `projects` by its id.
pub(super) const NAMED: &[(&str, &str)] = &[
    ("projects", "name"),
    ("project_imports", "project"),
    ("project_sync", "project"),
    ("pinned_projects", "project"),
    ("projects_on_top", "project"),
    ("paused_syncs", "project"),
    ("terminals", "project"),
    ("branch_reviews", "project"),
];

/// Tables of a project's that name the user but not the project.
pub(super) const OTHERS: &[&str] = &["project_workers"];

/// One project name of an account.
struct Name {
    name: String,
    /// `name` as Postgres lowers it, which is how names are compared.
    lower: String,
}

/// Moves the projects of `source` to `target`, renaming each one `target`
/// also has.
pub(super) async fn move_projects(tx: &mut PgConnection, source: &str, target: &str) -> Result<()> {
    let moved = names(tx, source).await?;
    let kept: HashSet<String> = names(tx, target)
        .await?
        .into_iter()
        .map(|name| name.lower)
        .collect();
    let mut taken: HashSet<String> = kept.clone();
    taken.extend(moved.iter().map(|name| name.lower.clone()));
    for name in moved.iter().filter(|name| kept.contains(&name.lower)) {
        let n = (2u32..)
            .find(|n| !taken.contains(&format!("{} ({n})", name.lower)))
            .unwrap_or(2);
        // `lower` leaves the ASCII suffix as it is.
        taken.insert(format!("{} ({n})", name.lower));
        let renamed = format!("{} ({n})", name.name);
        for (table, column) in NAMED {
            sqlx::query(&format!(
                "UPDATE {table} SET {column}=$3 WHERE user_id=$1 AND lower({column})=$2"
            ))
            .bind(source)
            .bind(&name.lower)
            .bind(&renamed)
            .execute(&mut *tx)
            .await?;
        }
    }
    let tables = NAMED
        .iter()
        .map(|(table, _)| *table)
        .chain(OTHERS.iter().copied());
    for table in tables {
        sqlx::query(&format!("UPDATE {table} SET user_id=$2 WHERE user_id=$1"))
            .bind(source)
            .bind(target)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

/// Every project name of `user`, once each without case; as `projects`
/// spells it when it has one.
async fn names(tx: &mut PgConnection, user: &str) -> Result<Vec<Name>> {
    let union = NAMED
        .iter()
        .enumerate()
        .map(|(rank, (table, column))| {
            format!("SELECT {column} AS name, {rank} AS rank FROM {table} WHERE user_id=$1")
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let rows: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT DISTINCT ON (lower(name)) name, lower(name) FROM ({union}) n
         ORDER BY lower(name), rank, name"
    ))
    .bind(user)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, lower)| Name { name, lower })
        .collect())
}
