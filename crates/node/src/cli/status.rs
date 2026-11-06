//! `beep-node status`: the running node, its mesh and each project's sync.

use beep_node::link::local_status::{LocalStatus, Project};
use beep_proto::node::sync::State;

/// This build's version.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What to print when no node runs from the folder.
pub(crate) fn not_running() -> String {
    format!(
        "beep-node {VERSION}\n\
         Node      not running\n\
         \n\
         Start it with `beep-node service install`, which keeps it running in the\n\
         background, or `beep-node run` to serve in this terminal.\n"
    )
}

/// The status as text; `now` is seconds since the Unix epoch.
pub(crate) fn render(status: &LocalStatus, now: u64) -> String {
    let mut out = format!("beep-node {VERSION}\n");
    let mut node = format!(
        "running for {}, version {}",
        span(now.saturating_sub(status.started)),
        status.version
    );
    if status.version != VERSION {
        node.push_str(" (restart it to run this version)");
    }
    line(&mut out, "Node", &node);
    line(
        &mut out,
        "Machine",
        &format!("{} ({})", status.name, status.id),
    );
    match status.mesh {
        Some(mesh) => {
            let online = status.devices.iter().filter(|d| d.online).count();
            let all = status.devices.len();
            line(
                &mut out,
                "Mesh",
                &format!("{} ({online} of {all} devices online)", mesh.fmt_short()),
            );
        }
        None => line(
            &mut out,
            "Mesh",
            "none; join one with `beep-node mesh join <ticket>`",
        ),
    }

    if !status.devices.is_empty() {
        out.push_str("\nDevices\n");
        let rows: Vec<[String; 3]> = status
            .devices
            .iter()
            .map(|device| {
                let state = if device.id == status.id {
                    "this machine".to_owned()
                } else if device.online {
                    "online".to_owned()
                } else {
                    match device.last_seen {
                        Some(at) => {
                            format!("offline, last seen {} ago", span(now.saturating_sub(at)))
                        }
                        None => "offline, not seen since the node started".to_owned(),
                    }
                };
                [device.name.clone(), device.kind.to_string(), state]
            })
            .collect();
        table(&mut out, &rows);
    }

    out.push_str("\nProjects\n");
    if status.projects.is_empty() {
        out.push_str("  none synced on this machine\n");
    } else {
        let mut projects: Vec<&Project> = status.projects.iter().collect();
        projects.sort_by(|a, b| a.status.name.cmp(&b.status.name));
        let rows: Vec<[String; 3]> = projects
            .into_iter()
            .map(|project| {
                [
                    project.status.name.clone(),
                    project.status.folder.clone(),
                    sync_state(project),
                ]
            })
            .collect();
        table(&mut out, &rows);
    }
    out
}

/// How a project's sync stands, in a few words.
fn sync_state(project: &Project) -> String {
    let status = &project.status;
    if project.missing {
        return "folder missing".to_owned();
    }
    if project.held {
        return "waiting for Syncthing to let go of the folder".to_owned();
    }
    let to_bring = status.pending.max(status.waiting.len() as u64);
    let mut text = match status.state {
        State::Failed => format!(
            "not syncing: {}",
            status.error.as_deref().unwrap_or("stopped")
        ),
        State::Paused => "paused".to_owned(),
        _ if status.paused => "paused".to_owned(),
        State::Scanning => "syncing, looking for changes".to_owned(),
        State::Syncing => match status.progress {
            Some(progress) if progress.files_total > 0 => format!(
                "syncing, {} of {} files",
                progress.files_done, progress.files_total
            ),
            _ => syncing(to_bring),
        },
        State::Idle if to_bring > 0 => syncing(to_bring),
        State::Idle => match &status.error {
            Some(error) => format!("waiting: {error}"),
            None => "in sync".to_owned(),
        },
        State::Other => "in a state this version does not know".to_owned(),
    };
    match status.conflicts.len() {
        0 => {}
        1 => text.push_str(", 1 conflict"),
        n => text.push_str(&format!(", {n} conflicts")),
    }
    text
}

