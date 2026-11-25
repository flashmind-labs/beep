use super::*;
use log::Level;
use serde_json::json;

use crate::event::Action;
use crate::ident::Slot;
use crate::instance::{
    ComposerContext, InstanceKey, SessionHeaderContext, SessionRowContext, SlotContext,
};
use crate::tool::{ToolCall, ToolError};
use crate::topic::Publication;
use crate::ui::{Align, CallbackId, Position, Tone, Transition};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot a real image";

/// A plugin folder named `test-plugin` inside a temp dir.
struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new(permissions: &str, main: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("test-plugin");
        std::fs::create_dir_all(dir.join("art")).unwrap();
        std::fs::write(
            dir.join("beep-plugin.json"),
            format!(
                r#"{{ "id": "test-plugin", "name": "T", "version": "0.1.0", "permissions": [{permissions}] }}"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), main).unwrap();
        std::fs::write(dir.join("art/cat.png"), PNG).unwrap();
        std::fs::write(root.path().join("outside.png"), PNG).unwrap();

        Self { root }
    }

    fn dir(&self) -> PathBuf {
        self.root.path().join("test-plugin")
    }

    /// Loads the plugin with the composer open, as when a session is shown.
    fn load(&self, now: Instant) -> Result<Sandbox, SandboxError> {
        Ok(with_composer(Sandbox::load(&self.dir(), now)?))
    }
}

fn instance(key: &str, context: SlotContext) -> Instance {
    Instance {
        key: InstanceKey::try_from(key.to_owned()).unwrap(),
        context,
    }
}

fn composer_instance() -> Instance {
    instance(
        "composer",
        SlotContext::Composer(ComposerContext {
            machine: "build-box".to_owned(),
            session: "s1".to_owned(),
        }),
    )
}

/// Opens the composer, the one instance of its slot.
fn with_composer(mut sandbox: Sandbox) -> Sandbox {
    sandbox.set_instances(&[composer_instance()]).unwrap();

    sandbox
}

fn only_node(placements: Option<Vec<Placement>>) -> Node {
    let mut placements = placements.expect("a render");
    assert_eq!(placements.len(), 1);

    placements.remove(0).node
}

#[test]
fn placed_image_becomes_a_data_url() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        const src = "art/cat.png";
        beep.place({ slot: "composer", offset: 84, sink: 4 }, () =>
          beep.image({ src, width: 56, height: 48, pixelated: true }));
        "#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    let placements = sandbox.render().unwrap().unwrap();

    assert_eq!(placements.len(), 1);
    let placement = &placements[0];
    assert_eq!(placement.plugin.to_string(), "test-plugin");
    assert_eq!(placement.slot, Slot::Composer);
    assert_eq!(
        placement.position,
        Position::Edge {
            align: Align::End,
            offset: 84,
            sink: 4
        }
    );
    assert_eq!(placement.instance.to_string(), "composer");
    let Node::Image {
        asset,
        width,
        height,
        pixelated,
    } = &placement.node
    else {
        panic!("expected an image");
    };
    assert_eq!(
        asset.hash.to_string(),
        blake3::hash(PNG).to_hex().to_string()
    );
    assert_eq!(asset.mime.to_string(), "image/png");
    assert_eq!(sandbox.assets().len(), 1);
    assert_eq!((*width, *height, *pixelated), (56, 48, true));
    assert!(
        sandbox.render().unwrap().is_none(),
        "clean state renders nothing"
    );
}

#[test]
fn timer_and_state_rerender() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        const word = beep.state("idle");
        beep.place({ slot: "composer" }, () => beep.row({ gap: 2 }, [beep.text(word.get())]));
        beep.after(100, () => word.set("blink"));
        "#,
    );
    let start = Instant::now();
    let mut sandbox = fixture.load(start).unwrap();
    let text = |word: &str| Node::Row {
        gap: 2,
        children: vec![Node::Text {
            text: word.to_owned(),
            tone: Tone::Normal,
        }],
    };

    assert_eq!(only_node(sandbox.render().unwrap()), text("idle"));
    let due = start + Duration::from_millis(100);
    assert_eq!(sandbox.next_timer(), Some(due));

    sandbox.fire_due(due - Duration::from_millis(1)).unwrap();
    assert!(sandbox.render().unwrap().is_none());

    sandbox.fire_due(due).unwrap();
    assert_eq!(only_node(sandbox.render().unwrap()), text("blink"));
    assert_eq!(sandbox.next_timer(), None);
}

#[test]
fn cancel_stops_a_timer() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        const word = beep.state("idle");
        beep.place({ slot: "composer" }, () => beep.text(word.get()));
        const cancel = beep.after(100, () => word.set("blink"));
        cancel();
        "#,
    );
    let start = Instant::now();
    let mut sandbox = fixture.load(start).unwrap();

    assert_eq!(sandbox.next_timer(), None);
    sandbox.fire_due(start + Duration::from_secs(1)).unwrap();
    sandbox.render().unwrap();
    assert!(sandbox.render().unwrap().is_none());
}

#[test]
fn timer_delay_has_a_floor_and_chains_run_later() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        let n = 0;
        const count = beep.state(0);
        beep.place({ slot: "composer" }, () => beep.text(String(count.get())));
        function tick() { n += 1; count.set(n); beep.after(0, tick); }
        beep.after(0, tick);
        "#,
    );
    let start = Instant::now();
    let mut sandbox = fixture.load(start).unwrap();

    assert_eq!(
        sandbox.next_timer(),
        Some(start + Duration::from_millis(16))
    );
    sandbox.fire_due(start + Duration::from_millis(16)).unwrap();

    assert_eq!(
        only_node(sandbox.render().unwrap()),
        Node::Text {
            text: "1".to_owned(),
            tone: Tone::Normal
        }
    );
}

#[test]
fn parent_paths_are_refused() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () => beep.image({ src: "../outside.png", width: 1, height: 1 }));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    let error = sandbox.render().unwrap_err();

    assert!(
        matches!(
            error,
            SandboxError::Image {
                reason: ImageError::Outside,
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn absolute_and_non_image_paths_are_refused() {
    let absolute = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () => beep.image({ src: "/etc/hosts", width: 1, height: 1 }));"#,
    );
    let error = absolute.load(Instant::now()).unwrap().render().unwrap_err();
    assert!(matches!(
        error,
        SandboxError::Image {
            reason: ImageError::Absolute,
            ..
        }
    ));

    let text = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () => beep.image({ src: "main.js", width: 1, height: 1 }));"#,
    );
    let error = text.load(Instant::now()).unwrap().render().unwrap_err();
    assert!(matches!(
        error,
        SandboxError::Image {
            reason: ImageError::NotImage,
            ..
        }
    ));
}

#[test]
fn endless_loop_is_interrupted() {
    let fixture = Fixture::new(r#""ui.add""#, "while (true) {}");

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(matches!(error, SandboxError::Script(_)), "{error}");
}

#[test]
fn endless_loop_in_a_timer_is_interrupted() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.after(20, () => { try { while (true) {} } catch (e) {} });"#,
    );
    let start = Instant::now();
    let mut sandbox = fixture.load(start).unwrap();

    let result = sandbox.fire_due(start + Duration::from_millis(20));

    assert!(result.is_err());
}

#[test]
fn placing_without_permission_fails() {
    let fixture = Fixture::new(
        "",
        r#"beep.place({ slot: "composer" }, () => beep.text("hi"));"#,
    );

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(error.to_string().contains("ui.add"), "{error}");
}

