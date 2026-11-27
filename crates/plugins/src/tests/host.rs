use std::sync::mpsc;

use serde_json::json;

use super::*;
use crate::event::Action;
use crate::ident::{Permission, Slot};
use crate::install::{Builtin, PreviewKind};
use crate::instance::{
    ComposerContext, Instance, InstanceKey, InstanceRef, SessionHeaderContext, SessionRowContext,
    SlotContext,
};
use crate::managed::Origin;
use crate::state::PluginState;
use crate::status::{Info, PluginStatus};
use crate::style::StyleToken;
use crate::ui::{CallbackId, Node};

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
        dir.join(id).join("main.js"),
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
    let host = Host::start(
        dir.to_path_buf(),
        Reports { shown, wanted },
        Policy::Dev,
        &[],
    )
    .unwrap();
    host.set_visible("test", true);
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
        folder.join("main.js"),
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
        root.path().join("bad-one/main.js"),
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
        root.path().join("follower/main.js"),
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
        root.path().join("follower/main.js"),
        "throw new Error('boom');",
    )
    .unwrap();

    wait_wanted(&wanted, <[Topic]>::is_empty);
}

fn write_custom(dir: &std::path::Path, id: &str, main: &str) {
    write_plugin(dir, id, "unused");
    std::fs::write(dir.join(id).join("main.js"), main).unwrap();
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

struct Quiet;

impl Handler for Quiet {
    fn shown(&self, _placements: &[Placement]) {}

    fn wanted(&self, _topics: &[Topic]) {}
}

fn write_lazy(dir: &std::path::Path, id: &str, activation: &str, main: &str) {
    let folder = dir.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("beep-plugin.json"),
        format!(
            r#"{{ "id": "{id}", "name": "Lazy", "version": "0.3.0", "permissions": ["ui.add"], "activation": {activation} }}"#
        ),
    )
    .unwrap();
    std::fs::write(folder.join("main.js"), main).unwrap();
}

/// A supervisor driven by hand, with no loop and no file watcher. Plugin threads are real.
fn driven(dir: &std::path::Path) -> (Supervisor, Arc<Published>, mpsc::Receiver<Event>) {
    let (events, inbox) = mpsc::channel();
    let published = Arc::new(Published::default());
    let dir = dir.canonicalize().unwrap();
    let supervisor = Supervisor::new(
        dir.clone(),
        Arc::new(Managed::load(&dir, Policy::Dev)),
        events,
        Arc::new(Quiet),
        Arc::clone(&published),
    );

    (supervisor, published, inbox)
}

fn state_of(published: &Published, id: &str) -> PluginState {
    let id: PluginId = id.parse().unwrap();
    let list = locked(&published.registry).list();

    list.into_iter()
        .find(|status| status.id == id)
        .unwrap()
        .state
}

fn show_composer(supervisor: &mut Supervisor) {
    supervisor.handle(Event::Instances(batch(vec![composer_instance()], vec![])));
}

fn hide_composer(supervisor: &mut Supervisor) {
    supervisor.handle(Event::Instances(batch(
        vec![],
        vec![gone(Slot::Composer, "composer")],
    )));
}

const LAZY: &str = r#"beep.place({ slot: "composer" }, () => beep.text("hi"));"#;

#[test]
fn a_slot_activated_plugin_waits_until_its_slot_appears() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(root.path(), "lazy-one", r#"["slot:composer"]"#, LAZY);
    let (mut supervisor, published, _events) = driven(root.path());
    let now = Instant::now();

    supervisor.load_all(now);
    supervisor.reconcile(now);
    assert_eq!(state_of(&published, "lazy-one"), PluginState::Waiting);
    assert!(supervisor.running.is_empty());

    // A slot it does not list does not start it.
    supervisor.handle(Event::Instances(batch(
        vec![row_instance("s1", "T")],
        vec![],
    )));
    supervisor.reconcile(now);
    assert!(supervisor.running.is_empty());

    show_composer(&mut supervisor);
    supervisor.reconcile(now);
    assert_eq!(supervisor.running.len(), 1);
    assert_eq!(state_of(&published, "lazy-one"), PluginState::Paused);
}

#[test]
fn a_plugin_without_activation_starts_at_launch() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "eager-one", "hi");
    let (mut supervisor, published, _events) = driven(root.path());

    supervisor.load_all(Instant::now());

    assert_eq!(supervisor.running.len(), 1);
    assert_ne!(state_of(&published, "eager-one"), PluginState::Waiting);
}

#[test]
fn a_topic_activated_plugin_starts_on_the_first_publication_and_stays() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(root.path(), "lazy-two", r#"["topic:ui.composer"]"#, LAZY);
    let (mut supervisor, published, _events) = driven(root.path());
    let now = Instant::now();
    supervisor.load_all(now);
    assert_eq!(state_of(&published, "lazy-two"), PluginState::Waiting);

    let delivery = Delivery::new(&composer_update(json!(null))).unwrap();
    supervisor.handle(Event::Publish(delivery));
    supervisor.reconcile(now);
    assert_eq!(supervisor.running.len(), 1);

    // No slots to lose, so it never idles out.
    supervisor.reconcile(now + Duration::from_secs(3600));
    assert_eq!(supervisor.running.len(), 1);
}

