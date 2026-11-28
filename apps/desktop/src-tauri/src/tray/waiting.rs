//! What waits on the user, for the tray menu: the questions coding agents
//! ask, as Home lists them under Needs you.

use crate::store::lock;
use crate::{
    home::NeedsYou,
    notes,
    targets::{AgentTarget, CenterTarget},
};
use beep_proto::SessionId;
use std::sync::{Mutex, MutexGuard};
use tauri::AppHandle;

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
        lock(&self.entries)
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
    pub(super) fn set(&self, entries: Vec<Entry>) -> bool {
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

    notes::finish(app, |note| {
        notes::asked(note).is_some_and(|key| {
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

#[cfg(test)]
#[path = "../tests/tray_waiting.rs"]
mod tests;