#[test]
fn invalid_trees_fail() {
    let unknown = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () => ({ kind: "text", text: "x", extra: 1 }));"#,
    );
    assert!(matches!(
        unknown.load(Instant::now()).unwrap().render(),
        Err(SandboxError::Tree(_))
    ));

    let slot = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "nowhere" }, () => beep.text("x"));"#,
    );
    assert!(slot.load(Instant::now()).is_err());

    let many = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () =>
             beep.row({}, Array.from({ length: 80 }, () => beep.text("x"))));"#,
    );
    assert!(matches!(
        many.load(Instant::now()).unwrap().render(),
        Err(SandboxError::TooManyNodes(_))
    ));

    let big = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () => beep.text("x".repeat(70000)));"#,
    );
    assert!(matches!(
        big.load(Instant::now()).unwrap().render(),
        Err(SandboxError::TooLarge(_))
    ));
}

#[test]
fn timer_count_is_limited() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"for (let i = 0; i < 65; i++) beep.after(1000, () => {});"#,
    );

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(
        error.to_string().contains("too many pending timers"),
        "{error}"
    );
}

#[test]
fn plugin_cannot_reach_the_host() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        const names = Object.getOwnPropertyNames(globalThis);
        if (names.includes("schedule") || names.includes("__beepSchedule")) throw new Error("leak");
        try { beep.after = () => {}; } catch (e) {}
        if (!Object.isFrozen(beep)) throw new Error("not frozen");
        "#,
    );

    fixture.load(Instant::now()).unwrap();
}

fn topic(name: &str, key: Option<&str>, machine: Option<&str>) -> Topic {
    Topic {
        name: name.to_owned(),
        key: key.map(str::to_owned),
        machine: machine.map(str::to_owned),
    }
}

fn update(topic: Topic, machine: Option<&str>, data: serde_json::Value) -> Delivery {
    Delivery::new(&Publication {
        topic,
        machine: machine.map(str::to_owned),
        data,
    })
    .unwrap()
}

fn text_of(placements: Option<Vec<Placement>>) -> String {
    match only_node(placements) {
        Node::Text { text, .. } => text,
        other => panic!("expected text, got {other:?}"),
    }
}

const FOLLOW_COMPOSER: &str = r#"
    const seen = beep.state("none");
    beep.on({ name: "ui.composer" }, (event) =>
      seen.set(event.data === null ? "gone" : String(event.data.session)));
    beep.place({ slot: "composer" }, () => beep.text(seen.get()));
"#;

#[test]
fn a_delivered_update_runs_the_callback_and_rerenders() {
    let fixture = Fixture::new(r#""ui.add""#, FOLLOW_COMPOSER);
    let mut sandbox = fixture.load(Instant::now()).unwrap();
    let composer = topic("ui.composer", None, None);

    assert_eq!(text_of(sandbox.render().unwrap()), "none");
    assert_eq!(sandbox.take_wanted(), Some(vec![composer.clone()]));

    let shown = json!({ "machine": "m1", "session": "s1" });
    sandbox
        .deliver(&update(composer.clone(), None, shown))
        .unwrap();
    assert_eq!(text_of(sandbox.render().unwrap()), "s1");

    sandbox
        .deliver(&update(composer, None, json!(null)))
        .unwrap();
    assert_eq!(text_of(sandbox.render().unwrap()), "gone");
}

#[test]
fn only_the_followed_topic_reaches_the_callback() {
    let fixture = Fixture::new(
        r#""ui.add", "sessions.read""#,
        r#"
        const seen = beep.state("none");
        beep.on({ name: "agent", key: "s1", machine: "m1" }, (event) =>
          seen.set(`${event.machine}:${event.topic.key}:${event.data}`));
        beep.place({ slot: "composer" }, () => beep.text(seen.get()));
        "#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();
    sandbox.render().unwrap();

    for other in [
        topic("agent", Some("s2"), Some("m1")),
        topic("agent", Some("s1"), Some("m2")),
        topic("agent", Some("s1"), None),
        topic("agents", None, None),
    ] {
        sandbox
            .deliver(&update(other, Some("m1"), json!(0)))
            .unwrap();
    }
    assert!(sandbox.render().unwrap().is_none());

    let wanted = topic("agent", Some("s1"), Some("m1"));
    sandbox
        .deliver(&update(wanted, Some("m1"), json!(7)))
        .unwrap();
    assert_eq!(text_of(sandbox.render().unwrap()), "m1:s1:7");
}

#[test]
fn cancel_stops_delivery() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        const seen = beep.state("none");
        const stop = beep.on({ name: "ui.composer" }, () => seen.set("heard"));
        beep.place({ slot: "composer" }, () => beep.text(seen.get()));
        beep.after(100, () => { stop(); stop(); });
        "#,
    );
    let start = Instant::now();
    let mut sandbox = fixture.load(start).unwrap();
    sandbox.render().unwrap();
    sandbox.take_wanted();

    sandbox
        .fire_due(start + Duration::from_millis(100))
        .unwrap();
    assert_eq!(sandbox.take_wanted(), Some(Vec::new()));

    sandbox
        .deliver(&update(topic("ui.composer", None, None), None, json!(1)))
        .unwrap();
    assert!(sandbox.render().unwrap().is_none());
}

#[test]
fn a_callback_can_follow_and_drop_other_topics() {
    let fixture = Fixture::new(
        r#""sessions.read""#,
        r#"
        let stop = null;
        beep.on({ name: "ui.composer" }, (event) => {
          if (stop !== null) stop();
          stop = event.data === null ? null : beep.on({ name: "agent", key: event.data.session }, () => {});
        });
        "#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();
    sandbox.take_wanted();
    let composer = topic("ui.composer", None, None);

    sandbox
        .deliver(&update(composer.clone(), None, json!({ "session": "s1" })))
        .unwrap();
    let wanted = sandbox.take_wanted().unwrap();
    assert!(wanted.contains(&topic("agent", Some("s1"), None)));

    sandbox
        .deliver(&update(composer, None, json!({ "session": "s2" })))
        .unwrap();
    let wanted = sandbox.take_wanted().unwrap();
    assert!(!wanted.contains(&topic("agent", Some("s1"), None)));
    assert!(wanted.contains(&topic("agent", Some("s2"), None)));
}

#[test]
fn following_sessions_needs_the_permission() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.on({ name: "agent", key: "s1" }, () => {});"#,
    );

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(error.to_string().contains("sessions.read"), "{error}");
}

#[test]
fn unknown_topic_names_are_refused() {
    let fixture = Fixture::new(
        r#""ui.add", "sessions.read""#,
        r#"beep.on({ name: "machines" }, () => {});"#,
    );

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(error.to_string().contains("unknown topic"), "{error}");
}

#[test]
fn bad_arguments_to_on_are_refused() {
    for call in [
        r#"beep.on({ name: "ui.a" })"#,
        r#"beep.on("ui.a", () => {})"#,
        r#"beep.on({ name: "ui.a", key: 3 }, () => {})"#,
        r#"beep.on({ name: "ui.a", machine: {} }, () => {})"#,
    ] {
        let fixture = Fixture::new("", call);

        assert!(fixture.load(Instant::now()).is_err(), "{call}");
    }
}

#[test]
fn subscriptions_are_limited_to_sixteen() {
    let fixture = Fixture::new(
        "",
        r#"for (let i = 0; i < 17; i++) beep.on({ name: "ui.composer" }, () => {});"#,
    );

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(error.to_string().contains("at most 16"), "{error}");
}

