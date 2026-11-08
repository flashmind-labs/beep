use super::*;
use crate::center::window::{LABEL, project_label};
use uuid::Uuid;

fn key(session: &str, question: Option<u32>) -> AgentNoticeKey {
    AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "claude".into(),
        session: session.into(),
        question,
        machine_name: "build-box".into(),
    }
}

#[test]
fn an_agent_session_counts_as_shown_while_its_view_reads_it() {
    let shown = AgentsShown::default();
    let at = Instant::now();
    shown.read("center", "m1", "claude", "s1", at);
    let s1 = key("s1", None);
    assert!(shown.shows(&["center"], &s1, at + Duration::from_secs(5)));
    assert!(
        !shown.shows(&["center"], &s1, at + AGENT_SHOWN_FOR),
        "a view that stopped reading has moved on"
    );
    assert!(
        !shown.shows(&["bar"], &s1, at),
        "only where it is on screen"
    );
    assert!(!shown.shows(&["center"], &key("s2", None), at));
    shown.clear("center");
    assert!(!shown.shows(&["center"], &s1, at));
}

/// The bar's current chat stands in as one more window of `Shown`:
/// the real one lives in `Beep`, which needs a network node to build.
const BAR: &str = "bar";

fn bar_on(shown: &Shown, session: &str) {
    shown.window(BAR).set_session(Some(session.into()));
}

#[test]
fn closing_the_window_forgets_its_chat() {
    let shown = Shown::default();
    shown.window(LABEL).set_session(Some("a".into()));
    shown.window(LABEL).set_group(Some("g".into()));
    shown.clear(LABEL);
    assert_eq!(shown.get(LABEL), Showing::default());
}

#[test]
fn closing_one_window_keeps_the_others_chats() {
    let shown = Shown::default();
    let project = project_label("site");
    shown.window(LABEL).set_session(Some("a".into()));
    shown.window(&project).set_session(Some("b".into()));
    shown.clear(&project);
    assert_eq!(shown.get(&project), Showing::default());
    assert_eq!(shown.get(LABEL).session.as_deref(), Some("a"));
}

#[test]
fn leaving_a_group_clears_only_that_group() {
    let shown = Shown::default();
    let project = project_label("site");
    shown.window(LABEL).set_group(Some("g".into()));
    shown.window(&project).set_group(Some("g".into()));
    shown.leave_group("other");
    assert_eq!(shown.get(LABEL).group.as_deref(), Some("g"));
    shown.leave_group("g");
    assert_eq!(shown.get(LABEL).group, None);
    assert_eq!(shown.get(&project).group, None);
}

#[test]
fn a_deleted_chat_leaves_every_window() {
    let shown = Shown::default();
    let project = project_label("site");
    shown.window(LABEL).set_session(Some("a".into()));
    shown.window(&project).set_session(Some("a".into()));
    bar_on(&shown, "b");
    shown.forget_session("a");
    assert_eq!(shown.get(LABEL).session, None);
    assert_eq!(shown.get(&project).session, None);
    assert_eq!(shown.get(BAR).session.as_deref(), Some("b"));
}

#[test]
fn center_label_leaves_bar_chat() {
    let shown = Shown::default();
    bar_on(&shown, "a");
    remember_in(
        Holder::Center,
        &shown.window(BAR),
        &shown.window(LABEL),
        Chat::Session(Some("b".into())),
    );
    assert_eq!(shown.get(BAR).session.as_deref(), Some("a"));
    assert_eq!(shown.get(LABEL).session.as_deref(), Some("b"));
}

#[test]
fn each_window_keeps_its_own_chat() {
    let shown = Shown::default();
    let project = project_label("site");
    let bar = shown.window(BAR);
    remember_in(
        Holder::Center,
        &bar,
        &shown.window(LABEL),
        Chat::Session(Some("a".into())),
    );
    remember_in(
        Holder::Center,
        &bar,
        &shown.window(&project),
        Chat::Session(Some("b".into())),
    );
    assert_eq!(shown.get(LABEL).session.as_deref(), Some("a"));
    assert_eq!(shown.get(&project).session.as_deref(), Some("b"));
}

#[test]
fn bar_label_moves_bar_chat() {
    let shown = Shown::default();
    bar_on(&shown, "a");
    remember_in(
        Holder::Bar,
        &shown.window(BAR),
        &shown.window(LABEL),
        Chat::Session(Some("b".into())),
    );
    assert_eq!(shown.get(BAR).session.as_deref(), Some("b"));
    assert_eq!(shown.get(LABEL), Showing::default());
}

#[test]
fn bar_session_leaves_group_and_group_keeps_session() {
    let shown = Shown::default();
    bar_on(&shown, "a");
    let bar = shown.window(BAR);
    let window = shown.window(LABEL);
    remember_in(Holder::Bar, &bar, &window, Chat::Group(Some("g".into())));
    assert_eq!(
        shown.get(BAR),
        Showing {
            session: Some("a".into()),
            group: Some("g".into()),
        }
    );
    remember_in(Holder::Bar, &bar, &window, Chat::Group(None));
    assert_eq!(shown.get(BAR).session.as_deref(), Some("a"));
    remember_in(Holder::Bar, &bar, &window, Chat::Session(None));
    assert_eq!(shown.get(BAR), Showing::default());
}

#[test]
fn center_group_replaces_its_session() {
    let shown = Shown::default();
    let bar = shown.window(BAR);
    let window = shown.window(LABEL);
    remember_in(
        Holder::Center,
        &bar,
        &window,
        Chat::Session(Some("a".into())),
    );
    remember_in(Holder::Center, &bar, &window, Chat::Group(Some("g".into())));
    assert_eq!(
        shown.get(LABEL),
        Showing {
            session: None,
            group: Some("g".into()),
        }
    );
    remember_in(Holder::Center, &bar, &window, Chat::Group(None));
    assert_eq!(shown.get(LABEL), Showing::default());
    assert_eq!(shown.get(BAR), Showing::default());
}

#[test]
fn center_send_leaves_bar_session() {
    let shown = Shown::default();
    bar_on(&shown, "a");
    let sent_to = Uuid::new_v4().to_string();
    remember_in(
        Holder::Center,
        &shown.window(BAR),
        &shown.window(LABEL),
        Chat::Session(Some(sent_to.clone())),
    );
    assert_eq!(shown.get(BAR).session.as_deref(), Some("a"));
    assert_eq!(shown.get(LABEL).session, Some(sent_to));
}

#[test]
fn holder_reads_labels() {
    assert_eq!(holder("center"), Holder::Center);
    assert_eq!(holder(&project_label("site")), Holder::Center);
    assert_eq!(holder("bar"), Holder::Bar);
    assert_eq!(holder("research-1"), Holder::Bar);
}

#[test]
fn on_screen_in_either_place() {
    let bar = Seen {
        bar: Some("a".into()),
        window: None,
    };
    assert!(on_screen(&bar, "a"));
    assert!(!on_screen(&bar, "b"));
    let window = Seen {
        bar: None,
        window: Some("b".into()),
    };
    assert!(on_screen(&window, "b"));
    assert!(!on_screen(&window, "a"));
    let nowhere = Seen::default();
    assert!(!on_screen(&nowhere, "a"));
    assert!(!on_screen(&nowhere, "b"));
}