#[test]
fn a_plugin_stops_after_its_slots_were_gone_for_the_grace_period() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(root.path(), "lazy-one", r#"["slot:composer"]"#, LAZY);
    let (mut supervisor, published, _events) = driven(root.path());
    let t0 = Instant::now();
    supervisor.load_all(t0);
    show_composer(&mut supervisor);
    supervisor.reconcile(t0);
    assert_eq!(supervisor.next_idle_stop(), None);

    hide_composer(&mut supervisor);
    supervisor.reconcile(t0);
    assert_eq!(supervisor.next_idle_stop(), Some(t0 + IDLE_GRACE));
    supervisor.reconcile(t0 + IDLE_GRACE - Duration::from_secs(1));
    assert_eq!(supervisor.running.len(), 1);

    supervisor.reconcile(t0 + IDLE_GRACE);
    assert!(supervisor.running.is_empty());
    assert_eq!(state_of(&published, "lazy-one"), PluginState::Waiting);
    assert_eq!(supervisor.next_idle_stop(), None);

    // The slot coming back starts it again.
    show_composer(&mut supervisor);
    supervisor.reconcile(t0 + IDLE_GRACE);
    assert_eq!(supervisor.running.len(), 1);
}

#[test]
fn a_plugin_failure_clears_its_idle_deadline() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(root.path(), "lazy-one", r#"["slot:composer"]"#, LAZY);
    let (mut supervisor, published, _events) = driven(root.path());
    let now = Instant::now();
    supervisor.load_all(now);
    show_composer(&mut supervisor);
    supervisor.reconcile(now);

    hide_composer(&mut supervisor);
    supervisor.reconcile(now);
    assert_eq!(supervisor.next_idle_stop(), Some(now + IDLE_GRACE));

    let id: PluginId = "lazy-one".parse().unwrap();
    let generation = supervisor.running[&id].generation;
    supervisor.handle(Event::Stopped {
        id,
        generation,
        reason: "stopped".into(),
    });
    supervisor.reconcile(now + IDLE_GRACE);

    assert_eq!(supervisor.next_idle_stop(), None);
    assert!(supervisor.running.is_empty());
    assert!(matches!(
        state_of(&published, "lazy-one"),
        PluginState::Stopped { .. }
    ));
}

#[test]
fn a_slot_that_comes_back_in_time_restarts_the_grace_period() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(root.path(), "lazy-one", r#"["slot:composer"]"#, LAZY);
    let (mut supervisor, _published, _events) = driven(root.path());
    let t0 = Instant::now();
    supervisor.load_all(t0);
    show_composer(&mut supervisor);
    supervisor.reconcile(t0);

    hide_composer(&mut supervisor);
    supervisor.reconcile(t0);
    show_composer(&mut supervisor);
    supervisor.reconcile(t0 + Duration::from_secs(30));
    assert_eq!(supervisor.next_idle_stop(), None);

    hide_composer(&mut supervisor);
    supervisor.reconcile(t0 + Duration::from_secs(40));
    supervisor.reconcile(t0 + Duration::from_secs(70));

    assert_eq!(
        supervisor.running.len(),
        1,
        "60 s have not passed since it left"
    );
    assert_eq!(
        supervisor.next_idle_stop(),
        Some(t0 + Duration::from_secs(100))
    );
}

#[test]
fn a_startup_plugin_never_idles_out() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(
        root.path(),
        "eager-two",
        r#"["startup", "slot:composer"]"#,
        LAZY,
    );
    let (mut supervisor, _published, _events) = driven(root.path());
    let t0 = Instant::now();
    supervisor.load_all(t0);

    supervisor.reconcile(t0 + Duration::from_secs(3600));

    assert_eq!(supervisor.running.len(), 1);
    assert_eq!(supervisor.next_idle_stop(), None);
}

#[test]
fn status_names_each_state_and_the_reason_a_plugin_stopped() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(root.path(), "lazy-one", r#"["slot:composer"]"#, LAZY);
    write_lazy(
        root.path(),
        "greedy-one",
        r#"["startup"]"#,
        "throw new Error(\"nope\");",
    );
    let folder = root.path().join("broken-one");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("beep-plugin.json"), "{ not json").unwrap();
    let (mut supervisor, published, events) = driven(root.path());
    supervisor.load_all(Instant::now());

    // Wait for the failing plugin's stop, as the loop would hear it.
    loop {
        let event = events.recv_timeout(TIMEOUT).unwrap();
        let stopped = matches!(event, Event::Stopped { .. });
        supervisor.handle(event);
        if stopped {
            break;
        }
    }
    let list = locked(&published.registry).list();
    let find = |id: &str| {
        list.iter()
            .find(|status| status.id.to_string() == id)
            .unwrap()
    };

    assert_eq!(find("lazy-one").state, PluginState::Waiting);
    assert_eq!(find("lazy-one").info.name, "Lazy");
    assert_eq!(find("lazy-one").info.version, semver::Version::new(0, 3, 0));
    let PluginState::Stopped { reason } = &find("greedy-one").state else {
        panic!("expected a stopped plugin");
    };
    assert!(reason.contains("nope"), "{reason}");
    let PluginState::Stopped { reason } = &find("broken-one").state else {
        panic!("expected a stopped plugin");
    };
    assert!(reason.contains("invalid"), "{reason}");
}