#[test]
fn a_callback_that_loops_is_interrupted() {
    let fixture = Fixture::new("", r#"beep.on({ name: "ui.a" }, () => { for (;;) {} });"#);
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    let result = sandbox.deliver(&update(topic("ui.a", None, None), None, json!(1)));

    assert!(matches!(result, Err(SandboxError::Script(_))));
}

#[test]
fn a_callback_that_throws_fails_the_delivery() {
    let fixture = Fixture::new(
        "",
        r#"beep.on({ name: "ui.a" }, () => { throw new Error("boom"); });"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    let result = sandbox.deliver(&update(topic("ui.a", None, None), None, json!(1)));

    assert!(result.unwrap_err().to_string().contains("boom"));
}

fn cat_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/examples/composer-cat")
}

fn file_hash(dir: &Path, name: &str) -> String {
    blake3::hash(&std::fs::read(dir.join(name)).unwrap())
        .to_hex()
        .to_string()
}

#[test]
fn composer_cat_declares_its_motions_and_holds_no_timers() {
    let dir = cat_dir();
    let start = Instant::now();
    let mut sandbox = with_composer(Sandbox::load(&dir, start).unwrap());

    let first = sandbox.render().unwrap().expect("a first render");

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].slot, Slot::Composer);
    assert_eq!(first[0].enter, Transition::Rise);
    assert_eq!(first[0].leave, Transition::None);
    let Node::Sprite {
        frames,
        rest,
        motions,
        reactions,
        play,
        ..
    } = &first[0].node
    else {
        panic!("expected a sprite, got {:?}", first[0].node);
    };
    // Idle: five frames for the motions, five more for the reactions.
    assert_eq!(frames.len(), 10);
    assert_eq!(
        frames[usize::from(*rest)].hash.to_string(),
        file_hash(&dir, "art/cat-idle.png")
    );
    assert_eq!(motions.len(), 3);
    assert!(
        motions
            .iter()
            .all(|motion| matches!(motion.repeat, Repeat::Every { .. }))
    );
    assert_eq!(
        reactions.keys().map(String::as_str).collect::<Vec<_>>(),
        ["happy", "sad", "startled", "yawn"]
    );
    assert_eq!(*play, None);
    assert_eq!(sandbox.assets().len(), 10);

    // Nothing is pending, so a quiet minute needs no wakeup and no redraw.
    assert_eq!(sandbox.next_timer(), None);
    sandbox.fire_due(start + Duration::from_secs(600)).unwrap();
    assert!(sandbox.render().unwrap().is_none());
}

fn session_turn(state: &str, outcome: Option<&str>, turn: u32) -> serde_json::Value {
    json!({ "session": "s1", "state": state, "outcome": outcome, "turn": turn })
}

/// The cat as the page gets it: the rest frame's file, the motions' repeats and the requested reaction.
struct Cat {
    rest: String,
    motions: Vec<Repeat>,
    play: Option<(String, u32)>,
}

fn cat(sandbox: &mut Sandbox) -> Option<Cat> {
    let placements = sandbox.render().unwrap()?;
    let Node::Sprite {
        frames,
        rest,
        motions,
        play,
        ..
    } = &placements[0].node
    else {
        panic!("expected a sprite, got {:?}", placements[0].node);
    };

    Some(Cat {
        rest: frames[usize::from(*rest)].hash.to_string(),
        motions: motions.iter().map(|motion| motion.repeat).collect(),
        play: play.as_ref().map(|play| (play.name.clone(), play.id)),
    })
}

#[test]
fn composer_cat_types_asks_and_reacts_to_each_way_a_turn_ends() {
    let dir = cat_dir();
    let mut sandbox = with_composer(Sandbox::load(&dir, Instant::now()).unwrap());
    let composer = topic("ui.composer", None, None);
    let session = topic("session.turn", Some("s1"), Some("build-box"));
    let idle = file_hash(&dir, "art/cat-idle.png");
    let ask = file_hash(&dir, "art/cat-ask.png");

    assert_eq!(sandbox.take_wanted(), Some(vec![composer.clone()]));
    assert!(cat(&mut sandbox).is_some());

    let open = json!({ "machine": "build-box", "session": "s1" });
    sandbox.deliver(&update(composer, None, open)).unwrap();
    let mut wanted = vec![topic("ui.composer", None, None), session.clone()];
    wanted.sort();
    assert_eq!(sandbox.take_wanted(), Some(wanted));

    let mut send = |state: &str, outcome: Option<&str>, turn: u32| {
        let data = session_turn(state, outcome, turn);
        sandbox
            .deliver(&update(session.clone(), Some("build-box"), data))
            .unwrap();

        cat(&mut sandbox).expect("a redraw")
    };

    // The first update is a baseline: an end that came before is not reacted to.
    let shown = send("idle", Some("finished"), 4);
    assert_eq!(
        (shown.rest.as_str(), shown.motions.len(), shown.play),
        (idle.as_str(), 3, None)
    );

    // While the turn runs, the typing loop is the only motion.
    let shown = send("working", Some("finished"), 4);
    assert_eq!(shown.motions, [Repeat::Loop]);
    assert_eq!(shown.play, None);

    // While it asks, the ears stay up on the rest frame, with no motion.
    let shown = send("asking", Some("finished"), 4);
    assert_eq!(
        (shown.rest.as_str(), shown.motions.len()),
        (ask.as_str(), 0)
    );

    // Each end plays its reaction once, with a new id, and the idle motions return.
    let ends = [
        ("finished", "happy"),
        ("cancelled", "startled"),
        ("failed", "sad"),
        ("refused", "sad"),
        ("token-limit", "yawn"),
        ("request-limit", "yawn"),
        ("stopped", "yawn"),
    ];
    for (index, (outcome, reaction)) in ends.into_iter().enumerate() {
        let turn = 5 + u32::try_from(index).unwrap();
        let id = 1 + u32::try_from(index).unwrap();

        let shown = send("idle", Some(outcome), turn);
        assert_eq!(shown.play, Some((reaction.to_owned(), id)), "{outcome}");
        assert_eq!(
            (shown.rest.as_str(), shown.motions.len()),
            (idle.as_str(), 3)
        );

        // The same end again, and the next turn starting, ask for nothing new.
        let again = send("idle", Some(outcome), turn);
        assert_eq!(
            again.play,
            Some((reaction.to_owned(), id)),
            "{outcome} again"
        );
        let next = send("working", Some(outcome), turn);
        assert_eq!(
            next.play,
            Some((reaction.to_owned(), id)),
            "{outcome} working"
        );
    }
    assert_eq!(sandbox.next_timer(), None);
}

#[test]
fn composer_cat_ignores_what_it_does_not_expect() {
    let mut sandbox = with_composer(Sandbox::load(&cat_dir(), Instant::now()).unwrap());
    let composer = topic("ui.composer", None, None);
    let session = topic("session.turn", Some("s1"), Some("m1"));
    sandbox.render().unwrap();
    sandbox.take_wanted();

    for data in [json!(null), json!(7), json!({ "machine": 1 }), json!([])] {
        sandbox
            .deliver(&update(composer.clone(), None, data))
            .unwrap();
        assert_eq!(sandbox.take_wanted(), None);
    }

    let open = json!({ "machine": "m1", "session": "s1" });
    sandbox.deliver(&update(composer, None, open)).unwrap();
    sandbox.render().unwrap();

    for data in [
        json!(null),
        json!({ "session": "s1" }),
        json!({ "session": "s1", "state": "napping", "outcome": null, "turn": 1 }),
        json!({ "session": "s1", "state": "idle", "outcome": "won", "turn": 1 }),
        json!({ "session": "s1", "state": "idle", "outcome": null, "turn": "1" }),
        json!({ "error": "boom" }),
    ] {
        sandbox
            .deliver(&update(session.clone(), Some("m1"), data))
            .unwrap();
    }

    assert!(sandbox.render().unwrap().is_none());
}

fn row_instance(session: &str, title: &str) -> Instance {
    instance(
        session,
        SlotContext::SessionRow(SessionRowContext {
            machine: "build-box".to_owned(),
            session: session.to_owned(),
            title: title.to_owned(),
        }),
    )
}

fn header_instance(session: &str) -> Instance {
    instance(
        session,
        SlotContext::SessionHeader(SessionHeaderContext {
            machine: "build-box".to_owned(),
            session: session.to_owned(),
        }),
    )
}

/// The text of each placement, with its instance key, in order.
fn texts_by_instance(placements: &[Placement]) -> Vec<(String, String)> {
    placements
        .iter()
        .map(|placement| {
            let Node::Text { text, .. } = &placement.node else {
                panic!("expected text, got {:?}", placement.node);
            };
            (placement.instance.to_string(), text.clone())
        })
        .collect()
}

