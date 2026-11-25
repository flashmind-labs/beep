//! What waits on the user, for the tray menu: the questions coding agents
//! ask, as Home lists them under Needs you.

use crate::{
    agents::sessions::overlaid,
    center::{self, AgentTarget, CenterTarget, NeedsYou},
    fleet::{Fleet, home_signal::HomeSignal, session_mirror::SessionMirror},
    notices::history,
};
use beep_proto::SessionId;
use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tokio::{select, time::sleep};

/// Menu ids of Waiting's items start with this.
pub const ITEM: &str = "wait:";
/// The menu id of the item that opens Home for the rest.
#[cfg_attr(mobile, allow(dead_code))]
pub const MORE: &str = "wait-more";
/// Items the menu lists; Home has the rest.
#[cfg_attr(mobile, allow(dead_code))]
pub const SHOWN: usize = 8;
/// The longest label, in characters, so the menu keeps its width.
const LABEL_MAX: usize = 60;
/// The least time between two reads of the mirror, so a burst of changes makes one.
const GAP: Duration = Duration::from_secs(2);

/// One thing that waits, as the tray lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Its menu id, `ITEM` and then what names the question.
    pub id: String,
    pub label: String,
    /// What a click opens: its agent session.
    pub target: CenterTarget,
}

/// The latest list, kept so the tray menu can be built without waiting on
/// the machines.
#[derive(Default)]
pub struct WaitingList {
    entries: Mutex<Vec<Entry>>,
}

impl WaitingList {
    fn slot(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.slot().clone()
    }

    /// What the item with menu id `id` opens, while it still waits.
    #[cfg_attr(mobile, allow(dead_code))]
    pub fn target(&self, id: &str) -> Option<CenterTarget> {
        self.slot()
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.target.clone())
    }

    /// Replaces the list; true when it changed.
    fn set(&self, entries: Vec<Entry>) -> bool {
        let mut slot = self.slot();

        if *slot == entries {
            return false;
        }
        *slot = entries;
        true
    }
}

/// The tray's entries for what Home lists under Needs you, in its order.
pub fn entries(needs: Vec<NeedsYou>) -> Vec<Entry> {
    needs
        .into_iter()
        .map(|need| match need {
            NeedsYou::Agent {
                machine_id,
                machine_name,
                agent,
                session,
                title,
                question,
            } => Entry {
                id: agent_item(&machine_id, &session, question.id),
                label: shorten(&question.text),
                target: CenterTarget::Agent(AgentTarget::asking(
                    &machine_id,
                    &machine_name,
                    &agent,
                    &session,
                    title.as_deref(),
                )),
            },
        })
        .collect()
}

/// The menu id of `question` of `session` on `machine_id`.
pub(crate) fn agent_item(machine_id: &str, session: &SessionId, question: u32) -> String {
    format!("{ITEM}agent:{machine_id}:{session}:{question}")
}

/// What the tray listed as waiting and no longer does, by its menu ids:
/// those questions were answered, here or anywhere.
pub(crate) fn no_longer_waiting(app: &AppHandle, gone: &[String]) {
    if gone.is_empty() {
        return;
    }

    history::finish(app, |note| {
        history::asked(note).is_some_and(|key| {
            key.question.is_some_and(|question| {
                gone.contains(&agent_item(&key.machine_id, &key.session, question))
            })
        })
    });
}

/// `text` on one line, cut to `LABEL_MAX` characters.
fn shorten(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if line.chars().count() <= LABEL_MAX {
        return line;
    }

    let mut cut: String = line.chars().take(LABEL_MAX - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// The count beside the tray icon while anything waits.
#[cfg_attr(mobile, allow(dead_code))]
pub fn badge(count: usize) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

#[cfg_attr(mobile, allow(dead_code))]
pub fn tooltip(count: usize) -> String {
    match count {
        0 => "Beep".into(),
        1 => "Beep: 1 waits on you".into(),
        n => format!("Beep: {n} wait on you"),
    }
}

/// Keeps the list current while Beep runs: it follows the mirror's rows and the signal Home
/// follows, and reads the rows as they stand. Alerts and pushes reach the rows, so nothing asks
/// the machines. On a desktop it holds an interest in the `agents` rows, which no window pauses,
/// so the nodes keep pushing them while every window is hidden.
pub async fn follow(app: AppHandle) {
    let list = app.state::<WaitingList>();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let mirror = app.state::<Arc<SessionMirror>>();
    let mut rows = mirror.watch();
    let mut pokes = app.state::<Arc<HomeSignal>>().follow();
    #[cfg(desktop)]
    let events = app.state::<crate::fleet::events::Events>();
    #[cfg(desktop)]
    let _standing = events
        .hold(
            "tray",
            vec![crate::fleet::events::Interest {
                machine: None,
                topic: beep_proto::node::events::Topic::new("agents"),
            }],
        )
        .inspect_err(|error| log::warn!("the tray cannot follow the sessions: {error}"));
    #[cfg(desktop)]
    events.start(app.clone(), fleet.clone());

    loop {
        let sessions = overlaid(&app, &fleet, &mirror.current());
        let entries = entries(center::needs_you(sessions.iter()));
        let gone: Vec<String> = list
            .entries()
            .into_iter()
            .map(|e| e.id)
            .filter(|id| !entries.iter().any(|e| &e.id == id))
            .collect();

        if list.set(entries) {
            crate::tray::refresh(&app);
            no_longer_waiting(&app, &gone);
        }

        sleep(GAP).await;
        select! {
            changed = rows.changed() => if changed.is_err() { return },
            changed = pokes.changed() => if changed.is_err() { return },
        }
    }
}

#[cfg(test)]
#[path = "../tests/tray_waiting.rs"]
mod tests;