#[test]
fn status_serializes_the_state_and_reason_flat() {
    let status = PluginStatus {
        id: "lazy-one".parse().unwrap(),
        info: Info {
            name: "Lazy".to_owned(),
            version: semver::Version::new(1, 0, 0),
            description: String::new(),
            permissions: vec![Permission::UiAdd],
            approved: vec![Permission::UiAdd],
            source: Origin::Dev,
            tools: Vec::new(),
            style: Default::default(),
            enabled: true,
        },
        state: PluginState::Stopped {
            reason: "used 600 ms".to_owned(),
        },
    };

    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        json!({ "id": "lazy-one", "name": "Lazy", "version": "1.0.0", "description": "",
                "permissions": ["ui.add"], "approved": ["ui.add"], "source": { "kind": "dev" },
                "tools": [], "style": {}, "enabled": true, "state": "stopped", "reason": "used 600 ms" })
    );
    let running = PluginStatus {
        state: PluginState::Paused,
        ..status
    };
    assert_eq!(serde_json::to_value(&running).unwrap()["state"], "paused");
}

#[test]
fn running_plugins_are_paused_while_no_window_is_visible() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "eager-one", "hi");
    let (mut supervisor, published, _events) = driven(root.path());
    supervisor.load_all(Instant::now());
    assert_eq!(state_of(&published, "eager-one"), PluginState::Paused);

    supervisor.handle(Event::Visible {
        window: "bar".to_owned(),
        visible: true,
    });
    supervisor.handle(Event::Visible {
        window: "center".to_owned(),
        visible: true,
    });
    assert_eq!(state_of(&published, "eager-one"), PluginState::Running);

    // One window hiding is not enough, and the last one pauses.
    supervisor.handle(Event::Visible {
        window: "bar".to_owned(),
        visible: false,
    });
    assert_eq!(state_of(&published, "eager-one"), PluginState::Running);
    supervisor.handle(Event::Visible {
        window: "center".to_owned(),
        visible: false,
    });
    assert_eq!(state_of(&published, "eager-one"), PluginState::Paused);
    let running = supervisor.running.values().next().unwrap();
    assert!(running.hidden.load(Ordering::Acquire));
}

#[test]
fn the_host_serves_the_images_that_placements_use_and_stops_when_they_go() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("pics-one");
    std::fs::create_dir_all(folder.join("art")).unwrap();
    std::fs::write(
        folder.join("beep-plugin.json"),
        r#"{ "id": "pics-one", "name": "P", "version": "0.1.0", "permissions": ["ui.add"] }"#,
    )
    .unwrap();
    std::fs::write(folder.join("art/a.png"), b"\x89PNG\r\n\x1a\nbytes").unwrap();
    std::fs::write(
        folder.join("main.js"),
        r#"beep.place({ slot: "composer" }, () => beep.image({ src: "art/a.png", width: 1, height: 1 }));"#,
    )
    .unwrap();
    let (host, rx) = start(root.path());

    let placements = wait_for(&rx, |p| !p.is_empty());

    let Node::Image { asset, .. } = &placements[0].node else {
        panic!("expected an image");
    };
    let served = host.asset(&asset.hash).expect("the image is served");
    assert_eq!(served.bytes(), b"\x89PNG\r\n\x1a\nbytes");
    assert_eq!(served.hash(), &asset.hash);
    assert!(host.asset(&"0".repeat(64).parse().unwrap()).is_none());

    std::fs::remove_dir_all(&folder).unwrap();
    wait_for(&rx, <[Placement]>::is_empty);
    assert!(host.asset(&asset.hash).is_none());
}

#[test]
fn a_started_host_lists_its_plugins_at_once() {
    let root = tempfile::tempdir().unwrap();
    write_lazy(root.path(), "lazy-one", r#"["slot:composer"]"#, LAZY);
    let (host, _rx) = start(root.path());

    let list = host.status();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].state, PluginState::Waiting);
}

fn start_with(dir: &std::path::Path, policy: Policy) -> (Host, mpsc::Receiver<Vec<Placement>>) {
    start_with_builtins(dir, policy, &[])
}

fn start_with_builtins(
    dir: &std::path::Path,
    policy: Policy,
    builtins: &[Builtin],
) -> (Host, mpsc::Receiver<Vec<Placement>>) {
    let (shown, shown_rx) = mpsc::channel();
    let (wanted, _wanted_rx) = mpsc::channel();
    let host = Host::start(
        dir.to_path_buf(),
        Reports { shown, wanted },
        policy,
        builtins,
    )
    .unwrap();
    host.set_visible("test", true);
    host.set_instances(batch(vec![composer_instance()], vec![]))
        .unwrap();

    (host, shown_rx)
}

fn status_of(host: &Host, id: &str) -> Option<PluginStatus> {
    let id: PluginId = id.parse().unwrap();

    host.status().into_iter().find(|status| status.id == id)
}

fn plugin_id(id: &str) -> PluginId {
    id.parse().unwrap()
}

#[test]
fn a_managed_folder_without_a_record_waits_for_approval() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");

    let (host, _rx) = start_with(root.path(), Policy::Managed);

    let status = status_of(&host, "cat-one").unwrap();
    assert_eq!(
        status.state,
        PluginState::NeedsApproval {
            missing: vec![Permission::UiAdd]
        }
    );
    assert_eq!(status.info.source, Origin::Local);
    assert!(host.placements().is_empty());
}