fn press(plugin: &Sandbox, id: u64, action: Action) -> UiEvent {
    UiEvent {
        plugin: plugin.id.clone(),
        id: CallbackId::from(id),
        action,
    }
}

const ROWS: &str = r#"
    let renders = 0;
    const mood = beep.state("calm");
    beep.place({ slot: "session.row" }, (row) => {
      renders += 1;
      return beep.text(`${row.title}/${mood.get()}/${renders}`);
    });
"#;

#[test]
fn each_instance_is_rendered_with_its_own_context() {
    let fixture = Fixture::new(r#""ui.add""#, ROWS);
    let mut sandbox = Sandbox::load(&fixture.dir(), Instant::now()).unwrap();

    sandbox
        .set_instances(&[row_instance("a", "Alpha"), row_instance("b", "Beta")])
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();

    assert_eq!(
        texts_by_instance(&placements),
        [
            ("a".to_owned(), "Alpha/calm/1".to_owned()),
            ("b".to_owned(), "Beta/calm/2".to_owned())
        ]
    );
    assert!(
        placements
            .iter()
            .all(|p| p.slot == Slot::SessionRow && p.position == Position::Inline)
    );
}

#[test]
fn only_instances_that_changed_are_rendered_again() {
    let fixture = Fixture::new(r#""ui.add""#, ROWS);
    let mut sandbox = Sandbox::load(&fixture.dir(), Instant::now()).unwrap();
    sandbox
        .set_instances(&[row_instance("a", "Alpha"), row_instance("b", "Beta")])
        .unwrap();
    sandbox.render().unwrap();

    // A new row renders alone. The others keep what they drew.
    sandbox
        .set_instances(&[
            row_instance("a", "Alpha"),
            row_instance("b", "Beta"),
            row_instance("c", "Gamma"),
        ])
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    assert_eq!(
        texts_by_instance(&placements),
        [
            ("a".to_owned(), "Alpha/calm/1".to_owned()),
            ("b".to_owned(), "Beta/calm/2".to_owned()),
            ("c".to_owned(), "Gamma/calm/3".to_owned())
        ]
    );

    // The same list again renders nothing.
    sandbox
        .set_instances(&[
            row_instance("a", "Alpha"),
            row_instance("b", "Beta"),
            row_instance("c", "Gamma"),
        ])
        .unwrap();
    assert!(sandbox.render().unwrap().is_none());

    // A changed context renders that instance alone.
    sandbox
        .set_instances(&[
            row_instance("a", "Alpha"),
            row_instance("b", "Renamed"),
            row_instance("c", "Gamma"),
        ])
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    assert_eq!(
        texts_by_instance(&placements)[1],
        ("b".to_owned(), "Renamed/calm/4".to_owned())
    );
    assert_eq!(texts_by_instance(&placements)[0].1, "Alpha/calm/1");
}

#[test]
fn a_state_change_renders_every_instance_again() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        &format!(r#"{ROWS} beep.after(50, () => mood.set("busy"));"#),
    );
    let start = Instant::now();
    let mut sandbox = Sandbox::load(&fixture.dir(), start).unwrap();
    sandbox
        .set_instances(&[row_instance("a", "Alpha"), row_instance("b", "Beta")])
        .unwrap();
    sandbox.render().unwrap();

    sandbox.fire_due(start + Duration::from_millis(50)).unwrap();
    let placements = sandbox.render().unwrap().unwrap();

    assert_eq!(
        texts_by_instance(&placements),
        [
            ("a".to_owned(), "Alpha/busy/3".to_owned()),
            ("b".to_owned(), "Beta/busy/4".to_owned())
        ]
    );
}

#[test]
fn an_instance_that_goes_away_drops_its_output() {
    let fixture = Fixture::new(r#""ui.add""#, ROWS);
    let mut sandbox = Sandbox::load(&fixture.dir(), Instant::now()).unwrap();
    sandbox
        .set_instances(&[row_instance("a", "Alpha"), row_instance("b", "Beta")])
        .unwrap();
    sandbox.render().unwrap();

    sandbox.set_instances(&[row_instance("b", "Beta")]).unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    assert_eq!(texts_by_instance(&placements).len(), 1);
    assert_eq!(texts_by_instance(&placements)[0].0, "b");

    sandbox.set_instances(&[]).unwrap();
    assert_eq!(sandbox.render().unwrap().unwrap(), []);

    // When it comes back it is drawn fresh.
    sandbox
        .set_instances(&[row_instance("a", "Alpha")])
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    assert_eq!(texts_by_instance(&placements)[0].1, "Alpha/calm/3");
}

#[test]
fn placements_only_get_instances_of_their_slot() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        beep.place({ slot: "session.row" }, (c) => beep.text("row " + c.session));
        beep.place({ slot: "session.header" }, (c) => beep.text("head " + c.session));
        beep.place({ slot: "composer" }, (c) => beep.text("comp " + JSON.stringify(c)));
        "#,
    );
    let mut sandbox = Sandbox::load(&fixture.dir(), Instant::now()).unwrap();

    sandbox
        .set_instances(&[
            composer_instance(),
            row_instance("a", "Alpha"),
            header_instance("a"),
        ])
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();

    let mut seen: Vec<(Slot, String)> = placements
        .iter()
        .map(|p| {
            (
                p.slot,
                texts_by_instance(std::slice::from_ref(p))[0].1.clone(),
            )
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            (
                Slot::Composer,
                r#"comp {"machine":"build-box","session":"s1"}"#.to_owned()
            ),
            (Slot::SessionRow, "row a".to_owned()),
            (Slot::SessionHeader, "head a".to_owned())
        ]
    );
}

#[test]
fn a_render_that_returns_null_draws_nothing() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "session.row" }, (c) => c.title === "" ? null : beep.text(c.title));"#,
    );
    let mut sandbox = Sandbox::load(&fixture.dir(), Instant::now()).unwrap();

    sandbox
        .set_instances(&[row_instance("a", ""), row_instance("b", "Beta")])
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();

    assert_eq!(
        texts_by_instance(&placements),
        [("b".to_owned(), "Beta".to_owned())]
    );
}

