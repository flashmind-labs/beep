use serde_json::json;

use super::*;

fn key(text: &str) -> InstanceKey {
    InstanceKey::try_from(text.to_owned()).unwrap()
}

fn row(session: &str, title: &str) -> Instance {
    Instance {
        key: key(session),
        context: SlotContext::SessionRow(SessionRowContext {
            machine: "build-box".to_owned(),
            session: session.to_owned(),
            title: title.to_owned(),
        }),
    }
}

fn header(session: &str) -> Instance {
    Instance {
        key: key(session),
        context: SlotContext::SessionHeader(SessionHeaderContext {
            machine: "build-box".to_owned(),
            session: session.to_owned(),
        }),
    }
}

fn shown(instances: Vec<Instance>) -> InstanceBatch {
    InstanceBatch {
        shown: instances,
        ..InstanceBatch::default()
    }
}

#[test]
fn an_instance_reads_and_writes_the_page_shape() {
    let wire = json!({
        "key": "s1",
        "slot": "session.row",
        "context": { "machine": "build-box", "session": "s1", "title": "Fix it" },
    });

    let instance: Instance = serde_json::from_value(wire.clone()).unwrap();

    assert_eq!(instance, row("s1", "Fix it"));
    assert_eq!(instance.context.slot(), Slot::SessionRow);
    assert_eq!(serde_json::to_value(&instance).unwrap(), wire);
}

#[test]
fn a_context_that_does_not_fit_its_slot_is_refused() {
    let wrong = json!({ "key": "s1", "slot": "session.row", "context": { "machine": "m", "session": "s" } });
    let extra = json!({ "key": "s1", "slot": "composer", "context": { "machine": "m", "session": "s", "x": 1 } });
    let unknown = json!({ "key": "s1", "slot": "sidebar", "context": {} });

    for wire in [wrong, extra, unknown] {
        assert!(serde_json::from_value::<Instance>(wire).is_err());
    }
}

#[test]
fn keys_are_short_and_printable() {
    assert!(InstanceKey::try_from(String::new()).is_err());
    assert!(InstanceKey::try_from("a".repeat(129)).is_err());
    assert!(InstanceKey::try_from("a\nb".to_owned()).is_err());
    assert!(InstanceKey::try_from("a".repeat(128)).is_ok());
}

#[test]
fn a_batch_checks_the_context_size_and_its_own() {
    let long = shown(vec![row("s1", &"t".repeat(513))]);
    assert_eq!(long.check(), Err(InstanceError::Field(513)));

    let big = Instance {
        key: key("s1"),
        context: SlotContext::SessionRow(SessionRowContext {
            machine: "m".repeat(512),
            session: "s".repeat(512),
            title: "t".repeat(512),
        }),
    };
    assert!(matches!(
        shown(vec![big]).check(),
        Err(InstanceError::Context(_))
    ));

    let many = shown((0..1001).map(|i| header(&i.to_string())).collect());
    assert_eq!(many.check(), Err(InstanceError::Batch(1001)));
}

#[test]
fn instances_come_and_go_and_a_reset_starts_over() {
    let mut instances = Instances::default();

    assert!(instances.apply(shown(vec![row("a", "A"), row("b", "B"), header("a")])));
    assert_eq!(instances.list().len(), 3);

    // The same context again changes nothing. A new title does.
    assert!(!instances.apply(shown(vec![row("a", "A")])));
    assert!(instances.apply(shown(vec![row("a", "A2")])));

    let gone = InstanceBatch {
        gone: vec![InstanceRef {
            slot: Slot::SessionRow,
            key: key("a"),
        }],
        ..InstanceBatch::default()
    };
    assert!(instances.apply(gone));
    let keys: Vec<(Slot, String)> = instances
        .list()
        .iter()
        .map(|i| (i.context.slot(), i.key.to_string()))
        .collect();
    assert_eq!(
        keys,
        [
            (Slot::SessionRow, "b".to_owned()),
            (Slot::SessionHeader, "a".to_owned())
        ]
    );

    let reset = InstanceBatch {
        reset: true,
        shown: vec![row("c", "C")],
        gone: vec![],
    };
    assert!(instances.apply(reset));
    assert_eq!(instances.list().len(), 1);
}

#[test]
fn a_slot_holds_at_most_two_hundred_instances() {
    let mut instances = Instances::default();
    let all: Vec<Instance> = (0..250).map(|i| row(&format!("s{i}"), "t")).collect();

    instances.apply(shown(all));

    assert_eq!(instances.list().len(), MAX_INSTANCES_PER_SLOT);
    // Another slot has its own room, and a held instance can still change.
    assert!(instances.apply(shown(vec![header("h"), row("s0", "new")])));
    assert_eq!(instances.list().len(), MAX_INSTANCES_PER_SLOT + 1);
}