#[test]
fn a_dev_folder_runs_without_a_record_and_says_so() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");

    let (host, rx) = start_with(root.path(), Policy::Dev);

    wait_for(&rx, |p| !p.is_empty());
    let status = status_of(&host, "cat-one").unwrap();
    assert_eq!(status.state, PluginState::Running);
    assert_eq!(status.info.source, Origin::Dev);
    assert_eq!(status.info.approved, [Permission::UiAdd]);
}

#[test]
fn approving_starts_the_plugin() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");
    let (host, rx) = start_with(root.path(), Policy::Managed);

    host.approve(&plugin_id("cat-one")).unwrap();

    assert_eq!(
        status_of(&host, "cat-one").unwrap().state,
        PluginState::Running
    );
    assert_eq!(texts(&wait_for(&rx, |p| !p.is_empty())), ["meow"]);
    let record = host.record(&plugin_id("cat-one")).unwrap();
    assert!(record.enabled);
    assert_eq!(record.approved, [Permission::UiAdd]);
}

#[test]
fn disabling_stops_the_plugin_and_enabling_starts_it_again() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");
    let (host, rx) = start_with(root.path(), Policy::Dev);
    wait_for(&rx, |p| !p.is_empty());

    host.set_enabled(&plugin_id("cat-one"), false).unwrap();

    let status = status_of(&host, "cat-one").unwrap();
    assert_eq!(status.state, PluginState::Disabled);
    assert!(!status.info.enabled);
    wait_for(&rx, <[Placement]>::is_empty);
    assert!(host.placements().is_empty());

    host.set_enabled(&plugin_id("cat-one"), true).unwrap();

    assert_eq!(
        status_of(&host, "cat-one").unwrap().state,
        PluginState::Running
    );
    wait_for(&rx, |p| !p.is_empty());
}

#[test]
fn a_plugin_that_gains_a_permission_stops_until_allowed() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");
    let (host, rx) = start_with(root.path(), Policy::Managed);
    host.approve(&plugin_id("cat-one")).unwrap();
    wait_for(&rx, |p| !p.is_empty());

    std::fs::write(
        root.path().join("cat-one/beep-plugin.json"),
        r#"{ "id": "cat-one", "name": "T", "version": "0.2.0",
             "permissions": ["ui.add", "sessions.read"] }"#,
    )
    .unwrap();
    host.set_enabled(&plugin_id("cat-one"), true).unwrap();

    assert_eq!(
        status_of(&host, "cat-one").unwrap().state,
        PluginState::NeedsApproval {
            missing: vec![Permission::SessionsRead]
        }
    );
    wait_for(&rx, <[Placement]>::is_empty);

    let shown = host.preview_allow(&plugin_id("cat-one")).unwrap();
    assert_eq!(shown.added, [Permission::SessionsRead]);
    host.approve(&plugin_id("cat-one")).unwrap();
    assert_eq!(
        status_of(&host, "cat-one").unwrap().state,
        PluginState::Running
    );
}

#[test]
fn removing_deletes_the_folder_and_the_record() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");
    let (host, rx) = start_with(root.path(), Policy::Dev);
    wait_for(&rx, |p| !p.is_empty());
    host.set_enabled(&plugin_id("cat-one"), true).unwrap();

    host.remove(&plugin_id("cat-one")).unwrap();

    assert!(!root.path().join("cat-one").exists());
    assert!(host.record(&plugin_id("cat-one")).is_none());
    assert!(status_of(&host, "cat-one").is_none());
    wait_for(&rx, <[Placement]>::is_empty);
    assert!(matches!(
        host.remove(&plugin_id("cat-one")),
        Err(ManageError::Unknown(_))
    ));
}

#[test]
fn installing_a_prepared_folder_runs_it_and_an_update_replaces_it() {
    let root = tempfile::tempdir().unwrap();
    let (host, rx) = start_with(root.path(), Policy::Managed);
    let origin = |commit: &str| {
        Origin::Git(crate::managed::GitOrigin {
            url: "https://github.com/owner/repo.git".to_owned(),
            reference: None,
            subfolder: None,
            commit: commit.parse().unwrap(),
        })
    };
    let staged = root.path().join(".stage-one");
    write_plugin(root.path(), ".stage-one", "one");
    std::fs::write(
        staged.join("beep-plugin.json"),
        r#"{ "id": "cat-one", "name": "T", "version": "0.1.0", "permissions": ["ui.add"] }"#,
    )
    .unwrap();

    let shown = host.preview_install(&staged, &origin("aaaaaaa")).unwrap();
    assert_eq!(shown.kind, PreviewKind::New);
    host.install(&staged, origin("aaaaaaa")).unwrap();

    assert_eq!(
        status_of(&host, "cat-one").unwrap().state,
        PluginState::Running
    );
    assert_eq!(texts(&wait_for(&rx, |p| !p.is_empty())), ["one"]);

    let update = root.path().join(".stage-two");
    write_plugin(root.path(), ".stage-two", "two");
    std::fs::write(
        update.join("beep-plugin.json"),
        r#"{ "id": "cat-one", "name": "T", "version": "0.2.0", "permissions": ["ui.add"] }"#,
    )
    .unwrap();
    let other = root.path().join(".stage-three");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::copy(
        update.join("beep-plugin.json"),
        other.join("beep-plugin.json"),
    )
    .unwrap();
    assert!(matches!(
        host.install(&other, Origin::Local),
        Err(ManageError::AlreadyInstalled(_))
    ));

    host.install(&update, origin("bbbbbbb")).unwrap();

    let status = status_of(&host, "cat-one").unwrap();
    assert_eq!(status.info.version, semver::Version::new(0, 2, 0));
    assert_eq!(status.info.source, origin("bbbbbbb"));
    assert_eq!(texts(&wait_for(&rx, |p| texts(p) == ["two"])), ["two"]);
}

