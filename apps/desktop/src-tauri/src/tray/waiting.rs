//! What waits on the user, for the tray menu: the questions coding agents
//! ask, as Home lists them under Needs you.

use crate::{
    agents::sessions::machine_sessions,
    center::{self, AgentTarget, CenterTarget, NeedsYou},
    fleet::Fleet,
    machines::list::MachineLists,
    notices::history,
};
use beep_proto::SessionId;
use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tokio::{
    sync::Notify,
    time::{sleep, timeout},
};

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
/// How often the list is fetched when nothing poked, at first and after
/// a change; each fetch that changes nothing doubles the wait up to
/// `QUIET`.
const EVERY: Duration = Duration::from_secs(60);
const QUIET: Duration = Duration::from_secs(5 * 60);
/// The least time between two fetches, so a burst of notices makes one.
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
    wake: Notify,
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

    /// Asks for a fetch soon. Pokes that come during one make one more.
    pub fn poke(&self) {
        self.wake.notify_one();
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

/// Keeps the list current while Beep runs: on each poke, and every `EVERY`
/// to `QUIET`.
/// Machines poke on every question or finished turn they send, and when
/// they connect; the slow look catches an entry answered elsewhere and a
/// session that ended with no alert.
pub async fn follow(app: AppHandle) {
    let list = app.state::<WaitingList>();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    list.poke();
    let mut wait = EVERY;

    loop {
        // Timing out is the usual way round: nothing poked.
        let _ = timeout(wait, list.wake.notified()).await;
        wait = (wait * 2).min(QUIET);
        let entries = fetch(&app, &fleet).await;
        let gone: Vec<String> = list
            .entries()
            .into_iter()
            .map(|e| e.id)
            .filter(|id| !entries.iter().any(|e| &e.id == id))
            .collect();

        if list.set(entries) {
            wait = EVERY;
            crate::tray::refresh(&app);
            history::no_longer_waiting(&app, &gone);
        }

        sleep(GAP).await;
    }
}

async fn fetch(app: &AppHandle, fleet: &Arc<Fleet>) -> Vec<Entry> {
    let sessions = machine_sessions(app, fleet, &app.state::<MachineLists>()).await;
    entries(center::needs_you(sessions.iter()))
}

#[cfg(test)]
#[path = "../tests/tray_waiting.rs"]
mod tests;