fn syncing(files: u64) -> String {
    match files {
        0 => "syncing".to_owned(),
        1 => "syncing, 1 file to bring".to_owned(),
        n => format!("syncing, {n} files to bring"),
    }
}

fn line(out: &mut String, label: &str, text: &str) {
    out.push_str(&format!("{label:<9} {text}\n"));
}

/// Rows in columns, the last one left to run on.
fn table(out: &mut String, rows: &[[String; 3]]) {
    let first = rows
        .iter()
        .map(|row| row[0].chars().count())
        .max()
        .unwrap_or(0);
    let second = rows
        .iter()
        .map(|row| row[1].chars().count())
        .max()
        .unwrap_or(0);
    for [a, b, c] in rows {
        out.push_str(&format!("  {a:<first$}  {b:<second$}  {c}\n"));
    }
}

/// A span of seconds, such as 45s, 12m, 3h 4m or 2d 5h.
fn span(secs: u64) -> String {
    let (m, h, d) = (secs / 60, secs / 3600, secs / 86_400);
    if d > 0 {
        format!("{d}d {}h", h % 24)
    } else if h > 0 {
        format!("{h}h {}m", m % 60)
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_mesh::DeviceKind;
    use beep_node::link::local_status::Peer;
    use beep_proto::node::sync::ProjectStatus;
    use iroh::SecretKey;

    fn project(name: &str, state: State, held: bool, missing: bool) -> Project {
        Project {
            status: ProjectStatus {
                name: name.to_owned(),
                folder: format!("/srv/{name}"),
                state,
                error: (state == State::Failed).then(|| "no such disk".to_owned()),
                ..ProjectStatus::default()
            },
            held,
            missing,
        }
    }

    #[test]
    fn says_each_device_and_how_each_project_syncs() {
        let me = SecretKey::generate().public();
        let other = SecretKey::generate().public();
        let desk = SecretKey::generate().public();
        let peer = |id, name: &str, kind, online, last_seen| Peer {
            id,
            name: name.to_owned(),
            kind,
            online,
            last_seen,
        };
        let status = LocalStatus {
            version: VERSION.to_owned(),
            started: 1_000,
            name: "build-box".to_owned(),
            id: me,
            mesh: Some(desk),
            devices: vec![
                peer(desk, "laptop", DeviceKind::Desktop, true, None),
                peer(
                    other,
                    "gpu-box",
                    DeviceKind::Node,
                    false,
                    Some(1_000 + 3_000),
                ),
                peer(me, "build-box", DeviceKind::Node, true, None),
            ],
            projects: vec![
                project("api", State::Idle, false, false),
                project("notes", State::Failed, true, false),
                project("old", State::Failed, false, true),
                project("docs", State::Failed, false, false),
            ],
        };
        let text = render(&status, 1_000 + 3_720);
        assert!(text.contains("running for 1h 2m"), "{text}");
        assert!(text.contains("2 of 3 devices online"), "{text}");
        assert!(text.contains("offline, last seen 12m ago"), "{text}");
        assert!(text.contains("this machine"), "{text}");
        for (name, state) in [
            ("api", "in sync"),
            ("notes", "waiting for Syncthing"),
            ("old", "folder missing"),
            ("docs", "not syncing: no such disk"),
        ] {
            let row = text
                .lines()
                .find(|line| line.trim_start().starts_with(name))
                .unwrap_or_else(|| panic!("no row for {name}: {text}"));
            assert!(row.contains(state), "{row}");
        }
    }

    #[test]
    fn spans_read_as_people_say_them() {
        assert_eq!(span(45), "45s");
        assert_eq!(span(720), "12m");
        assert_eq!(span(3 * 3600 + 240), "3h 4m");
        assert_eq!(span(2 * 86_400 + 5 * 3600), "2d 5h");
    }
}
