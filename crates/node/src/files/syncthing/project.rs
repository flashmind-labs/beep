//! Per-project Syncthing exclusions, stored in the shared folder's ignore file.

use super::{Gui, Managed, Syncthing, TIMEOUT, get, query_value, real, timeout};
use super::stignore::{BEGIN, END};
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::SyncthingFiles;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Ignores {
    #[serde(default)]
    ignore: Vec<String>,
}

impl Syncthing {
    /// Reads or changes this project's exclusion. Syncthing's API applies
    /// the ignore file immediately; no shared folder is paused.
    pub(crate) async fn project_files(
        &self,
        folder: &Path,
        enabled: Option<bool>,
    ) -> Result<SyncthingFiles> {
        let folder = folder.to_owned();
        let folder = tokio::task::spawn_blocking(move || real(&folder)).await?;
        let listed = match self.gui().await {
            Ok(gui) => match timeout(TIMEOUT, super::fetch(&gui)).await {
                Ok(Ok(mut raw)) => {
                    for folder in &mut raw {
                        folder.paused = false;
                    }

                    let home = self.inner.home.clone();
                    Some(
                        tokio::task::spawn_blocking(move || super::resolve(raw, home.as_deref()))
                            .await?,
                    )
                }
                _ => None,
            },
            Err(_) => None,
        };
        let managed = match listed {
            Some(listed) => listed
                .iter()
                .filter(|share| share.holds(&folder.to_string_lossy()))
                .max_by_key(|share| share.path.len())
                .map(|share| Managed {
                    id: Some(share.id.clone()),
                    label: share.name().to_owned(),
                    root: share.path.clone().into(),
                }),
            None => {
                let folder = folder.clone();
                let home = self.inner.home.clone();
                tokio::task::spawn_blocking(move || super::marked(&folder, home.as_deref())).await?
            }
        };
        let Some(managed) = managed else {
            return Ok(SyncthingFiles {
                available: false,
                enabled: false,
                label: None,
            });
        };

        if let Some(enabled) = enabled {
            let id = managed
                .id
                .as_deref()
                .context("Start Syncthing before changing project file sync")?;
            let gui = self.gui().await?;
            let path = format!("/rest/db/ignores?folder={}", query_value(id));
            let raw = timeout(TIMEOUT, get(&gui, &path)).await??;
            let old: Ignores = serde_json::from_slice(&raw)?;
            let pattern = pattern(&managed.root, &folder)?;
            let updated = update(&old.ignore, &pattern, enabled);

            if updated != old.ignore {
                post(&gui, &path, &Ignores { ignore: updated }).await?;
            }
        }

        let share = managed.root.clone();
        let ignored = tokio::task::spawn_blocking(move || {
            super::stignore::project_ignored(&folder, &share)
        })
        .await??;
        Ok(SyncthingFiles {
            available: true,
            enabled: !ignored,
            label: Some(managed.label),
        })
    }
}

fn pattern(root: &Path, folder: &Path) -> Result<String> {
    let relative = folder
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/");
    if relative.contains(['\n', '\r', '*', '?', '[', ']', '{', '}']) {
        bail!(
            "This project's path contains characters Syncthing treats as patterns; set its ignore rule in Syncthing"
        );
    }

    Ok(super::stignore::project_pattern(&relative))
}

/// Keeps every excluded project's pattern in one managed block, so the
/// block holds all of them between a single pair of markers.
fn update(lines: &[String], pattern: &str, enabled: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut patterns: Vec<String> = Vec::new();
    let mut inside = false;

    for line in lines {
        if line == BEGIN {
            inside = true;
        } else if line == END && inside {
            inside = false;
        } else if inside {
            patterns.push(line.clone());
        } else {
            out.push(line.clone());
        }
    }

    patterns.retain(|existing| existing != pattern);

    if !enabled {
        patterns.push(pattern.to_owned());
    }

    if !patterns.is_empty() {
        let git_rules: Vec<_> = super::LOCAL_GIT.lines().map(str::to_owned).collect();
        let at = if out.starts_with(&git_rules) {
            git_rules.len()
        } else {
            0
        };
        let block = [BEGIN.to_owned()]
            .into_iter()
            .chain(patterns)
            .chain([END.to_owned()]);
        out.splice(at..at, block);
    }

    out
}

async fn post(gui: &Gui, path: &str, ignores: &Ignores) -> Result<()> {
    timeout(
        TIMEOUT,
        super::request(gui, "POST", path, &serde_json::to_vec(ignores)?),
    )
    .await
    .context("Updating Syncthing's project ignore rule timed out")??;
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/files_syncthing_project.rs"]
mod tests;
