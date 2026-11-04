//! Projects and coding-agent sessions for the palette: imported projects,
//! the board's, and the folders agents worked in, found once each.

use super::{
    lists::PaletteLists,
    rank::{Hit, HitKind, PER_KIND, Tier, best, tier},
};
use crate::agents::sessions::SessionRow;
use beep_proto::{ImportedProject, MachineSession};
use std::{cmp::Reverse, collections::HashSet, time::SystemTime};

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
fn within(cwd: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches(['/', '\\']);
    !folder.is_empty()
        && cwd
            .strip_prefix(folder)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
}

/// The project a session runs in: the imported project whose folder on its
/// machine holds it (the deepest), else its folder's name.
fn session_project<'a>(
    session: &'a MachineSession,
    imported: &'a [ImportedProject],
) -> Option<&'a str> {
    imported
        .iter()
        .filter(|p| p.machine_id == session.machine_id && within(&session.cwd, &p.folder))
        .max_by_key(|p| p.folder.len())
        .map(|p| p.project.as_str())
        .or_else(|| last_part(&session.cwd))
}

/// A folder coding agents worked in that no project covers.
struct FolderRow<'a> {
    name: &'a str,
    sessions: usize,
    machines: Vec<&'a str>,
}

/// Projects by name, once each: imported ones, then the board's, then the
/// folders coding agents worked in. A folder named like a listed
/// project's repo is that project.
pub(super) fn project_hits(
    query: &str,
    lists: &PaletteLists,
    imported: &[ImportedProject],
) -> Vec<(Tier, Hit)> {
    let mut hits = Vec::new();
    let mut names: HashSet<&str> = HashSet::new();
    let mut repos: HashSet<String> = HashSet::new();
    for p in imported {
        // A project is imported once per machine it is on; it lists once.
        if !names.insert(p.project.as_str()) {
            continue;
        }
        repos.insert(last_part(&p.project).unwrap_or(&p.project).to_lowercase());
        let Some(tier) = tier(query, &p.project) else {
            continue;
        };
        let (id, name) = (p.project.clone(), p.project.clone());
        hits.push((
            tier,
            Hit::new(HitKind::Project, id, name, p.machine_name.clone()),
        ));
    }
    for b in lists.boards.iter().flatten() {
        if !names.insert(b.name.as_str()) {
            continue;
        }
        repos.insert(last_part(&b.name).unwrap_or(&b.name).to_lowercase());
        let Some(tier) = tier(query, &b.name) else {
            continue;
        };
        let detail = "Project chat · not imported".to_owned();
        let (id, name) = (b.chat_id.to_string(), b.name.clone());
        hits.push((tier, Hit::new(HitKind::ProjectChat, id, name, detail)));
    }
    let mut folders: Vec<FolderRow> = Vec::new();
    for s in lists.sessions.iter().flatten() {
        let Some(name) = session_project(s, imported) else {
            continue;
        };
        if names.contains(name) || repos.contains(&name.to_lowercase()) {
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
        let Some(tier) = tier(query, f.name) else {
            continue;
        };
        let count = if f.sessions == 1 {
            "1 session".to_owned()
        } else {
            format!("{} sessions", f.sessions)
        };
        let detail = format!("Not imported · {count} · {}", f.machines.join(", "));
        let name = f.name.to_owned();
        hits.push((tier, Hit::new(HitKind::Folder, name.clone(), name, detail)));
    }
    best(hits)
}

/// A session that matched, and when it last moved.
struct FoundSession {
    tier: Tier,
    at: SystemTime,
    hit: Hit,
}

/// Coding-agent sessions found by title, project, harness or machine,
/// the best match first, then the one that moved last.
pub(super) fn session_hits(
    query: &str,
    lists: &PaletteLists,
    imported: &[ImportedProject],
) -> Vec<(Tier, Hit)> {
    let mut found: Vec<FoundSession> = lists
        .sessions
        .iter()
        .flatten()
        .filter_map(|s| {
            let project = session_project(s, imported);
            let harness = harness_name(&s.agent);
            let title = s
                .title
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .or(project)
                .unwrap_or(harness);
            let tier = [
                Some(title),
                project,
                Some(harness),
                Some(s.agent.as_str()),
                Some(s.machine_name.as_str()),
            ]
            .into_iter()
            .flatten()
            .filter_map(|text| tier(query, text))
            .min()?;
            let detail = [Some(harness), project, Some(s.machine_name.as_str())]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            let id = format!("{} {}", s.machine_id, s.id);
            let mut hit = Hit::new(HitKind::AgentSession, id, title.to_owned(), detail);
            hit.session = Some(SessionRow::from(s.clone()));
            Some(FoundSession {
                tier,
                at: s.last_activity,
                hit,
            })
        })
        .collect();
    found.sort_by_key(|f| (f.tier, Reverse(f.at)));
    found
        .into_iter()
        .take(PER_KIND)
        .map(|f| (f.tier, f.hit))
        .collect()
}
