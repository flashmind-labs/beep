use std::sync::mpsc;

use super::*;
use crate::ui::Node;

const TIMEOUT: Duration = Duration::from_secs(30);

fn write_plugin(dir: &std::path::Path, id: &str, text: &str) {
    let folder = dir.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("beep-plugin.json"),
        format!(
            r#"{{ "id": "{id}", "name": "T", "version": "0.1.0", "permissions": ["ui.add"] }}"#
        ),
    )
    .unwrap();
    write_main(dir, id, text);
}

fn write_main(dir: &std::path::Path, id: &str, text: &str) {
    std::fs::write(
        dir.join(id).join("main.ts"),
        format!(r#"beep.place({{ slot: "composer" }}, () => beep.text("{text}"));"#),
    )
    .unwrap();
}

fn start(dir: &std::path::Path) -> (Host, mpsc::Receiver<Vec<Placement>>) {
    let (tx, rx) = mpsc::channel();
    let host = Host::start(dir.to_path_buf(), move |placements| {
        let _ = tx.send(placements.to_vec());
    })
    .unwrap();

    (host, rx)
}

/// Waits for a change that satisfies `accept`.
fn wait_for(
    rx: &mpsc::Receiver<Vec<Placement>>,
    accept: impl Fn(&[Placement]) -> bool,
) -> Vec<Placement> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let placements = rx.recv_timeout(left).expect("no matching change in time");
        if accept(&placements) {
            return placements;
        }
    }
}

fn texts(placements: &[Placement]) -> Vec<String> {
    placements
        .iter()
        .filter_map(|placement| match &placement.node {
            Node::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plugin_folder_shows_its_placement() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");

    let (host, rx) = start(root.path());
    let placements = wait_for(&rx, |p| !p.is_empty());

    assert_eq!(texts(&placements), ["meow"]);
    assert_eq!(host.placements(), placements);
}

#[test]
fn creates_a_missing_folder() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("plugins");

    let (host, _rx) = start(&dir);

    assert!(dir.is_dir());
    assert!(host.placements().is_empty());
}

#[test]
fn an_edited_file_reloads_the_plugin() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "before");
    let (_host, rx) = start(root.path());
    wait_for(&rx, |p| texts(p) == ["before"]);

    write_main(root.path(), "cat-one", "after");

    wait_for(&rx, |p| texts(p) == ["after"]);
}

#[test]
fn a_new_folder_loads_and_a_removed_folder_hides() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "one");
    let (host, rx) = start(root.path());
    wait_for(&rx, |p| texts(p) == ["one"]);

    write_plugin(root.path(), "cat-two", "two");
    wait_for(&rx, |p| texts(p) == ["one", "two"]);

    std::fs::remove_dir_all(root.path().join("cat-one")).unwrap();
    wait_for(&rx, |p| texts(p) == ["two"]);
    assert_eq!(texts(&host.placements()), ["two"]);
}

#[test]
fn a_failing_plugin_shows_nothing_and_others_keep_running() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "good-one", "ok");
    write_plugin(root.path(), "bad-one", "x");
    std::fs::write(
        root.path().join("bad-one/main.ts"),
        "throw new Error('boom');",
    )
    .unwrap();

    let (_host, rx) = start(root.path());

    wait_for(&rx, |p| texts(p) == ["ok"]);
}