#[test]
fn place_options_must_fit_the_slot() {
    for options in [
        r#"{ slot: "session.row", align: "end" }"#,
        r#"{ slot: "session.header", offset: 3 }"#,
        r#"{ slot: "composer", gap: 1 }"#,
        r#"{ slot: "composer", align: "middle" }"#,
        r#"{ slot: "nowhere" }"#,
    ] {
        let fixture = Fixture::new(
            r#""ui.add""#,
            &format!(r#"beep.place({options}, () => beep.text("x"));"#),
        );

        let error = fixture.load(Instant::now()).err().expect("load fails");

        assert!(
            error.to_string().contains("beep.place"),
            "{options}: {error}"
        );
    }
}

#[test]
fn at_most_eight_places_and_two_thousand_entries() {
    let nine = Fixture::new(
        r#""ui.add""#,
        r#"for (let i = 0; i < 9; i++) beep.place({ slot: "composer" }, () => beep.text("x"));"#,
    );
    let error = nine.load(Instant::now()).err().expect("load fails");
    assert!(error.to_string().contains("at most 8"), "{error}");

    let many = Fixture::new(
        r#""ui.add""#,
        r#"for (let i = 0; i < 8; i++) beep.place({ slot: "session.row" }, () => beep.text("x"));"#,
    );
    let mut sandbox = Sandbox::load(&many.dir(), Instant::now()).unwrap();
    let rows: Vec<Instance> = (0..200)
        .map(|i| row_instance(&format!("s{i}"), "t"))
        .collect();
    sandbox.set_instances(&rows).unwrap();
    // 8 places x 200 rows is 1600 entries, inside the limit.
    assert_eq!(sandbox.render().unwrap().unwrap().len(), 1600);
}

const CONTROLS: &str = r#"
    const clicks = beep.state(0);
    const on = beep.state(false);
    const typed = beep.state("");
    const sent = beep.state("");
    beep.place({ slot: "session.header" }, (c) =>
      beep.row({ gap: 4 }, [
        beep.button({ label: "Go", onPress: () => clicks.set(clicks.get() + 1) }, [beep.text("Go " + c.session + " " + clicks.get())]),
        beep.toggle({ label: "On", value: on.get(), onChange: (v) => on.set(v) }),
        beep.input({ label: "Note", value: typed.get(), placeholder: "p",
          onChange: (v) => typed.set(v), onSubmit: (v) => sent.set(v) }),
        beep.text(`${sent.get()}`, { tone: "dim" }),
      ]));
"#;

/// The callback ids of a header render: button, toggle, input change, input submit.
fn control_ids(placements: &[Placement]) -> (u64, u64, u64, u64) {
    let Node::Row { children, .. } = &placements[0].node else {
        panic!("expected a row");
    };
    let id = |callback: Option<CallbackId>| {
        serde_json::to_value(callback.unwrap())
            .unwrap()
            .as_u64()
            .unwrap()
    };
    let [
        Node::Button { on_press, .. },
        Node::Toggle { on_change, .. },
        Node::Input {
            on_change: change,
            on_submit,
            ..
        },
        _,
    ] = children.as_slice()
    else {
        panic!("expected button, toggle, input and text: {children:?}");
    };

    (
        id(Some(*on_press)),
        id(Some(*on_change)),
        id(*change),
        id(*on_submit),
    )
}

fn controls() -> (
    Sandbox,
    tempfile::TempDir,
    Vec<Placement>,
    (u64, u64, u64, u64),
) {
    let fixture = Fixture::new(r#""ui.add""#, CONTROLS);
    let mut sandbox = Sandbox::load(&fixture.dir(), Instant::now()).unwrap();
    sandbox.set_instances(&[header_instance("s1")]).unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    let ids = control_ids(&placements);

    (sandbox, fixture.root, placements, ids)
}

#[test]
fn a_button_press_runs_its_callback_and_renders_again() {
    let (mut sandbox, _root, _first, (button, ..)) = controls();

    sandbox
        .handle(&press(&sandbox, button, Action::Press))
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();

    let Node::Row { children, .. } = &placements[0].node else {
        panic!("expected a row");
    };
    let Node::Button {
        label,
        children: inside,
        ..
    } = &children[0]
    else {
        panic!("expected a button");
    };
    assert_eq!(label, "Go");
    assert_eq!(
        inside,
        &[Node::Text {
            text: "Go s1 1".to_owned(),
            tone: Tone::Normal
        }]
    );
}

#[test]
fn toggles_and_inputs_pass_their_value() {
    let (mut sandbox, _root, _first, (_, toggle, change, submit)) = controls();

    sandbox
        .handle(&press(&sandbox, toggle, Action::Toggle { value: true }))
        .unwrap();
    sandbox
        .handle(&press(
            &sandbox,
            change,
            Action::Text {
                value: "hi".to_owned(),
            },
        ))
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    let Node::Row { children, .. } = &placements[0].node else {
        panic!("expected a row");
    };
    assert!(matches!(children[1], Node::Toggle { value: true, .. }));
    assert!(matches!(&children[2], Node::Input { value, .. } if value == "hi"));

    // The ids were made again, so submit is read from the new render.
    let (_, _, _, submit_now) = control_ids(&placements);
    assert_ne!(submit_now, submit, "an id is never reused");
    sandbox
        .handle(&press(
            &sandbox,
            submit_now,
            Action::Text {
                value: "sent".to_owned(),
            },
        ))
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    let Node::Row { children, .. } = &placements[0].node else {
        panic!("expected a row");
    };
    assert_eq!(
        children[3],
        Node::Text {
            text: "sent".to_owned(),
            tone: Tone::Dim
        }
    );
}

#[test]
fn stale_unknown_and_mismatched_events_are_ignored() {
    let (mut sandbox, _root, _first, (button, toggle, change, _)) = controls();

    // The press renders again, which makes new ids.
    sandbox
        .handle(&press(&sandbox, button, Action::Press))
        .unwrap();
    sandbox.render().unwrap();

    // The old id does nothing now.
    sandbox
        .handle(&press(&sandbox, button, Action::Press))
        .unwrap();
    // Neither does an id nobody had.
    sandbox
        .handle(&press(&sandbox, 9999, Action::Press))
        .unwrap();
    assert!(sandbox.render().unwrap().is_none());

    // A fresh id with the wrong kind of value is ignored too.
    let placements = {
        sandbox
            .set_instances(&[header_instance("s1"), header_instance("s2")])
            .unwrap();
        sandbox.render().unwrap().unwrap()
    };
    let (new_button, new_toggle, new_change, _) = control_ids(&placements[..1]);
    assert!(new_button > button && new_toggle > toggle && new_change > change);
    sandbox
        .handle(&press(&sandbox, new_button, Action::Toggle { value: true }))
        .unwrap();
    sandbox
        .handle(&press(
            &sandbox,
            new_toggle,
            Action::Text {
                value: "x".to_owned(),
            },
        ))
        .unwrap();
    sandbox
        .handle(&press(&sandbox, new_change, Action::Press))
        .unwrap();
    assert!(sandbox.render().unwrap().is_none());
}

#[test]
fn the_ids_of_an_instance_that_left_are_stale() {
    let (mut sandbox, _root, _first, (button, ..)) = controls();

    sandbox.set_instances(&[]).unwrap();
    sandbox.render().unwrap();
    sandbox
        .handle(&press(&sandbox, button, Action::Press))
        .unwrap();

    assert!(sandbox.render().unwrap().is_none());
}

#[test]
fn a_callback_that_throws_fails_the_event() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () => beep.button({ label: "x", onPress: () => { throw new Error("nope"); } }));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    let Node::Button { on_press, .. } = &placements[0].node else {
        panic!("expected a button");
    };
    let event = UiEvent {
        plugin: sandbox.id.clone(),
        id: *on_press,
        action: Action::Press,
    };

    let error = sandbox.handle(&event).unwrap_err();

    assert!(error.to_string().contains("nope"), "{error}");
}

#[test]
fn controls_need_their_callbacks() {
    for tree in [
        r#"beep.button({ label: "x" })"#,
        r#"beep.button({ label: "x", onPress: "go" })"#,
        r#"beep.toggle({ label: "x", value: true })"#,
        r#"beep.input({ label: "x", onChange: 3 })"#,
        r#"{ kind: "button", label: "x", onPress: () => {}, extra: 1 }"#,
    ] {
        let fixture = Fixture::new(
            r#""ui.add""#,
            &format!(r#"beep.place({{ slot: "composer" }}, () => ({tree}));"#),
        );
        let mut sandbox = fixture.load(Instant::now()).unwrap();

        assert!(sandbox.render().is_err(), "{tree}");
    }
}

#[test]
fn session_rows_take_no_controls() {
    for tree in [
        r#"beep.button({ label: "x", onPress: () => {} })"#,
        r#"beep.row({}, [beep.input({ label: "x" })])"#,
    ] {
        let fixture = Fixture::new(
            r#""ui.add""#,
            &format!(r#"beep.place({{ slot: "session.row" }}, () => {tree});"#),
        );
        let mut sandbox = Sandbox::load(&fixture.dir(), Instant::now()).unwrap();
        sandbox.set_instances(&[row_instance("a", "A")]).unwrap();

        assert!(
            matches!(
                sandbox.render(),
                Err(SandboxError::NotInteractive(Slot::SessionRow))
            ),
            "{tree}"
        );
    }
}

#[test]
fn the_composer_context_is_machine_and_session() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, (c) => beep.text(`${c.machine}:${c.session}`));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    assert_eq!(text_of(sandbox.render().unwrap()), "build-box:s1");
}

