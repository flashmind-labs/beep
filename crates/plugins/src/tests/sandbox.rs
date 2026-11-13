use super::*;
use crate::ui::{Align, Slot};

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
        std::fs::write(dir.join("main.ts"), main).unwrap();
        std::fs::write(dir.join("art/cat.png"), PNG).unwrap();
        std::fs::write(root.path().join("outside.png"), PNG).unwrap();

        Self { root }
    }

    fn dir(&self) -> PathBuf {
        self.root.path().join("test-plugin")
    }

    fn load(&self, now: Instant) -> Result<Sandbox, SandboxError> {
        Sandbox::load(&self.dir(), now)
    }
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
        const src: string = "art/cat.png";
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
    assert_eq!(placement.align, Align::End);
    assert_eq!((placement.offset, placement.sink), (84, 4));
    let Node::Image {
        src,
        width,
        height,
        pixelated,
    } = &placement.node
    else {
        panic!("expected an image");
    };
    assert!(src.starts_with("data:image/png;base64,"));
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
            text: "1".to_owned()
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
        r#"beep.place({ slot: "composer" }, () => beep.image({ src: "main.ts", width: 1, height: 1 }));"#,
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
    assert!(slot.load(Instant::now()).unwrap().render().is_err());

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

fn image_src(node: &Node) -> &str {
    match node {
        Node::Image { src, .. } => src,
        other => panic!("expected an image, got {other:?}"),
    }
}

#[test]
fn composer_cat_example_runs() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/examples/composer-cat");
    let start = Instant::now();
    let mut sandbox = Sandbox::load(&dir, start).unwrap();

    let first = sandbox.render().unwrap().expect("a first render");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].slot, Slot::Composer);
    let idle = image_src(&first[0].node).to_owned();
    assert!(idle.starts_with("data:image/png;base64,"));

    let mut seen = vec![idle.clone()];
    for tick in 1..=1200u64 {
        sandbox
            .fire_due(start + Duration::from_millis(tick * 50))
            .unwrap();
        if let Some(placements) = sandbox.render().unwrap() {
            assert_eq!(placements.len(), 1);
            let src = image_src(&placements[0].node);
            assert!(src.starts_with("data:image/png;base64,"));
            seen.push(src.to_owned());
        }
    }

    assert!(seen.iter().any(|src| *src != idle), "a motion fired");
}
