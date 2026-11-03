//! A merged account's projects. A project is known by its name, compared
//! without case, in every table that names it. When the account joined has
//! a project of the same name, both stay: the moved one, in every table, is
//! renamed by [`copy_name`].

use crate::{
    api::project_sync::MAX_NAME,
    projects::model::{MAX_REPO_PART, RepoName},
};
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
        let Some(renamed) = (1u32..)
            .map(|n| copy_name(&name.name, n))
            .find(|copy| !taken.contains(&copy.to_lowercase()))
        else {
            anyhow::bail!("no copy name is free for the project {}", name.name);
        };
        taken.insert(renamed.to_lowercase());
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

/// The `n`th name a moved project may take when the kept account has its
/// name: `<name>-copy`, then `<name>-copy-2` and on. An `owner/repo` name
/// stays one, so imports, sync, memory scopes and checkout folders take it
/// (`RepoName`, the import's repository parts, `project_sync::project_name`
/// and `RepoName::memory_scope`); the name is cut to leave room for the
/// suffix.
pub(super) fn copy_name(name: &str, n: u32) -> String {
    let suffix = match n {
        1 => "-copy".to_owned(),
        n => format!("-copy-{n}"),
    };
    let longest = match RepoName::parse(name) {
        Ok(repo) => repo.owner().len() + 1 + MAX_REPO_PART,
        Err(_) => MAX_NAME,
    };
    let mut keep = longest.saturating_sub(suffix.len()).min(name.len());
    while !name.is_char_boundary(keep) {
        keep -= 1;
    }
    format!("{}{suffix}", &name[..keep])
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::project_sync::project_name,
        imports::{
            place::{Folders, checkout_folder},
            repo::RepoRef,
        },
    };

    /// Every rule a project name meets on its way through imports, sync,
    /// memory and checkout folders.
    fn passes_every_rule(name: &str) {
        let repo = RepoName::parse(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        // A project's memories are scoped by its repository part.
        repo.memory_scope().expect("a memory scope");
        assert_eq!(project_name(name).ok(), Some(name));
        let import = RepoRef::parse(name, &[]).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(import.project(), name);
        let folders = Folders {
            os: Some("linux"),
            home: Some("/home/user"),
            workspace: None,
        };
        let folder = checkout_folder(folders, &import).expect("a checkout folder");
        assert_eq!(folder, format!("/home/user/projects/{name}"));
    }

    #[test]
    fn a_copy_of_a_repository_name_is_still_one() {
        assert_eq!(copy_name("acme/app", 1), "acme/app-copy");
        assert_eq!(copy_name("acme/app", 3), "acme/app-copy-3");
        passes_every_rule(&copy_name("acme/app", 1));
        passes_every_rule(&copy_name("Acme/My_App.rs", 12));
        // The old form fails the import's rules.
        assert!(RepoName::parse("acme/app (2)").is_err());
        assert!(RepoRef::parse("acme/app (2)", &[]).is_err());

        let long = format!("acme/{}", "r".repeat(MAX_REPO_PART));
        let copy = copy_name(&long, 1000);
        assert!(copy.ends_with("-copy-1000"));
        passes_every_rule(&copy);
    }

    #[test]
    fn a_copy_of_another_name_stays_in_bounds() {
        assert_eq!(copy_name("Notes", 2), "Notes-copy-2");
        let long = "\u{e9}".repeat(MAX_NAME);
        let copy = copy_name(&long, 1);
        assert!(copy.len() <= MAX_NAME && copy.ends_with("-copy"));
        assert!(project_name(&copy).is_ok());
    }
}