/// The example, copied so that its store lands in a temp folder and not in the repository.
fn session_notes() -> (tempfile::TempDir, Sandbox) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/examples/session-notes");
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("session-notes");
    std::fs::create_dir(&dir).unwrap();
    for file in ["beep-plugin.json", "main.js"] {
        std::fs::copy(from.join(file), dir.join(file)).unwrap();
    }
    let sandbox = Sandbox::load(&dir, Instant::now()).unwrap();

    (root, sandbox)
}

/// Calls a tool of the sandbox and returns its answer.
fn call_tool(
    sandbox: &mut Sandbox,
    name: &str,
    input: serde_json::Value,
) -> Result<String, ToolError> {
    let (reply, answer) = std::sync::mpsc::channel();
    let call = ToolCall {
        name: name.parse().unwrap(),
        input: input.to_string(),
        reply,
    };
    sandbox.call_tool(call, Instant::now()).unwrap();

    answer.try_recv().expect("the handler answered at once")
}

#[test]
fn session_notes_tools_read_and_write_the_notes_the_rows_show() {
    let (_root, mut sandbox) = session_notes();
    sandbox
        .set_instances(&[row_instance("a", "Alpha")])
        .unwrap();
    assert!(sandbox.render().unwrap().unwrap().is_empty());

    let set = call_tool(
        &mut sandbox,
        "set_note",
        json!({ "session": "a", "note": " ship it " }),
    );
    assert_eq!(set.unwrap(), r#"{"note":"ship it"}"#);
    assert_eq!(
        call_tool(&mut sandbox, "get_note", json!({ "session": "a" })).unwrap(),
        r#"{"note":"ship it"}"#
    );
    assert_eq!(
        call_tool(&mut sandbox, "get_note", json!({ "session": "b" })).unwrap(),
        r#"{"note":""}"#
    );

    // The row the UI shows follows the tool's write.
    let placements = sandbox.render().unwrap().unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(text_of(Some(placements)), "ship it");
}

#[test]
fn session_notes_draws_a_note_per_row_from_the_header() {
    let (_root, mut sandbox) = session_notes();
    sandbox
        .set_instances(&[
            row_instance("a", "Alpha"),
            row_instance("b", "Beta"),
            header_instance("a"),
        ])
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();

    // No note yet, so the rows draw nothing and the header has its controls.
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].slot, Slot::SessionHeader);
    let Node::Row { children, .. } = &placements[0].node else {
        panic!("expected a row");
    };
    let Node::Input {
        on_change,
        on_submit,
        ..
    } = &children[0]
    else {
        panic!("expected an input");
    };
    let (on_change, on_submit) = (on_change.unwrap(), on_submit.unwrap());

    let event = |id: CallbackId, value: &str| UiEvent {
        plugin: sandbox_id(),
        id,
        action: Action::Text {
            value: value.to_owned(),
        },
    };
    sandbox.handle(&event(on_change, "  ship it ")).unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    assert_eq!(placements.len(), 1, "typing alone adds no badge");
    let Node::Row { children, .. } = &placements[0].node else {
        panic!("expected a row");
    };
    let Node::Input {
        on_submit: submit,
        value,
        ..
    } = &children[0]
    else {
        panic!("expected an input");
    };
    assert_eq!(value, "  ship it ");
    let _ = on_submit;

    sandbox
        .handle(&event(submit.unwrap(), "  ship it "))
        .unwrap();
    let placements = sandbox.render().unwrap().unwrap();
    let rows: Vec<(String, String)> = placements
        .iter()
        .filter(|p| p.slot == Slot::SessionRow)
        .map(|p| texts_by_instance(std::slice::from_ref(p))[0].clone())
        .collect();
    assert_eq!(rows, [("a".to_owned(), "ship it".to_owned())]);
    assert!(matches!(
        placements
            .iter()
            .find(|p| p.slot == Slot::SessionRow)
            .unwrap()
            .node,
        Node::Text {
            tone: Tone::Accent,
            ..
        }
    ));
}

#[test]
fn plugin_capabilities_report_the_build_and_cannot_be_changed() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
          let refused = false;
          try { beep.capabilities.tools = !beep.capabilities.tools; }
          catch { refused = true; }
          beep.place({ slot: "composer" }, () => beep.text(
            `${beep.capabilities.tools}:${Object.isFrozen(beep.capabilities)}:${refused}`
          ));
        "#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    assert_eq!(text_of(sandbox.render().unwrap()), "true:true:true");
}

fn sandbox_id() -> PluginId {
    "session-notes".parse().unwrap()
}

/// A fixture with two more frames, `art/a.png` and `art/b.png`, of other bytes than `art/cat.png`.
fn sprite_fixture(main: &str) -> Fixture {
    let fixture = Fixture::new(r#""ui.add""#, main);
    std::fs::write(fixture.dir().join("art/a.png"), b"\x89PNG\r\n\x1a\nframe a").unwrap();
    std::fs::write(fixture.dir().join("art/b.png"), b"\x89PNG\r\n\x1a\nframe b").unwrap();

    fixture
}

fn sprite_error(motions: &str) -> SandboxError {
    let fixture = sprite_fixture(&format!(
        r#"beep.place({{ slot: "composer" }}, () =>
          beep.sprite({{ rest: "art/cat.png", width: 8, height: 8, motions: {motions} }}));"#
    ));
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    sandbox.render().unwrap_err()
}

#[test]
fn a_sprite_lists_each_frame_once_and_steps_point_at_them() {
    let fixture = sprite_fixture(
        r#"beep.place({ slot: "composer" }, () => beep.sprite({
          rest: "art/cat.png", width: 8, height: 6, pixelated: true,
          motions: [
            { steps: [{ frame: "art/a.png", ms: 100 }, { frame: "art/cat.png", ms: 50 }],
              repeat: { every: { min: 1000, max: 2000 } } },
            { steps: [{ frame: "art/b.png", ms: 70 }, { frame: "art/a.png", ms: 70 }] },
          ] }));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    let node = only_node(sandbox.render().unwrap());

    let Node::Sprite {
        frames,
        rest,
        width,
        height,
        pixelated,
        motions,
        ..
    } = node
    else {
        panic!("expected a sprite");
    };
    assert_eq!((rest, width, height, pixelated), (0, 8, 6, true));
    assert_eq!(frames.len(), 3);
    assert_eq!(
        frames[0].hash.to_string(),
        blake3::hash(PNG).to_hex().to_string()
    );
    assert_eq!(
        motions,
        vec![
            Motion {
                steps: vec![Step { frame: 1, ms: 100 }, Step { frame: 0, ms: 50 }],
                repeat: Repeat::Every {
                    min_ms: 1000,
                    max_ms: 2000
                },
            },
            Motion {
                steps: vec![Step { frame: 2, ms: 70 }, Step { frame: 1, ms: 70 }],
                repeat: Repeat::Once,
            },
        ]
    );
    assert_eq!(sandbox.assets().len(), 3);
    assert_eq!(sandbox.next_timer(), None);
}

#[test]
fn a_sprite_node_serializes_with_indexes_and_camel_case() {
    let fixture = sprite_fixture(
        r#"beep.place({ slot: "composer" }, () => beep.sprite({
          rest: "art/cat.png", width: 8, height: 8,
          motions: [{ steps: [{ frame: "art/a.png", ms: 100 }], repeat: { every: { min: 500, max: 900 } } }] }));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    let node = only_node(sandbox.render().unwrap());
    let json = serde_json::to_value(&node).unwrap();

    assert_eq!(json["kind"], "sprite");
    assert_eq!(json["rest"], 0);
    assert_eq!(json["frames"][0]["mime"], "image/png");
    assert_eq!(json["frames"][1]["hash"].as_str().unwrap().len(), 64);
    assert_eq!(
        json["motions"][0],
        json!({ "steps": [{ "frame": 1, "ms": 100 }], "repeat": { "kind": "every", "minMs": 500, "maxMs": 900 } })
    );
}