#[test]
fn a_dot_folder_is_never_a_plugin() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), ".stage-one", "meow");
    write_plugin(root.path(), "cat-one", "meow");

    let (host, _rx) = start_with(root.path(), Policy::Dev);

    let ids: Vec<String> = host.status().iter().map(|s| s.id.to_string()).collect();
    assert_eq!(ids, ["cat-one"]);
}

#[test]
fn the_new_states_serialize_by_name() {
    let disabled = serde_json::to_value(PluginState::Disabled).unwrap();
    let waiting = serde_json::to_value(PluginState::NeedsApproval {
        missing: vec![Permission::SessionsRead],
    })
    .unwrap();

    assert_eq!(disabled, json!({ "state": "disabled" }));
    assert_eq!(
        waiting,
        json!({ "state": "needs-approval", "missing": ["sessions.read"] })
    );
}

struct StatusWatch(mpsc::Sender<Vec<PluginStatus>>);

impl Handler for StatusWatch {
    fn shown(&self, _placements: &[Placement]) {}

    fn wanted(&self, _topics: &[Topic]) {}

    fn status_changed(&self, statuses: &[PluginStatus]) {
        let _ = self.0.send(statuses.to_vec());
    }
}

#[test]
fn the_handler_hears_when_a_status_changes() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");
    let (changed, heard) = mpsc::channel();
    let host = Host::start(
        root.path().to_path_buf(),
        StatusWatch(changed),
        Policy::Managed,
        &[],
    )
    .unwrap();
    while heard.try_recv().is_ok() {}

    host.approve(&plugin_id("cat-one")).unwrap();

    let paused = |statuses: &[PluginStatus]| {
        statuses
            .iter()
            .any(|status| status.id == plugin_id("cat-one") && status.state == PluginState::Paused)
    };
    loop {
        let statuses = heard.recv_timeout(TIMEOUT).expect("no status change heard");
        if paused(&statuses) {
            break;
        }
    }
    assert_eq!(
        status_of(&host, "cat-one").unwrap().state,
        PluginState::Paused
    );
}

const BUILTIN_V1: &[(&str, &[u8])] = &[
    (
        "beep-plugin.json",
        br#"{ "id": "own-cat", "name": "Own", "version": "1.0.0", "permissions": ["ui.add"] }"#,
    ),
    (
        "main.js",
        br#"beep.place({ slot: "composer" }, () => beep.text("one"));"#,
    ),
    ("art/frame.png", b"png"),
];

const BUILTIN_V2: &[(&str, &[u8])] = &[
    (
        "beep-plugin.json",
        br#"{ "id": "own-cat", "name": "Own", "version": "2.0.0", "permissions": ["ui.add", "sessions.read"] }"#,
    ),
    (
        "main.js",
        br#"beep.place({ slot: "composer" }, () => beep.text("two"));"#,
    ),
];

fn own_cat(files: &'static [(&'static str, &'static [u8])]) -> [Builtin; 1] {
    [Builtin {
        id: plugin_id("own-cat"),
        files,
    }]
}

#[test]
fn a_built_in_replaces_an_old_typescript_entry_with_compiled_javascript() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("own-cat");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(
        dir.join("beep-plugin.json"),
        r#"{ "id": "own-cat", "name": "Own", "version": "0.1.0", "entry": "main.ts" }"#,
    )
    .unwrap();
    std::fs::write(dir.join("main.ts"), "const old: number = 1;").unwrap();

    let (_host, _rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(BUILTIN_V1));

    assert!(dir.join("main.js").is_file());
    assert!(!dir.join("main.ts").exists());
    assert_eq!(
        Manifest::load(&dir).unwrap().entry,
        PathBuf::from("main.js")
    );
}

#[test]
fn a_built_in_is_installed_allowed_and_off_at_first_start() {
    let root = tempfile::tempdir().unwrap();

    let (host, rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(BUILTIN_V1));

    let status = status_of(&host, "own-cat").unwrap();
    assert_eq!(status.info.source, Origin::Builtin);
    assert_eq!(status.info.approved, [Permission::UiAdd]);
    assert!(!status.info.enabled);
    assert!(root.path().join("own-cat/art/frame.png").is_file());

    host.set_enabled(&plugin_id("own-cat"), true).unwrap();
    wait_for(&rx, |placements| texts(placements) == ["one"]);
}

