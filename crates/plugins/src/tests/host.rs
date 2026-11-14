use std::sync::mpsc;

use serde_json::json;

use super::*;
use crate::event::Action;
use crate::instance::{
    ComposerContext, Instance, InstanceKey, InstanceRef, SessionHeaderContext, SessionRowContext,
    SlotContext,
};
use crate::ui::{CallbackId, Node, Slot};

const TIMEOUT: Duration = Duration::from_secs(30);

fn key(text: &str) -> InstanceKey {
    InstanceKey::try_from(text.to_owned()).unwrap()
}

fn composer_instance() -> Instance {
    Instance {
        key: key("composer"),
        context: SlotContext::Composer(ComposerContext {
            machine: "build-box".to_owned(),
            session: "s1".to_owned(),
        }),
    }
}

fn row_instance(session: &str, title: &str) -> Instance {
    Instance {
        key: key(session),
        context: SlotContext::SessionRow(SessionRowContext {
            machine: "build-box".to_owned(),
            session: session.to_owned(),
            title: title.to_owned(),
        }),
    }
}

fn header_instance(session: &str) -> Instance {
    Instance {
        key: key(session),
        context: SlotContext::SessionHeader(SessionHeaderContext {
            machine: "build-box".to_owned(),
            session: session.to_owned(),
        }),
    }
}

fn batch(shown: Vec<Instance>, gone: Vec<InstanceRef>) -> InstanceBatch {
    InstanceBatch {
        reset: false,
        shown,
        gone,
    }
}

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
    host.set_instances(batch(vec![composer_instance()], vec![]))
        .unwrap();

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
            Node::Text { text, .. } => Some(text.clone()),
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

fn write_custom(dir: &std::path::Path, id: &str, main: &str) {
    write_plugin(dir, id, "unused");
    std::fs::write(dir.join(id).join("main.ts"), main).unwrap();
}

fn gone(slot: Slot, text: &str) -> InstanceRef {
    InstanceRef {
        slot,
        key: key(text),
    }
}