#[test]
fn a_sprite_declares_reactions_and_asks_for_one() {
    let fixture = sprite_fixture(
        r#"beep.place({ slot: "composer" }, () => beep.sprite({
          rest: "art/cat.png", width: 8, height: 8,
          reactions: { happy: { steps: [{ frame: "art/a.png", ms: 100 }, { frame: "art/b.png", ms: 80 }] } },
          play: { name: "happy", id: 3 } }));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    let node = only_node(sandbox.render().unwrap());
    let json = serde_json::to_value(&node).unwrap();

    assert_eq!(
        json["reactions"],
        json!({ "happy": [{ "frame": 1, "ms": 100 }, { "frame": 2, "ms": 80 }] })
    );
    assert_eq!(json["play"], json!({ "name": "happy", "id": 3 }));
    assert_eq!(json["frames"].as_array().unwrap().len(), 3);
}

#[test]
fn a_sprite_cannot_play_a_reaction_it_lacks() {
    let fixture = sprite_fixture(
        r#"beep.place({ slot: "composer" }, () => beep.sprite({
          rest: "art/cat.png", width: 8, height: 8,
          play: { name: "happy", id: 1 } }));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    assert!(matches!(
        sandbox.render().unwrap_err(),
        SandboxError::Sprite(SpriteError::NoReaction(name)) if name == "happy"
    ));
}

#[test]
fn a_placement_declares_how_it_enters_and_leaves() {
    let fixture = sprite_fixture(
        r#"beep.place({ slot: "composer", enter: "rise", leave: "fade" }, () =>
             beep.text("a"));
           beep.place({ slot: "session.row" }, () => beep.text("b"));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();
    sandbox
        .set_instances(&[composer_instance(), row_instance("r", "Row")])
        .unwrap();

    let placements = sandbox.render().unwrap().unwrap();

    let json = serde_json::to_value(&placements[0]).unwrap();
    assert_eq!(
        (json["enter"].as_str(), json["leave"].as_str()),
        (Some("rise"), Some("fade"))
    );
    let json = serde_json::to_value(&placements[1]).unwrap();
    assert_eq!(
        (json["enter"].as_str(), json["leave"].as_str()),
        (Some("none"), Some("none"))
    );
}

#[test]
fn an_image_used_twice_is_one_asset() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.place({ slot: "composer" }, () => beep.row({ gap: 1 }, [
          beep.image({ src: "art/cat.png", width: 1, height: 1 }),
          beep.image({ src: "art/cat.png", width: 2, height: 2 }),
        ]));"#,
    );
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    sandbox.render().unwrap();

    assert_eq!(sandbox.assets().len(), 1);
}

#[test]
fn sprite_frames_must_exist_and_be_images() {
    let missing = sprite_error(r#"[{ steps: [{ frame: "art/none.png", ms: 100 }] }]"#);
    assert!(matches!(
        missing,
        SandboxError::Image {
            reason: ImageError::Io(_),
            ..
        }
    ));

    let text = sprite_error(r#"[{ steps: [{ frame: "main.js", ms: 100 }] }]"#);
    assert!(matches!(
        text,
        SandboxError::Image {
            reason: ImageError::NotImage,
            ..
        }
    ));
}

#[test]
fn sprite_durations_counts_and_ranges_are_bounded() {
    let step = |ms: u32| format!(r#"{{ frame: "art/a.png", ms: {ms} }}"#);
    let one = |ms: u32| format!("[{{ steps: [{}] }}]", step(ms));

    for ms in [0, 15, 60_001] {
        assert!(matches!(
            sprite_error(&one(ms)),
            SandboxError::Sprite(SpriteError::StepDuration(_))
        ));
    }
    assert!(matches!(
        sprite_error("[{ steps: [] }]"),
        SandboxError::Sprite(SpriteError::NoSteps)
    ));

    let many = vec![step(20); 65].join(",");
    assert!(matches!(
        sprite_error(&format!("[{{ steps: [{many}] }}]")),
        SandboxError::Sprite(SpriteError::TooManySteps(65))
    ));

    let motions = vec![format!("{{ steps: [{}] }}", step(20)); 9].join(",");
    assert!(matches!(
        sprite_error(&format!("[{motions}]")),
        SandboxError::Sprite(SpriteError::TooManyMotions(9))
    ));

    for (min, max) in [(99, 500), (500, 400), (500, 3_600_001)] {
        let every = format!(
            "[{{ steps: [{}], repeat: {{ every: {{ min: {min}, max: {max} }} }} }}]",
            step(20)
        );
        assert!(matches!(
            sprite_error(&every),
            SandboxError::Sprite(SpriteError::Range { .. })
        ));
    }

    let two_loops = format!(
        r#"[{{ steps: [{0}], repeat: "loop" }}, {{ steps: [{0}] }}]"#,
        step(20)
    );
    assert!(matches!(
        sprite_error(&two_loops),
        SandboxError::Sprite(SpriteError::LoopAlone)
    ));
}

#[test]
fn a_sprite_has_at_most_thirty_two_frames() {
    let fixture = sprite_fixture("");
    for index in 0..33 {
        std::fs::write(
            fixture.dir().join(format!("art/f{index}.png")),
            format!("PNG frame {index}"),
        )
        .unwrap();
    }
    let steps: Vec<String> = (0..33)
        .map(|index| format!(r#"{{ frame: "art/f{index}.png", ms: 20 }}"#))
        .collect();
    std::fs::write(
        fixture.dir().join("main.js"),
        format!(
            r#"beep.place({{ slot: "composer" }}, () => beep.sprite({{ rest: "art/cat.png", width: 1, height: 1,
              motions: [{{ steps: [{}] }}] }}));"#,
            steps.join(",")
        ),
    )
    .unwrap();
    let mut sandbox = fixture.load(Instant::now()).unwrap();

    // 33 steps use 33 frames, and the rest frame makes 34.
    assert!(matches!(
        sandbox.render().unwrap_err(),
        SandboxError::Sprite(SpriteError::TooManyFrames(34))
    ));
}

#[test]
fn a_plugin_over_its_cpu_budget_is_stopped() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        const word = beep.state("a");
        beep.place({ slot: "composer" }, () => beep.text(word.get()));
        beep.after(20, () => {
          let n = 0;
          for (let i = 0; i < 300000; i++) n += i;
          word.set("b" + (n > 0));
        });
        "#,
    );
    let start = Instant::now();
    let meter = Arc::new(Meter::new(Duration::from_millis(25)));
    let mut sandbox =
        with_composer(Sandbox::load_metered(&fixture.dir(), start, meter.clone()).unwrap());
    // The meter now holds the whole budget, so any CPU the timer uses is over it.
    meter.record(Instant::now(), Duration::from_millis(25));

    let error = sandbox
        .fire_due(start + Duration::from_millis(20))
        .unwrap_err();

    assert!(
        matches!(error, SandboxError::OverBudget { budget_ms: 25, .. }),
        "{error}"
    );
    assert!(error.to_string().contains("budget is 25 ms"), "{error}");
    assert!(meter.used(Instant::now()) > Duration::from_millis(25));
}

#[test]
fn a_cheap_plugin_stays_within_the_default_budget() {
    let cat = cat_dir();
    let meter = Arc::new(Meter::new(crate::meter::BUDGET));
    let mut sandbox =
        with_composer(Sandbox::load_metered(&cat, Instant::now(), meter.clone()).unwrap());

    for _ in 0..200 {
        sandbox.render().unwrap();
        sandbox
            .deliver(&update(
                topic("ui.composer", None, None),
                None,
                json!({ "machine": "m", "session": "s" }),
            ))
            .unwrap();
    }

    assert!(meter.used(Instant::now()) < crate::meter::BUDGET);
}

