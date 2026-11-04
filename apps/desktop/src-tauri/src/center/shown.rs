//! Which chat each webview shows: the bar's own, each command center
//! window's in `Shown`, and the agent sessions in `AgentsShown`, so notices
//! for what is on screen stay quiet. Entry points: `remember_for`, `on_screen`.

use super::window::is_center;
use crate::beep::Beep;
use beep_proto::AgentNoticeKey;
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tauri::{Manager, Runtime, WebviewWindow};

/// Which webview a chat command came from. Each keeps its own current chat:
/// the bar's lives in `Beep`, the window's in `Shown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holder {
    Bar,
    Center,
}

/// The holder for a webview label: a command center window, the main one
/// or a project's, or the bar for anything else.
fn holder(label: &str) -> Holder {
    if is_center(label) {
        Holder::Center
    } else {
        Holder::Bar
    }
}

/// What a command center window shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Showing {
    pub(crate) session: Option<String>,
    pub(crate) group: Option<String>,
}

/// Each command center window's current chat, by window label, apart from
/// the bar's.
#[derive(Debug, Default)]
pub(crate) struct Shown(Mutex<HashMap<String, Showing>>);

impl Shown {
    fn windows(&self) -> MutexGuard<'_, HashMap<String, Showing>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Window `label`'s entry, to change its chat.
    pub(crate) fn window<'a>(&'a self, label: &'a str) -> ShownIn<'a> {
        ShownIn { shown: self, label }
    }

    /// What window `label` shows; nothing for a window that never said.
    pub(crate) fn get(&self, label: &str) -> Showing {
        self.windows().get(label).cloned().unwrap_or_default()
    }

    /// Forgets window `label`'s chat and group, when it closes.
    pub(crate) fn clear(&self, label: &str) {
        self.windows().remove(label);
    }

    /// Leaves group `id` in every window that shows it.
    pub(crate) fn leave_group(&self, id: &str) {
        for showing in self.windows().values_mut() {
            if showing.group.as_deref() == Some(id) {
                showing.group = None;
            }
        }
    }

    /// Leaves session `id` in every window that shows it.
    pub(crate) fn forget_session(&self, id: &str) {
        for showing in self.windows().values_mut() {
            if showing.session.as_deref() == Some(id) {
                showing.session = None;
            }
        }
    }
}

/// One window's current chat in `Shown`.
pub(crate) struct ShownIn<'a> {
    shown: &'a Shown,
    label: &'a str,
}

/// A chat a webview opened or left: a session or a group, `None` for none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Chat {
    Session(Option<String>),
    Group(Option<String>),
}

/// Where a holder keeps its current chat.
trait Current {
    fn set_session(&self, session: Option<String>);
    fn set_group(&self, group: Option<String>);
}

impl Current for Beep {
    fn set_session(&self, session: Option<String>) {
        Beep::set_session(self, session);
    }

    fn set_group(&self, group: Option<String>) {
        Beep::set_group(self, group);
    }
}

impl Current for ShownIn<'_> {
    fn set_session(&self, session: Option<String>) {
        let mut windows = self.shown.windows();
        windows.entry(self.label.to_owned()).or_default().session = session;
    }

    fn set_group(&self, group: Option<String>) {
        let mut windows = self.shown.windows();
        windows.entry(self.label.to_owned()).or_default().group = group;
    }
}

/// Makes `chat` the holder's current chat. A session, or none, also leaves
/// the holder's group. In the bar a group keeps the session: the bar leaves
/// a group on every view change and reopens its last chat after. A window
/// keeps no such chat, so a group it opens replaces its session.
/// The holder is the webview a command came from, with that window's entry
/// in `Shown` from the app's state.
pub(crate) fn remember_for<R: Runtime>(window: &WebviewWindow<R>, beep: &Beep, chat: Chat) {
    let shown = window.state::<Shown>();
    let label = window.label();
    remember_in(holder(label), beep, &shown.window(label), chat);
}

fn remember_in(holder: Holder, bar: &impl Current, shown: &ShownIn<'_>, chat: Chat) {
    let current: &dyn Current = match holder {
        Holder::Bar => bar,
        Holder::Center => shown,
    };
    match chat {
        Chat::Session(session) => {
            current.set_session(session);
            current.set_group(None);
        }
        Chat::Group(group) => {
            if holder == Holder::Center && group.is_some() {
                current.set_session(None);
            }
            current.set_group(group);
        }
    }
}

/// The chat on screen in each place: the bar's only while the bar is open,
/// a command center window's only while that window has focus.
#[derive(Debug, Default)]
pub(crate) struct Seen {
    pub(crate) bar: Option<String>,
    pub(crate) window: Option<String>,
}

/// How long after a webview last read an agent session it still counts as
/// shown there. The open session view reads it every 1.5 s while it works
/// and every 10 s once quiet.
const AGENT_SHOWN_FOR: Duration = Duration::from_secs(12);

/// The coding-agent session each webview's session view last read, and
/// when, by webview label.
#[derive(Debug, Default)]
pub(crate) struct AgentsShown(Mutex<HashMap<String, ShownAgent>>);

#[derive(Debug)]
struct ShownAgent {
    machine_id: String,
    agent: String,
    session: String,
    at: Instant,
}

impl AgentsShown {
    fn views(&self) -> MutexGuard<'_, HashMap<String, ShownAgent>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Webview `label` read `session` of `agent` on `machine_id` at `at`.
    pub(crate) fn read(
        &self,
        label: &str,
        machine_id: &str,
        agent: &str,
        session: &str,
        at: Instant,
    ) {
        let shown = ShownAgent {
            machine_id: machine_id.to_owned(),
            agent: agent.to_owned(),
            session: session.to_owned(),
            at,
        };
        self.views().insert(label.to_owned(), shown);
    }

    /// Forgets webview `label`'s session view, when it closes.
    pub(crate) fn clear(&self, label: &str) {
        self.views().remove(label);
    }

    /// Whether one of `labels`, the webviews on screen, read the session
    /// `key` is about lately, as of `now`.
    pub(crate) fn shows(&self, labels: &[&str], key: &AgentNoticeKey, now: Instant) -> bool {
        let views = self.views();
        labels.iter().any(|label| {
            views.get(*label).is_some_and(|shown| {
                key.is_session(&shown.machine_id, &shown.agent, &shown.session)
                    && now.saturating_duration_since(shown.at) < AGENT_SHOWN_FOR
            })
        })
    }
}

/// Whether the chat `id` is on screen in the bar or the window, so its
/// messages need no notice.
pub(crate) fn on_screen(seen: &Seen, id: &str) -> bool {
    seen.bar.as_deref() == Some(id) || seen.window.as_deref() == Some(id)
}

#[cfg(test)]
mod tests {
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
}
