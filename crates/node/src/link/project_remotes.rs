//! Apply saved repository renames locally, including after an offline machine returns.

use anyhow::{Context, Result, ensure};
use beep_projects::imports::repo::renamed_remote;
use beep_proto::node::mesh_state::{Place, ProjectRecord, RemoteRename};
use std::path::Path;
use tokio::{
    process::Command,
    time::{Duration, timeout},
};

pub(super) async fn apply(placed: impl Iterator<Item = (String, ProjectRecord, Place)>) {
    for (_, record, place) in placed {
        if record.remote_renames.is_empty() {
            continue;
        }

        match timeout(
            Duration::from_secs(15),
            update(Path::new(&place.folder), &record.remote_renames),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("repository remote update is pending: {error:#}"),
            Err(_) => log::warn!("repository remote update timed out; it will be retried"),
        }
    }
}

fn git(folder: &Path) -> Command {
    let mut command = crate::process::command("git");
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(variable);
    }
    command.arg("-C").arg(folder).kill_on_drop(true);
    command
}

async fn update(folder: &Path, renames: &[RemoteRename]) -> Result<()> {
    // Refuse Git's parent-directory discovery when the imported folder is not a checkout yet.
    if !folder.join(".git").exists() {
        return Ok(());
    }
    let output = git(folder)
        .args([
            "config",
            "--local",
            "--null",
            "--get-regexp",
            r"^remote\..*\.(url|pushurl)$",
        ])
        .output()
        .await?;
    ensure!(
        output.status.success() || output.status.code() == Some(1),
        "Git could not read remote URLs"
    );
    let text = std::str::from_utf8(&output.stdout).context("Git remote URLs are not UTF-8")?;

    for entry in text.split('\0').filter(|entry| !entry.is_empty()) {
        let Some((key, old)) = entry.split_once('\n') else {
            continue;
        };
        let mut new = old.to_owned();
        for rename in renames {
            if let Some(renamed) = renamed_remote(&new, rename) {
                new = renamed;
            }
        }
        if new == old {
            continue;
        }
        let output = git(folder)
            .args([
                "config",
                "--local",
                "--fixed-value",
                "--replace-all",
                key,
                &new,
                old,
            ])
            .output()
            .await?;
        ensure!(output.status.success(), "Git could not update a remote URL");
    }

    Ok(())
}

#[cfg(test)]
#[path = "../tests/link_project_remotes.rs"]
mod tests;
