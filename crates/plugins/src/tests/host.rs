use std::sync::mpsc;

use serde_json::json;

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

/// A handler that forwards what the host reports to two channels.
struct Reports {
    shown: mpsc::Sender<Vec<Placement>>,
    wanted: mpsc::Sender<Vec<Topic>>,
}

impl Handler for Reports {
    fn shown(&self, placements: &[Placement]) {
        let _ = self.shown.send(placements.to_vec());
    }

    fn wanted(&self, topics: &[Topic]) {
        let _ = self.wanted.send(topics.to_vec());
    }
}

fn start_reporting(
    dir: &std::path::Path,
) -> (
    Host,
    mpsc::Receiver<Vec<Placement>>,
    mpsc::Receiver<Vec<Topic>>,
) {
    let (shown, shown_rx) = mpsc::channel();
    let (wanted, wanted_rx) = mpsc::channel();
    let host = Host::start(dir.to_path_buf(), Reports { shown, wanted }).unwrap();

    (host, shown_rx, wanted_rx)
}

fn start(dir: &std::path::Path) -> (Host, mpsc::Receiver<Vec<Placement>>) {
    let (host, rx, _wanted) = start_reporting(dir);

    (host, rx)
}

/// A plugin that shows the data of its latest `ui.composer` update.
fn write_follower(dir: &std::path::Path, id: &str) {
    let folder = dir.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("beep-plugin.json"),
        format!(
            r#"{{ "id": "{id}", "name": "T", "version": "0.1.0", "permissions": ["ui.add"] }}"#
        ),
    )
    .unwrap();
    std::fs::write(
        folder.join("main.ts"),
        r#"
        const seen = beep.state("none");
        beep.on({ name: "ui.composer" }, (event) => seen.set(JSON.stringify(event.data)));
        beep.place({ slot: "composer" }, () => beep.text(seen.get()));
        "#,
    )
    .unwrap();
}

fn composer_topic() -> Topic {
    Topic {
        name: "ui.composer".to_owned(),
        key: None,
        machine: None,
    }
}

fn composer_update(data: serde_json::Value) -> Publication {
    Publication {
        topic: composer_topic(),
        machine: None,
        data,
    }
}

/// Waits for a report of the wanted topics that satisfies `accept`.
fn wait_wanted(rx: &mpsc::Receiver<Vec<Topic>>, accept: impl Fn(&[Topic]) -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let topics = rx.recv_timeout(left).expect("no matching report in time");
        if accept(&topics) {
            return;
        }
    }
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

#[test]
fn a_published_update_reaches_a_subscribed_plugin_and_reruns_render() {
    let root = tempfile::tempdir().unwrap();
    write_follower(root.path(), "follower");
    let (host, rx, wanted) = start_reporting(root.path());
    wait_wanted(&wanted, |topics| topics == [composer_topic()]);
    wait_for(&rx, |p| texts(p) == ["none"]);

    host.publish(&composer_update(json!({ "session": "s1" })))
        .unwrap();
    wait_for(&rx, |p| texts(p) == [r#"{"session":"s1"}"#]);

    host.publish(&composer_update(json!(null))).unwrap();
    wait_for(&rx, |p| texts(p) == ["null"]);
}

#[test]
fn a_late_subscriber_gets_the_latest_ui_value() {
    let root = tempfile::tempdir().unwrap();
    let (host, rx, _wanted) = start_reporting(root.path());
    host.publish(&composer_update(json!("first"))).unwrap();
    host.publish(&composer_update(json!("second"))).unwrap();

    write_follower(root.path(), "follower");

    wait_for(&rx, |p| texts(p) == [r#""second""#]);
}

#[test]
fn a_reloaded_plugin_gets_the_latest_ui_value_again() {
    let root = tempfile::tempdir().unwrap();
    write_follower(root.path(), "follower");
    let (host, rx, wanted) = start_reporting(root.path());
    wait_wanted(&wanted, |topics| topics == [composer_topic()]);
    host.publish(&composer_update(json!("kept"))).unwrap();
    wait_for(&rx, |p| texts(p) == [r#""kept""#]);

    std::fs::write(
        root.path().join("follower/main.ts"),
        r#"
        beep.on({ name: "ui.composer" }, (event) => seen.set("again " + event.data));
        const seen = beep.state("none");
        beep.place({ slot: "composer" }, () => beep.text(seen.get()));
        "#,
    )
    .unwrap();

    wait_for(&rx, |p| texts(p) == ["again kept"]);
}

#[test]
fn updates_for_topics_nobody_follows_are_dropped() {
    let root = tempfile::tempdir().unwrap();
    write_follower(root.path(), "follower");
    let (host, rx, wanted) = start_reporting(root.path());
    wait_wanted(&wanted, |topics| topics == [composer_topic()]);

    let other = Publication {
        topic: Topic {
            name: "ui.other".to_owned(),
            key: None,
            machine: None,
        },
        machine: None,
        data: json!("x"),
    };
    host.publish(&other).unwrap();
    host.publish(&composer_update(json!("y"))).unwrap();

    wait_for(&rx, |p| texts(p) == [r#""y""#]);
}

#[test]
fn refused_and_oversized_updates_are_not_published() {
    let root = tempfile::tempdir().unwrap();
    let (host, _rx, _wanted) = start_reporting(root.path());
    let mut update = composer_update(json!(null));

    update.topic.name = "machines".to_owned();
    assert!(matches!(host.publish(&update), Err(TopicError::Unknown(_))));

    update.topic.name = "ui.composer".to_owned();
    update.data = json!("x".repeat(300 * 1024));
    assert!(matches!(
        host.publish(&update),
        Err(TopicError::TooLarge(_))
    ));
}

#[test]
fn the_host_reports_wanted_topics_on_subscribe_and_unload() {
    let root = tempfile::tempdir().unwrap();
    let (host, _rx, wanted) = start_reporting(root.path());
    assert!(host.wanted().is_empty());

    write_follower(root.path(), "follower");
    wait_wanted(&wanted, |topics| topics == [composer_topic()]);
    assert_eq!(host.wanted(), [composer_topic()]);

    std::fs::remove_dir_all(root.path().join("follower")).unwrap();
    wait_wanted(&wanted, <[Topic]>::is_empty);
    assert!(host.wanted().is_empty());
}

#[test]
fn a_plugin_that_fails_follows_nothing() {
    let root = tempfile::tempdir().unwrap();
    write_follower(root.path(), "follower");
    let (_host, _rx, wanted) = start_reporting(root.path());
    wait_wanted(&wanted, |topics| topics == [composer_topic()]);

    std::fs::write(
        root.path().join("follower/main.ts"),
        "throw new Error('boom');",
    )
    .unwrap();

    wait_wanted(&wanted, <[Topic]>::is_empty);
}
