use std::sync::mpsc;

use serde_json::json;

use super::*;
use crate::event::Action;
use crate::instance::{
    ComposerContext, Instance, InstanceKey, InstanceRef, SessionHeaderContext, SessionRowContext,
    SlotContext,
};
use crate::status::{PluginState, PluginStatus};
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
    std::fs::write(folder.join("main.ts"), main).unwrap();
}

/// A supervisor driven by hand, with no loop and no file watcher. Plugin threads are real.
fn driven(dir: &std::path::Path) -> (Supervisor, Arc<Published>, mpsc::Receiver<Event>) {
    let (events, inbox) = mpsc::channel();
    let published = Arc::new(Published::default());
    let supervisor = Supervisor::new(
        dir.canonicalize().unwrap(),
        events,
        Arc::new(Quiet),
        Arc::clone(&published),
    );

    (supervisor, published, inbox)
}

fn state_of(published: &Published, id: &str) -> PluginState {
    let id: PluginId = id.parse().unwrap();
    let list = locked(&published.registry).list(Instant::now());

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
    let list = locked(&published.registry).list(Instant::now());
    let find = |id: &str| {
        list.iter()
            .find(|status| status.id.to_string() == id)
            .unwrap()
    };

    assert_eq!(find("lazy-one").state, PluginState::Waiting);
    assert_eq!(find("lazy-one").name, "Lazy");
    assert_eq!(find("lazy-one").version, semver::Version::new(0, 3, 0));
    assert_eq!(
        (find("lazy-one").used_ms, find("lazy-one").pending_timers),
        (0, 0)
    );
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
        name: "Lazy".to_owned(),
        version: semver::Version::new(1, 0, 0),
        state: PluginState::Stopped {
            reason: "used 600 ms".to_owned(),
        },
        used_ms: 600,
        pending_timers: 2,
    };

    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        json!({ "id": "lazy-one", "name": "Lazy", "version": "1.0.0", "state": "stopped",
                "reason": "used 600 ms", "usedMs": 600, "pendingTimers": 2 })
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
        folder.join("main.ts"),
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