#[test]
fn console_and_beep_log_write_to_the_app_log_like_console_log() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        console.log("seen-a", { n: 1 }, [2], null);
        console.warn("seen-b", undefined);
        console.error(new Error("seen-c"));
        beep.log("seen-d", 4, true);
        "#,
    );

    // The first call installs the capture, so it comes before the plugin runs.
    crate::console::tests::captured("test-plugin");
    fixture.load(Instant::now()).unwrap();

    let lines = crate::console::tests::captured("test-plugin");
    let found = |text: &str| lines.iter().find(|(_, line)| line.starts_with(text));
    assert_eq!(
        found("seen-a"),
        Some(&(Level::Info, r#"seen-a {"n":1} [2] null"#.to_owned()))
    );
    assert_eq!(
        found("seen-b"),
        Some(&(Level::Warn, "seen-b undefined".to_owned()))
    );
    assert_eq!(
        found("Error: seen-c"),
        Some(&(Level::Error, "Error: seen-c".to_owned()))
    );
    assert_eq!(
        found("seen-d"),
        Some(&(Level::Info, "seen-d 4 true".to_owned()))
    );
}

#[test]
fn the_store_survives_a_reload_and_is_written_after_the_debounce() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"
        const runs = (beep.store.get("runs") ?? 0) + 1;
        beep.store.set("runs", runs);
        beep.store.set("nested", { list: [1, "two"] });
        beep.store.delete("gone");
        beep.place({ slot: "composer" }, () => beep.text(`run ${runs}`));
        "#,
    );
    let file = fixture.root.path().join(".data/test-plugin.json");
    let start = Instant::now();

    let mut first = fixture.load(start).unwrap();
    assert_eq!(texts_of(first.render().unwrap()), ["run 1"]);
    assert!(!file.exists());
    assert!(first.next_timer().is_some());

    first.fire_due(start + Duration::from_millis(500)).unwrap();
    assert!(!file.exists());
    first.fire_due(start + Duration::from_secs(5)).unwrap();
    assert!(file.is_file());
    assert!(first.next_timer().is_none());
    drop(first);

    let mut second = fixture.load(start).unwrap();
    assert_eq!(texts_of(second.render().unwrap()), ["run 2"]);
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        saved,
        json!({ "runs": 1, "nested": { "list": [1, "two"] } })
    );

    // Dropping the sandbox writes what is pending.
    drop(second);
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["runs"], 2);
}

#[test]
fn a_store_set_over_the_cap_throws() {
    let fixture = Fixture::new(
        r#""ui.add""#,
        r#"beep.store.set("big", "x".repeat(300000));"#,
    );

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(error.to_string().contains("beep.store is full"), "{error}");
}

#[test]
fn a_store_value_must_be_json() {
    let fixture = Fixture::new(r#""ui.add""#, "beep.store.set('f', () => 1);");

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(error.to_string().contains("needs a JSON value"), "{error}");
}

fn texts_of(placements: Option<Vec<Placement>>) -> Vec<String> {
    placements
        .expect("a render")
        .into_iter()
        .filter_map(|placement| match placement.node {
            Node::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// Adds files to the plugin folder, then loads it.
fn load_with(main: &str, files: &[(&str, &str)], now: Instant) -> Result<Sandbox, SandboxError> {
    let fixture = Fixture::new(r#""ui.add""#, main);
    for (name, text) in files {
        let path = fixture.dir().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    // Every import is read at load, so the folder may go with the fixture.
    fixture.load(now)
}

#[test]
fn a_plugin_imports_its_other_files_and_they_see_beep() {
    let mut sandbox = load_with(
        r#"
        import { label } from "./lib/draw.js";
        import { shout } from "./lib/loud.js";
        beep.place({ slot: "composer" }, () => beep.text(shout(label("hi"))));
        "#,
        &[
            (
                "lib/draw.js",
                "import { wrap } from '../wrap.js';\nexport function label(s) { return wrap(s, '<', '>'); }",
            ),
            (
                "lib/loud.js",
                "export const shout = (s) => s.toUpperCase() + beep.state(1).get();",
            ),
            (
                "wrap.js",
                "export const wrap = (s, a, b) => a + s + b;",
            ),
        ],
        Instant::now(),
    )
    .unwrap();
    sandbox.set_instances(&[composer_instance()]).unwrap();

    assert_eq!(texts_of(sandbox.render().unwrap()), ["<HI>1"]);
}

#[test]
fn the_shipped_examples_load_as_compiled_javascript() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/examples");

    for name in ["composer-cat", "session-notes"] {
        let folder = examples.join(name);
        let manifest = Manifest::read(&folder).unwrap();
        assert_eq!(manifest.entry, PathBuf::from("main.js"));
        Sandbox::load(&folder, Instant::now()).unwrap_or_else(|error| {
            panic!("compiled example {name} did not load: {error}");
        });
    }
}

#[test]
fn imports_that_cycle_load_as_quickjs_loads_them() {
    let mut sandbox = load_with(
        r#"
        import { a } from "./a.js";
        beep.place({ slot: "composer" }, () => beep.text(a()));
        "#,
        &[
            (
                "a.js",
                "import { b } from './b.js';\nexport function a() { return 'a' + b(); }",
            ),
            (
                "b.js",
                "import { a } from './a.js';\nexport function b() { return typeof a; }",
            ),
        ],
        Instant::now(),
    )
    .unwrap();
    sandbox.set_instances(&[composer_instance()]).unwrap();

    assert_eq!(texts_of(sandbox.render().unwrap()), ["afunction"]);
}

#[test]
fn imports_outside_the_rules_are_refused() {
    let cases = [
        ("import '../outside.js';", "leaves the plugin folder"),
        ("import 'lodash';", "only relative imports"),
        ("import '/etc/passwd.js';", "only relative imports"),
        (
            "import 'https://example.com/x.js';",
            "only relative imports",
        ),
        ("import './data.json';", "only .js"),
        ("import './notes.txt';", "only .js"),
        ("import './source.ts';", "build typescript to javascript"),
        ("import './missing.js';", "cannot read"),
        ("import './bad.js';", "expected"),
    ];

    for (main, reason) in cases {
        let files = [
            ("data.json", "{}"),
            ("notes.txt", ""),
            ("bad.js", "const = ;"),
        ];

        let error = load_with(main, &files, Instant::now())
            .err()
            .unwrap_or_else(|| panic!("{main} loads"));

        assert!(
            error.to_string().to_lowercase().contains(reason),
            "{main}: {error}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_folder_is_refused() {
    let fixture = Fixture::new(r#""ui.add""#, "import './link.js';");
    std::fs::write(fixture.root.path().join("outside.js"), "1;").unwrap();
    std::os::unix::fs::symlink("../outside.js", fixture.dir().join("link.js")).unwrap();

    let error = fixture.load(Instant::now()).err().expect("load fails");

    assert!(
        error.to_string().contains("outside the plugin folder"),
        "{error}"
    );
}

#[test]
fn too_many_files_are_refused() {
    let imports: String = (0..40).map(|n| format!("import './f{n}.js';\n")).collect();
    let files: Vec<(String, &str)> = (0..40).map(|n| (format!("f{n}.js"), "1;")).collect();
    let files: Vec<(&str, &str)> = files.iter().map(|(n, t)| (n.as_str(), *t)).collect();

    let error = load_with(&imports, &files, Instant::now())
        .err()
        .expect("load fails");

    assert!(error.to_string().contains("more than 32 files"), "{error}");
}

#[test]
fn an_imported_file_runs_under_the_budget_and_reports_its_errors() {
    let looping = load_with(
        "import './spin.js';",
        &[("spin.js", "while (true) {}")],
        Instant::now(),
    );
    assert!(matches!(looping.err(), Some(SandboxError::Script(_))));

    let throwing = load_with(
        "import './boom.js';",
        &[("boom.js", "throw new Error('imported boom');")],
        Instant::now(),
    );
    let error = throwing.err().expect("load fails");
    assert!(error.to_string().contains("imported boom"), "{error}");
}