/// The text of every text node, with the instance it belongs to.
fn by_instance(placements: &[Placement]) -> Vec<(String, String)> {
    placements
        .iter()
        .filter_map(|placement| match &placement.node {
            Node::Text { text, .. } => Some((placement.instance.to_string(), text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn instances_from_the_page_are_rendered_and_dropped_when_they_go() {
    let root = tempfile::tempdir().unwrap();
    write_custom(
        root.path(),
        "rows-one",
        r#"beep.place({ slot: "session.row" }, (row) => beep.text(row.title));"#,
    );
    let (host, rx) = start(root.path());

    host.set_instances(batch(
        vec![row_instance("a", "Alpha"), row_instance("b", "Beta")],
        vec![],
    ))
    .unwrap();
    let placements = wait_for(&rx, |p| p.len() == 2);
    assert_eq!(
        by_instance(&placements),
        [
            ("a".to_owned(), "Alpha".to_owned()),
            ("b".to_owned(), "Beta".to_owned())
        ]
    );
    assert_eq!(host.placements(), placements);

    host.set_instances(batch(vec![], vec![gone(Slot::SessionRow, "a")]))
        .unwrap();
    let placements = wait_for(&rx, |p| p.len() == 1);
    assert_eq!(
        by_instance(&placements),
        [("b".to_owned(), "Beta".to_owned())]
    );
}

#[test]
fn a_page_that_starts_over_resets_the_instances() {
    let root = tempfile::tempdir().unwrap();
    write_custom(
        root.path(),
        "rows-one",
        r#"beep.place({ slot: "session.row" }, (row) => beep.text(row.title));"#,
    );
    let (host, rx) = start(root.path());
    host.set_instances(batch(vec![row_instance("a", "Alpha")], vec![]))
        .unwrap();
    wait_for(&rx, |p| p.len() == 1);

    let mut fresh = batch(vec![row_instance("z", "Zed")], vec![]);
    fresh.reset = true;
    host.set_instances(fresh).unwrap();

    let placements = wait_for(&rx, |p| {
        by_instance(p) == [("z".to_owned(), "Zed".to_owned())]
    });
    assert_eq!(placements.len(), 1);
}

#[test]
fn a_plugin_started_later_gets_the_instances_already_on_the_page() {
    let root = tempfile::tempdir().unwrap();
    let (host, rx) = start(root.path());
    host.set_instances(batch(vec![row_instance("a", "Alpha")], vec![]))
        .unwrap();

    write_custom(
        root.path(),
        "rows-one",
        r#"beep.place({ slot: "session.row" }, (row) => beep.text(row.title));"#,
    );

    wait_for(&rx, |p| {
        by_instance(p) == [("a".to_owned(), "Alpha".to_owned())]
    });
}

/// A plugin with a header button that counts presses, for the event tests.
const COUNTER: &str = r#"
    const count = beep.state(0);
    beep.place({ slot: "session.header" }, () =>
      beep.button({ label: "Count", onPress: () => count.set(count.get() + 1) },
        [beep.text("n:" + count.get())]));
"#;

fn button_id(placements: &[Placement]) -> CallbackId {
    match placements.first().map(|placement| &placement.node) {
        Some(Node::Button { on_press, .. }) => *on_press,
        other => panic!("expected a button, got {other:?}"),
    }
}

fn button_text(placements: &[Placement]) -> String {
    match placements.first().map(|placement| &placement.node) {
        Some(Node::Button { children, .. }) => match children.first() {
            Some(Node::Text { text, .. }) => text.clone(),
            other => panic!("expected text, got {other:?}"),
        },
        other => panic!("expected a button, got {other:?}"),
    }
}

fn press(id: CallbackId) -> UiEvent {
    UiEvent {
        plugin: "counter-one".parse().unwrap(),
        id,
        action: Action::Press,
    }
}

/// Starts the counter with a header open. Returns what it drew first.
fn start_counter(root: &std::path::Path) -> (Host, mpsc::Receiver<Vec<Placement>>, Vec<Placement>) {
    write_custom(root, "counter-one", COUNTER);
    let (host, rx) = start(root);
    host.set_instances(batch(vec![header_instance("s1")], vec![]))
        .unwrap();
    let first = wait_for(&rx, |p| button_text(p) == "n:0");

    (host, rx, first)
}

#[test]
fn a_button_press_runs_the_callback_and_the_plugin_draws_again() {
    let root = tempfile::tempdir().unwrap();
    let (host, rx, first) = start_counter(root.path());

    host.event(press(button_id(&first))).unwrap();
    let second = wait_for(&rx, |p| button_text(p) == "n:1");

    assert_ne!(button_id(&second), button_id(&first));
}

#[test]
fn an_old_id_does_nothing_and_events_for_unknown_plugins_are_dropped() {
    let root = tempfile::tempdir().unwrap();
    let (host, rx, first) = start_counter(root.path());

    host.event(press(button_id(&first))).unwrap();
    let second = wait_for(&rx, |p| button_text(p) == "n:1");

    // The first id is stale now. The events are handled in order, so by the
    // time the third press shows, the stale one has been seen and ignored.
    host.event(press(button_id(&first))).unwrap();
    let mut other = press(button_id(&second));
    other.plugin = "someone-else".parse().unwrap();
    host.event(other).unwrap();
    host.event(press(button_id(&second))).unwrap();

    wait_for(&rx, |p| button_text(p) == "n:2");
}

#[test]
fn oversized_text_is_refused_before_it_reaches_a_plugin() {
    let root = tempfile::tempdir().unwrap();
    let (host, _rx, _first) = start_counter(root.path());
    let event = UiEvent {
        plugin: "counter-one".parse().unwrap(),
        id: CallbackId::from(1),
        action: Action::Text {
            value: "x".repeat(5000),
        },
    };

    assert!(matches!(host.event(event), Err(EventError::TooLong(5000))));
}

#[test]
fn a_plugin_gets_at_most_thirty_events_a_second() {
    let root = tempfile::tempdir().unwrap();
    write_custom(
        root.path(),
        "counter-one",
        r#"
        // The press does not draw, so the button keeps its id.
        let count = 0;
        const shown = beep.state(0);
        const tick = () => { shown.set(count); beep.after(50, tick); };
        beep.after(50, tick);
        beep.place({ slot: "session.header" }, () =>
          beep.button({ label: "Count", onPress: () => { count += 1; } },
            [beep.text("n:" + shown.get())]));
        "#,
    );
    let (host, rx) = start(root.path());
    host.set_instances(batch(vec![header_instance("s1")], vec![]))
        .unwrap();
    let first = wait_for(&rx, |p| !p.is_empty());
    let id = button_id(&first);

    for _ in 0..100 {
        host.event(press(id)).unwrap();
    }

    wait_for(&rx, |p| button_text(p) == "n:30");
}
