use super::*;

fn machine() -> EndpointId {
    iroh::SecretKey::generate().public()
}

fn window_follows(id: Uuid) -> Interest {
    Interest {
        machine: None,
        topic: Topic::keyed("terminal-output", id.to_string()),
    }
}

fn place(machine: EndpointId, task: u32, pty: bool) -> Place {
    Place {
        id: Uuid::new_v4(),
        machine,
        task,
        pty,
    }
}

#[test]
fn a_window_follows_a_terminal_on_the_topic_its_node_keys_by_number() {
    let (new, old) = (machine(), machine());
    let (pty, task, on_old) = (
        place(new, 7, true),
        place(new, 7, false),
        place(old, 3, true),
    );
    let places = [pty.clone(), task.clone(), on_old.clone()];
    let interests = vec![
        window_follows(pty.id),
        window_follows(task.id),
        window_follows(on_old.id),
    ];
    let node = node_interests(interests, &places, |each| each == old);

    assert_eq!(
        node,
        vec![
            Interest {
                machine: Some(new.to_string()),
                topic: Topic::keyed("terminal-output", "7"),
            },
            Interest {
                machine: Some(new.to_string()),
                topic: Topic::keyed("process-output", "7"),
            },
            // A node from before is asked for the list that changes with the terminal.
            Interest {
                machine: Some(old.to_string()),
                topic: Topic::new("terminals"),
            },
        ]
    );
}

#[test]
fn other_interests_pass_and_a_terminal_the_mesh_lacks_waits() {
    let other = Interest {
        machine: None,
        topic: Topic::new("terminals"),
    };
    let interests = vec![other.clone(), window_follows(Uuid::new_v4())];

    assert_eq!(node_interests(interests, &[], |_| false), vec![other]);
}

#[test]
fn only_a_topic_naming_one_terminal_is_followed_as_one() {
    let id = Uuid::new_v4();

    assert_eq!(followed(&window_follows(id).topic), Some(id));
    assert_eq!(followed(&Topic::new("terminal-output")), None);
    assert_eq!(followed(&Topic::keyed("terminal-output", "7")), None);
    assert_eq!(followed(&Topic::keyed("terminals", id.to_string())), None);
}