#[test]
fn a_changed_built_in_replaces_the_copy_and_keeps_the_off_switch() {
    let root = tempfile::tempdir().unwrap();
    let id = plugin_id("own-cat");
    let (host, _rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(BUILTIN_V1));
    host.set_enabled(&id, false).unwrap();
    drop(host);
    std::fs::write(root.path().join("own-cat/main.js"), "// edited").unwrap();

    let (host, _rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(BUILTIN_V2));

    let status = status_of(&host, "own-cat").unwrap();
    assert_eq!(status.info.version.to_string(), "2.0.0");
    assert_eq!(status.state, PluginState::Disabled);
    assert_eq!(
        status.info.approved,
        [Permission::UiAdd, Permission::SessionsRead]
    );
    assert!(!root.path().join("own-cat/art").exists());
    let main = std::fs::read_to_string(root.path().join("own-cat/main.js")).unwrap();
    assert!(main.contains("two"));
}

#[test]
fn a_built_in_cannot_be_removed() {
    let root = tempfile::tempdir().unwrap();
    let (host, _rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(BUILTIN_V1));

    let refused = host.remove(&plugin_id("own-cat"));

    assert!(matches!(refused, Err(ManageError::Builtin(_))));
    assert!(root.path().join("own-cat").is_dir());
}

#[test]
fn a_plugin_with_a_built_in_id_cannot_be_installed() {
    let root = tempfile::tempdir().unwrap();
    let (host, _rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(BUILTIN_V1));
    write_plugin(root.path(), ".stage-one", "meow");
    std::fs::write(
        root.path().join(".stage-one/beep-plugin.json"),
        r#"{ "id": "own-cat", "name": "Mine", "version": "0.1.0" }"#,
    )
    .unwrap();

    let refused = host.install(&root.path().join(".stage-one"), Origin::Local);

    assert!(matches!(refused, Err(ManageError::Builtin(_))));
}

#[test]
fn the_dev_policy_writes_no_built_in_and_runs_the_dev_copy() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "own-cat", "dev");

    let (host, rx) = start_with_builtins(root.path(), Policy::Dev, &own_cat(BUILTIN_V1));

    assert!(!root.path().join("own-cat/art").exists());
    assert_eq!(
        status_of(&host, "own-cat").unwrap().info.source,
        Origin::Dev
    );
    wait_for(&rx, |placements| texts(placements) == ["dev"]);

    let empty = tempfile::tempdir().unwrap();
    let (_host, _rx) = start_with_builtins(empty.path(), Policy::Dev, &own_cat(BUILTIN_V1));
    assert!(!empty.path().join("own-cat").exists());
}

fn write_files(dir: &std::path::Path, id: &str, files: &[(&str, &str)]) {
    let folder = dir.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("beep-plugin.json"),
        format!(
            r#"{{ "id": "{id}", "name": "T", "version": "0.1.0", "permissions": ["ui.add"] }}"#
        ),
    )
    .unwrap();
    for (name, text) in files {
        std::fs::write(folder.join(name), text).unwrap();
    }
}

const MESSAGE: &str = r#"export const message = "{}";"#;
const IMPORTER: &str = r#"
import { message } from "./message.js";
beep.place({ slot: "composer" }, () => beep.text(message));
"#;

#[test]
fn a_change_to_an_imported_file_reloads_the_plugin() {
    let root = tempfile::tempdir().unwrap();
    write_files(
        root.path(),
        "cat-one",
        &[
            ("main.js", IMPORTER),
            ("message.js", &MESSAGE.replace("{}", "before")),
        ],
    );
    let (_host, rx) = start(root.path());
    wait_for(&rx, |p| texts(p) == ["before"]);

    std::fs::write(
        root.path().join("cat-one/message.js"),
        MESSAGE.replace("{}", "after"),
    )
    .unwrap();

    wait_for(&rx, |p| texts(p) == ["after"]);
}

#[test]
fn a_plugin_that_needs_a_newer_api_stops_with_a_reason() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "cat-one", "meow");
    std::fs::write(
        root.path().join("cat-one/beep-plugin.json"),
        r#"{ "id": "cat-one", "name": "T", "version": "0.1.0", "api": 3 }"#,
    )
    .unwrap();

    let (host, _rx) = start(root.path());

    let Some(PluginState::Stopped { reason }) = status_of(&host, "cat-one").map(|s| s.state) else {
        panic!("not stopped");
    };
    assert!(reason.starts_with("needs a newer Beep"), "{reason}");
}

#[test]
fn a_failing_plugin_logs_why_it_stopped() {
    let root = tempfile::tempdir().unwrap();
    write_files(
        root.path(),
        "logs-boom",
        &[("main.js", "console.log('hello'); throw new Error('boom');")],
    );

    crate::console::tests::captured("logs-boom");
    let (_host, _rx) = start(root.path());

    let wait = Instant::now() + TIMEOUT;
    loop {
        let lines = crate::console::tests::captured("logs-boom");
        if lines.iter().any(|(level, text)| {
            *level == log::Level::Error && text.starts_with("stopped:") && text.contains("boom")
        }) {
            assert!(lines.iter().any(|(_, text)| text == "hello"));
            return;
        }
        assert!(Instant::now() < wait, "no error line: {lines:?}");
        std::thread::yield_now();
    }
}

const STORING: &str = r#"
beep.store.set("seen", "yes");
beep.place({ slot: "composer" }, () => beep.text("stored"));
"#;

