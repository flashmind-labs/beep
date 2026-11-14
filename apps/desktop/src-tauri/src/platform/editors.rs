//! The code editors installed on this Mac, for a project's "Open in".

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::{DesktopError, Result};

/// An editor a checkout can open in, in the order the page prefers them.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Editor {
    Zed,
    Vscode,
    Cursor,
}

/// Where to look for an editor: its app bundles by name, then its CLI.
struct Spec {
    editor: Editor,
    name: &'static str,
    bundles: &'static [&'static str],
    cli: &'static str,
}

const EDITORS: [Spec; 3] = [
    Spec {
        editor: Editor::Zed,
        name: "Zed",
        bundles: &["Zed.app", "Zed Preview.app"],
        cli: "zed",
    },
    Spec {
        editor: Editor::Vscode,
        name: "VS Code",
        bundles: &["Visual Studio Code.app"],
        cli: "code",
    },
    Spec {
        editor: Editor::Cursor,
        name: "Cursor",
        bundles: &["Cursor.app"],
        cli: "cursor",
    },
];

/// How a found editor opens a folder.
enum Launch {
    /// `open -a <bundle> <folder>`.
    App(PathBuf),
    /// `<cli> <folder>`.
    Cli(PathBuf),
}

struct Found {
    editor: Editor,
    name: &'static str,
    launch: Launch,
}

/// One installed editor, as the page lists it.
#[derive(Serialize)]
pub struct EditorRow {
    id: Editor,
    name: &'static str,
}

/// Looked up once per run.
fn found() -> &'static [Found] {
    static FOUND: OnceLock<Vec<Found>> = OnceLock::new();
    FOUND.get_or_init(|| {
        EDITORS
            .iter()
            .filter_map(|s| {
                let launch = app(s.bundles)
                    .map(Launch::App)
                    .or_else(|| on_path(s.cli).map(Launch::Cli))?;
                Some(Found {
                    editor: s.editor,
                    name: s.name,
                    launch,
                })
            })
            .collect()
    })
}

/// The first of `bundles` in /Applications or ~/Applications.
#[cfg(target_os = "macos")]
fn app(bundles: &[&str]) -> Option<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join("Applications"));
    }
    bundles
        .iter()
        .flat_map(|b| dirs.iter().map(move |d| d.join(b)))
        .find(|p| p.is_dir())
}

#[cfg(not(target_os = "macos"))]
fn app(_bundles: &[&str]) -> Option<PathBuf> {
    None
}

fn on_path(cli: &str) -> Option<PathBuf> {
    which::which(cli).ok()
}

/// The editors installed here, preferred first; empty when there are none.
#[tauri::command]
pub fn editors() -> Vec<EditorRow> {
    found()
        .iter()
        .map(|f| EditorRow {
            id: f.editor,
            name: f.name,
        })
        .collect()
}

/// Opens folder `path` in `editor`.
#[tauri::command]
pub async fn open_in_editor(editor: Editor, path: String) -> Result<()> {
    let folder = Path::new(&path);

    if !folder.is_absolute() || !folder.is_dir() {
        return Err(DesktopError::BadRequest(
            "not a folder on this computer".into(),
        ));
    }

    let found = found()
        .iter()
        .find(|f| f.editor == editor)
        .ok_or_else(|| DesktopError::BadRequest("that editor is not installed".into()))?;
    match &found.launch {
        Launch::App(bundle) => {
            let mut cmd = Command::new("open");
            cmd.arg("-a").arg(bundle).arg(folder);
            launch(&mut cmd).await
        }
        Launch::Cli(cli) => {
            // A CLI may run for the editor's lifetime rather than just launch it.
            Command::new(cli)
                .arg(folder)
                .spawn()
                .context("Could not start the editor")?;
            Ok(())
        }
    }
}

async fn launch(cmd: &mut Command) -> Result<()> {
    let output = cmd.output().await.context("Could not start the editor")?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        let message = if detail.is_empty() {
            format!("The editor launcher exited with {}", output.status)
        } else {
            detail.to_owned()
        };

        return Err(DesktopError::BadRequest(message));
    }

    Ok(())
}

#[cfg(all(test, unix))]
#[path = "../tests/platform_editors.rs"]
mod tests;
