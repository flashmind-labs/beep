use super::*;
use crate::center::window::hex;
use beep_proto::watched;

fn project(name: &str) -> Alone {
    Alone::Project {
        project: name.to_owned(),
    }
}

#[test]
fn a_window_label_reads_back_as_what_it_shows() {
    let chat = Alone::Chat {
        machine_id: "build-box".into(),
        session: "chat-one".into(),
    };

    for alone in [project("site"), project("docs/API ü"), chat] {
        let label = alone.label();
        assert!(
            label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        );
        assert_eq!(Alone::of(&label), Some(alone));
    }

    assert_eq!(Alone::of(LABEL), None);
    assert_eq!(Alone::of("research-1"), None);
    assert_eq!(Alone::of("project-"), None);
    assert_eq!(Alone::of("project-abc"), None);
    assert_eq!(Alone::of("project-zz"), None);
    assert_eq!(Alone::of(&format!("chat-{}", hex("no-session"))), None);
}

#[test]
fn a_project_open_in_a_window_is_focused() {
    let site = project("site").label();
    let open = [LABEL, "bar", site.as_str(), "research-1"];
    assert_eq!(pick(&project("site"), open), Pick::Focus(site));
}

#[test]
fn a_project_without_a_window_gets_a_new_one() {
    assert_eq!(
        pick(&project("site"), [LABEL, "bar"]),
        Pick::Open {
            label: project("site").label(),
            cascade: 0,
        }
    );
}

#[test]
fn a_new_window_steps_past_the_other_project_and_chat_windows() {
    let docs = project("docs").label();
    let chat = Alone::Chat {
        machine_id: "build-box".into(),
        session: "chat-one".into(),
    }
    .label();
    let open = [LABEL, docs.as_str(), "research-1", chat.as_str()];
    assert_eq!(
        pick(&project("site"), open),
        Pick::Open {
            label: project("site").label(),
            cascade: 2,
        }
    );
}

#[test]
fn similar_names_get_their_own_windows() {
    let site = project("site").label();
    assert!(matches!(
        pick(&project("site2"), [site.as_str()]),
        Pick::Open { .. }
    ));
    assert!(matches!(
        pick(&project("sit"), [site.as_str()]),
        Pick::Open { .. }
    ));
}

#[test]
fn a_page_hears_its_window_state_again_only_when_it_changes() {
    let told = OnScreen::default();

    assert!(told.record(LABEL, false));
    assert!(!told.record(LABEL, false));
    assert!(told.record(LABEL, true));
    assert!(told.record("project-61", true), "each window keeps its own");

    told.clear(LABEL);
    assert!(told.record(LABEL, true), "a closed window starts over");
}

#[test]
fn native_work_hears_whether_its_window_shows() {
    let told = OnScreen::default();
    let mut shown = told.watch(LABEL);
    assert!(!hidden(&shown), "a window never told counts as shown");

    told.record(LABEL, false);
    assert!(shown.has_changed().unwrap_or(false));
    assert!(hidden(&shown));

    told.record(LABEL, true);
    assert!(!hidden(&shown));

    told.clear(LABEL);
    assert!(watched::read_and_update(&mut shown, Option::is_some));
    assert!(shown.has_changed().is_err(), "it closes with the window");
}

#[cfg(desktop)]
#[test]
fn minimizing_and_restoring_reach_the_page_from_resizes_and_focus() {
    use tauri::{PhysicalPosition, PhysicalSize};

    let minimized = || false;
    let restored = || true;

    assert_eq!(
        shown_after(&WindowEvent::Resized(PhysicalSize::new(0, 0)), minimized),
        Some((false, Tell::Changed))
    );
    assert_eq!(
        shown_after(&WindowEvent::Moved(PhysicalPosition::new(0, 0)), restored),
        Some((true, Tell::Changed))
    );
    assert_eq!(
        shown_after(&WindowEvent::Focused(false), minimized),
        Some((false, Tell::Changed))
    );
    assert_eq!(
        shown_after(&WindowEvent::Focused(true), || panic!(
            "focus alone means it shows"
        )),
        Some((true, Tell::Always))
    );
    assert_eq!(shown_after(&WindowEvent::Destroyed, restored), None);
}