#[test]
fn removing_a_plugin_deletes_its_data_but_disabling_keeps_it() {
    let root = tempfile::tempdir().unwrap();
    write_files(root.path(), "cat-one", &[("main.js", STORING)]);
    let data = root.path().join(".data/cat-one.json");
    let (host, rx) = start_with(root.path(), Policy::Dev);
    wait_for(&rx, |p| texts(p) == ["stored"]);

    host.set_enabled(&plugin_id("cat-one"), false).unwrap();
    assert_eq!(std::fs::read_to_string(&data).unwrap(), r#"{"seen":"yes"}"#);

    host.remove(&plugin_id("cat-one")).unwrap();
    assert!(!data.exists());
}

#[test]
fn a_built_in_update_keeps_the_plugins_data() {
    const V1: &[(&str, &[u8])] = &[
        (
            "beep-plugin.json",
            br#"{ "id": "own-cat", "name": "T", "version": "1.0.0", "permissions": ["ui.add"] }"#,
        ),
        ("main.js", STORING.as_bytes()),
    ];
    const V2: &[(&str, &[u8])] = &[
        (
            "beep-plugin.json",
            br#"{ "id": "own-cat", "name": "T", "version": "2.0.0", "permissions": ["ui.add"] }"#,
        ),
        ("main.js", STORING.as_bytes()),
    ];
    let root = tempfile::tempdir().unwrap();
    let (host, rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(V1));
    host.set_enabled(&plugin_id("own-cat"), true).unwrap();
    wait_for(&rx, |p| texts(p) == ["stored"]);
    drop(host);
    let data = root.path().join(".data/own-cat.json");
    assert!(data.is_file());

    let (host, _rx) = start_with_builtins(root.path(), Policy::Managed, &own_cat(V2));

    assert_eq!(
        status_of(&host, "own-cat")
            .unwrap()
            .info
            .version
            .to_string(),
        "2.0.0"
    );
    assert_eq!(std::fs::read_to_string(&data).unwrap(), r#"{"seen":"yes"}"#);
}

const TOOLS: &str = r#"[
  { "name": "add", "description": "Adds two numbers.",
    "input": { "type": "object", "properties": { "a": { "type": "number" }, "b": { "type": "number" } },
               "required": ["a", "b"], "additionalProperties": false } },
  { "name": "later", "description": "Answers from a timer." },
  { "name": "hang", "description": "Never answers." },
  { "name": "boom", "description": "Throws." }
]"#;

const TOOL_MAIN: &str = r#"
beep.tool("add", (input) => ({ sum: input.a + input.b }));
beep.tool("later", () => new Promise((resolve) => beep.after(50, () => resolve("done"))));
beep.tool("hang", () => new Promise(() => {}));
beep.tool("boom", () => { throw new Error("nope"); });
"#;

fn write_tool_plugin(dir: &std::path::Path, id: &str, tools: &str, main: &str, activation: &str) {
    let folder = dir.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("beep-plugin.json"),
        format!(
            r#"{{ "id": "{id}", "name": "T", "version": "0.1.0", "permissions": ["tools"],
                  "activation": {activation}, "tools": {tools} }}"#
        ),
    )
    .unwrap();
    std::fs::write(folder.join("main.js"), main).unwrap();
}

fn tool_name(name: &str) -> ToolName {
    name.parse().unwrap()
}

