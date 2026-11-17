use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::instance::{ComposerContext, Instance, InstanceKey, SlotContext};
use crate::meter::BUDGET;
use crate::topic::{Delivery, Publication, Topic};
use crate::ui::Node;

const MAIN: &str = r#"
const seen = beep.state("none");
let count = 0;
const shown = beep.state(0);
function tick() { count += 1; shown.set(count); beep.after(20, tick); }
beep.after(20, tick);
beep.on({ name: "ui.composer" }, (event) => seen.set(JSON.stringify(event.data)));
beep.place({ slot: "composer" }, () => beep.text(`${seen.get()}:${shown.get()}`));
"#;

struct Rig {
    _root: tempfile::TempDir,
    job: PluginJob,
    sandbox: Sandbox,
    events: mpsc::Receiver<Event>,
    start: Instant,
}

fn rig(hidden: bool) -> Rig {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("hold-test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("beep-plugin.json"),
        r#"{ "id": "hold-test", "name": "T", "version": "0.1.0", "permissions": ["ui.add"] }"#,
    )
    .unwrap();
    std::fs::write(dir.join("main.js"), MAIN).unwrap();

    let start = Instant::now();
    let meter = Arc::new(Meter::new(BUDGET));
    let sandbox = Sandbox::load_metered(&dir, start, meter.clone()).unwrap();
    let (sender, events) = mpsc::channel();
    let job = PluginJob {
        dir,
        id: "hold-test".parse().unwrap(),
        generation: 1,
        events: sender,
        inbox: Arc::new(Inbox::default()),
        mailbox: Arc::new(Mailbox::default()),
        meter,
        hidden: Arc::new(AtomicBool::new(hidden)),
    };
    job.mailbox.set_instances(Arc::new(vec![Instance {
        key: InstanceKey::try_from("composer".to_owned()).unwrap(),
        context: SlotContext::Composer(ComposerContext {
            machine: "build-box".to_owned(),
            session: "s1".to_owned(),
        }),
    }]));

    Rig {
        _root: root,
        job,
        sandbox,
        events,
        start,
    }
}

fn update(data: serde_json::Value) -> Delivery {
    Delivery::new(&Publication {
        topic: Topic::local("ui.composer").unwrap(),
        machine: None,
        data,
    })
    .unwrap()
}

fn drawn_text(events: &mpsc::Receiver<Event>) -> String {
    let Ok(Event::Shown { placements, .. }) = events.try_recv() else {
        panic!("expected a shown event");
    };
    let Node::Text { text, .. } = &placements[0].node else {
        panic!("expected text");
    };

    text.clone()
}

#[test]
fn a_hidden_plugin_draws_nothing_and_holds_timers_and_updates() {
    let mut rig = rig(true);
    let later = rig.start + Duration::from_secs(600);

    assert!(rig.job.draw(&mut rig.sandbox).unwrap());
    rig.job.inbox.push(update(json!("first")));
    rig.job.inbox.push(update(json!("second")));
    rig.job.on_wake(&mut rig.sandbox, later).unwrap();

    // Nothing was drawn, no timer fired and the newest update waits, alone.
    assert!(matches!(rig.events.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(
        rig.sandbox.next_timer(),
        Some(rig.start + Duration::from_millis(20))
    );
    assert_eq!(rig.job.inbox.take().len(), 1);
}

#[test]
fn a_plugin_that_shows_again_runs_each_overdue_timer_once_and_the_newest_update() {
    let mut rig = rig(true);
    let later = rig.start + Duration::from_secs(600);
    rig.job.inbox.push(update(json!("first")));
    rig.job.inbox.push(update(json!("second")));
    rig.job.on_wake(&mut rig.sandbox, later).unwrap();

    rig.job.hidden.store(false, Ordering::Release);
    rig.job.on_wake(&mut rig.sandbox, later).unwrap();
    assert!(rig.job.draw(&mut rig.sandbox).unwrap());

    // 600 s of 20 ms ticks fired once, not thirty thousand times.
    assert_eq!(drawn_text(&rig.events), "\"second\":1");
    assert_eq!(
        rig.sandbox.next_timer(),
        Some(later + Duration::from_millis(20))
    );
}

#[test]
fn a_visible_plugin_draws_and_runs_timers() {
    let mut rig = rig(false);

    assert!(rig.job.draw(&mut rig.sandbox).unwrap());
    assert_eq!(drawn_text(&rig.events), "none:0");

    rig.job
        .on_wake(&mut rig.sandbox, rig.start + Duration::from_millis(20))
        .unwrap();
    assert!(rig.job.draw(&mut rig.sandbox).unwrap());
    assert_eq!(drawn_text(&rig.events), "none:1");
}
