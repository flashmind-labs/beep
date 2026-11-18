//! Projects and coding-agent sessions for the palette: imported projects and
//! the folders agents worked in, listed once each.

use super::{
    index::{Entry, HitKind},
    lists::PaletteLists,
};
use crate::agents::sessions::SessionRow;
use beep_proto::{ImportedProject, MachineSession};
use std::{cmp::Reverse, collections::HashSet};

/// The harness's name as the app shows it, such as "Claude Code".
fn harness_name(agent: &str) -> &str {
    match agent {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// The last part of a path or of `org/repo`; `None` for a root or nothing.
fn last_part(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\']).find(|part| !part.is_empty())
}

/// True when `cwd` is `folder` or inside it.
pub(super) fn within(cwd: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches(['/', '\\']);
    !folder.is_empty()
        && cwd
            .strip_prefix(folder)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
}

/// The project a session runs in: the imported project whose folder on its
/// machine holds it (the deepest), else its folder's name. A folder named
/// like an imported project's repo is that project.
fn session_project<'a>(
    session: &'a MachineSession,
    imported: &'a [ImportedProject],
) -> Option<&'a str> {
    let holding = imported
        .iter()
        .filter(|p| p.machine_id == session.machine_id && within(&session.cwd, &p.folder))
        .max_by_key(|p| p.folder.len());

    if let Some(p) = holding {
        return Some(&p.project);
    }

    let folder = last_part(&session.cwd)?;
    let repo = folder.to_lowercase();
    let named = imported
        .iter()
        .find(|p| last_part(&p.project).unwrap_or(&p.project).to_lowercase() == repo);

    Some(named.map_or(folder, |p| &p.project))
}

/// A folder coding agents worked in that no project covers.
struct FolderRow<'a> {
    name: &'a str,
    sessions: usize,
    machines: Vec<&'a str>,
}

/// Projects by name, once each: imported ones, then the folders coding
/// agents worked in.
pub(super) fn project_entries(lists: &PaletteLists, imported: &[ImportedProject]) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut names: HashSet<&str> = HashSet::new();

    for p in imported {
        // A project is imported once per machine it is on; it lists once.
        if !names.insert(p.project.as_str()) {
            continue;
        }

        let (id, name, machine) = (
            p.project.clone(),
            p.display_name.as_ref().unwrap_or(&p.project).clone(),
            p.machine_name.clone(),
        );
        let keys = [p.project.as_str(), name.as_str()];
        entries.push(Entry::new(HitKind::Project, id, name.clone(), machine, &keys));
    }

    let mut folders: Vec<FolderRow> = Vec::new();

    for s in lists.sessions.iter().flatten() {
        let Some(name) = session_project(s, imported) else {
            continue;
        };

        if names.contains(name) {
            continue;
        }

        let at = match folders.iter().position(|f| f.name == name) {
            Some(at) => at,
            None => {
                folders.push(FolderRow {
                    name,
                    sessions: 0,
                    machines: Vec::new(),
                });
                folders.len() - 1
            }
        };
        let folder = &mut folders[at];
        folder.sessions += 1;
        if !folder.machines.contains(&s.machine_name.as_str()) {
            folder.machines.push(&s.machine_name);
        }
    }

    for f in folders {
        let count = if f.sessions == 1 {
            "1 session".to_owned()
        } else {
            format!("{} sessions", f.sessions)
        };
        let detail = format!("Not imported · {count} · {}", f.machines.join(", "));
        let name = f.name.to_owned();
        entries.push(Entry::new(
            HitKind::Folder,
            name.clone(),
            name,
            detail,
            &[f.name],
        ));
    }

    entries
}

/// Coding-agent sessions, the one that moved last first, found by title,
/// project, harness or machine.
pub(super) fn session_entries(lists: &PaletteLists, imported: &[ImportedProject]) -> Vec<Entry> {
    let mut sessions: Vec<&MachineSession> = lists.sessions.iter().flatten().collect();
    sessions.sort_by_key(|s| Reverse(s.last_activity));
    sessions
        .into_iter()
        .map(|s| session_entry(s, imported))
        .collect()
}

pub(super) fn session_entry(s: &MachineSession, imported: &[ImportedProject]) -> Entry {
    let project = session_project(s, imported);
    let harness = harness_name(&s.agent);
    let title = s
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .or(project)
        .unwrap_or(harness);
    let keys: Vec<&str> = [
        Some(title),
        project,
        Some(harness),
        Some(s.agent.as_str()),
        Some(s.machine_name.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect();
    // The harness shows as the row's icon.
    let detail = project.unwrap_or_default().to_owned();
    let id = format!("{} {}", s.machine_id, s.id);
    let mut entry = Entry::new(HitKind::AgentSession, id, title.to_owned(), detail, &keys);
    entry.session = Some(SessionRow::from(s.clone()));
    entry.project = project.map(str::to_owned);
    entry
}