/// Waits until the plugin is in `state`, which the supervisor reaches on its own thread.
fn wait_state(host: &Host, id: &str, state: PluginState) {
    let deadline = Instant::now() + TIMEOUT;
    while status_of(host, id).is_none_or(|status| status.state != state) {
        assert!(Instant::now() < deadline, "{id} never reached {state:?}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn call(
    host: &Host,
    id: &str,
    tool: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, ToolError> {
    host.call_tool(&plugin_id(id), &tool_name(tool), &input)
}

#[test]
fn a_plugin_answers_the_tools_it_declares_and_registers() {
    let root = tempfile::tempdir().unwrap();
    write_tool_plugin(root.path(), "tool-one", TOOLS, TOOL_MAIN, r#"["startup"]"#);
    let (host, _rx) = start_with(root.path(), Policy::Dev);
    wait_state(&host, "tool-one", PluginState::Running);

    let listed = status_of(&host, "tool-one").unwrap().info.tools;
    assert_eq!(listed.len(), 4);
    assert_eq!(listed[0].description, "Adds two numbers.");

    assert_eq!(
        call(&host, "tool-one", "add", json!({ "a": 2, "b": 3 })).unwrap(),
        json!({ "sum": 5 })
    );
    // A promise that a timer settles.
    assert_eq!(
        call(&host, "tool-one", "later", json!({})).unwrap(),
        json!("done")
    );

    // Arguments that do not fit the schema never reach the handler.
    let error = call(&host, "tool-one", "add", json!({ "a": "x", "b": 1 })).unwrap_err();
    assert!(matches!(error, ToolError::Input(_)), "{error}");
    let error = call(&host, "tool-one", "add", json!({ "a": 1 })).unwrap_err();
    assert!(matches!(error, ToolError::Input(_)), "{error}");

    // A tool the manifest does not declare, and a plugin that is not there.
    let error = call(&host, "tool-one", "sub", json!({})).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("provides: add, later, hang, boom"),
        "{error}"
    );
    let error = call(&host, "tool-two", "add", json!({})).unwrap_err();
    assert!(matches!(error, ToolError::NoPlugin(_)), "{error}");
}

#[test]
fn a_throwing_handler_or_a_promise_that_never_settles_fails_one_call_only() {
    let root = tempfile::tempdir().unwrap();
    write_tool_plugin(root.path(), "tool-one", TOOLS, TOOL_MAIN, r#"["startup"]"#);
    let (host, _rx) = start_with(root.path(), Policy::Dev);
    wait_state(&host, "tool-one", PluginState::Running);

    let error = call(&host, "tool-one", "boom", json!({})).unwrap_err();
    assert!(error.to_string().contains("nope"), "{error}");

    let started = Instant::now();
    let error = call(&host, "tool-one", "hang", json!({})).unwrap_err();
    assert!(matches!(error, ToolError::TimedOut), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));

    // The plugin still runs and answers.
    assert_eq!(
        status_of(&host, "tool-one").unwrap().state,
        PluginState::Running
    );
    assert_eq!(
        call(&host, "tool-one", "add", json!({ "a": 1, "b": 1 })).unwrap(),
        json!({ "sum": 2 })
    );
}

#[test]
fn a_tool_that_is_declared_but_not_registered_or_the_other_way_stops_the_plugin() {
    let root = tempfile::tempdir().unwrap();
    write_tool_plugin(
        root.path(),
        "unregistered",
        TOOLS,
        r#"beep.tool("add", () => 1);"#,
        r#"["startup"]"#,
    );
    write_tool_plugin(
        root.path(),
        "undeclared",
        r#"[{ "name": "add" }]"#,
        r#"beep.tool("add", () => 1); beep.tool("extra", () => 2);"#,
        r#"["startup"]"#,
    );
    let (host, _rx) = start_with(root.path(), Policy::Dev);

    for (id, reason) in [
        (
            "unregistered",
            "never registered with beep.tool: boom, hang, later",
        ),
        ("undeclared", "tool extra is not declared in the manifest"),
    ] {
        let deadline = Instant::now() + TIMEOUT;
        let state = loop {
            let state = status_of(&host, id).unwrap().state;
            if matches!(state, PluginState::Stopped { .. }) || Instant::now() > deadline {
                break state;
            }
            thread::sleep(Duration::from_millis(10));
        };
        let PluginState::Stopped { reason: given } = state else {
            panic!("{id} did not stop: {state:?}");
        };
        assert!(given.contains(reason), "{given}");

        let error = call(&host, id, "add", json!({})).unwrap_err();
        assert!(error.to_string().contains("stopped"), "{error}");
    }
}

#[test]
fn a_plugin_that_is_not_allowed_or_off_answers_with_its_state() {
    let root = tempfile::tempdir().unwrap();
    write_tool_plugin(root.path(), "tool-one", TOOLS, TOOL_MAIN, r#"["startup"]"#);
    let (host, _rx) = start_with(root.path(), Policy::Managed);

    let error = call(&host, "tool-one", "add", json!({ "a": 1, "b": 1 })).unwrap_err();
    assert!(error.to_string().contains("approval"), "{error}");

    host.approve(&plugin_id("tool-one")).unwrap();
    wait_state(&host, "tool-one", PluginState::Running);
    assert!(call(&host, "tool-one", "add", json!({ "a": 1, "b": 1 })).is_ok());

    host.set_enabled(&plugin_id("tool-one"), false).unwrap();
    let error = call(&host, "tool-one", "add", json!({ "a": 1, "b": 1 })).unwrap_err();
    assert!(error.to_string().contains("turned off"), "{error}");
}

#[test]
fn a_plugin_that_waits_for_its_slot_starts_for_the_call_but_not_while_paused() {
    let root = tempfile::tempdir().unwrap();
    write_tool_plugin(
        root.path(),
        "tool-one",
        TOOLS,
        TOOL_MAIN,
        r#"["slot:session.row"]"#,
    );
    let (host, _rx) = start_with(root.path(), Policy::Dev);
    wait_state(&host, "tool-one", PluginState::Waiting);

    assert!(call(&host, "tool-one", "add", json!({ "a": 1, "b": 2 })).is_ok());
    assert_eq!(
        status_of(&host, "tool-one").unwrap().state,
        PluginState::Running
    );

    host.set_visible("test", false);
    wait_state(&host, "tool-one", PluginState::Paused);
    let error = call(&host, "tool-one", "add", json!({ "a": 1, "b": 2 })).unwrap_err();
    assert!(error.to_string().contains("paused"), "{error}");
}

fn write_theme(dir: &std::path::Path, id: &str, radius: &str) {
    std::fs::create_dir_all(dir.join(id)).unwrap();
    std::fs::write(
        dir.join(id).join("beep-plugin.json"),
        json!({ "id": id, "name": id, "version": "0.1.0", "permissions": ["style"],
                "activation": [], "style": { "--radius": radius } })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn a_skin_needs_no_code_and_waits_for_approval() {
    let root = tempfile::tempdir().unwrap();
    write_theme(root.path(), "sharp", "0");
    let (host, _rx) = start_with(root.path(), Policy::Managed);

    let status = status_of(&host, "sharp").unwrap();
    assert_eq!(
        status.state,
        PluginState::NeedsApproval {
            missing: vec![Permission::Style]
        }
    );
    assert_eq!(status.info.style[&StyleToken::Radius].as_ref(), "0");

    host.approve(&plugin_id("sharp")).unwrap();
    let status = status_of(&host, "sharp").unwrap();
    assert_eq!(status.state, PluginState::Waiting);
    assert_eq!(status.info.approved, [Permission::Style]);
}
