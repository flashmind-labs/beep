//! What waits on the user, for the tray menu: the approvals chats wait on
//! and the questions coding agents ask, as Home lists them under Needs you.

use crate::{
    agents::sessions::machine_sessions,
    beep::Beep,
    center::{self, AgentTarget, CenterTarget, NeedsYou},
    error::{DesktopError, Result},
    fleet::Fleet,
};
use beep_proto::ListPendingApprovals;
use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tokio::{
    join,
    sync::Notify,
    time::{sleep, timeout},
};

/// Menu ids of Waiting's items start with this.
pub const ITEM: &str = "wait:";
/// The menu id of the item that opens Home for the rest.
pub const MORE: &str = "wait-more";
/// Items the menu lists; Home has the rest.
pub const SHOWN: usize = 8;
/// The longest label, in characters, so the menu keeps its width.
const LABEL_MAX: usize = 60;
/// How often the list is fetched when nothing says it changed. Agent
/// questions from older nodes and from sessions started outside Beep come
/// with no notice.
const EVERY: Duration = Duration::from_secs(30);
/// The least time between two fetches, so a burst of notices makes one.
const GAP: Duration = Duration::from_secs(2);

/// One thing that waits, as the tray lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Its menu id, `ITEM` and then what names the question.
    pub id: String,
    pub label: String,
    /// What a click opens: its chat or agent session.
    pub target: CenterTarget,
}

/// The latest list, kept so the tray menu can be built without waiting on
/// the server.
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
            NeedsYou::Chat {
                session_id,
                pending,
                ..
            } => Entry {
                id: format!("{ITEM}chat:{}", pending.query_id),
                label: chat_label(&pending.approval.app, &pending.approval.action),
                target: CenterTarget::Chat {
                    session: Some(session_id),
                },
            },
            NeedsYou::Agent {
                machine_id,
                machine_name,
                agent,
                session,
                title,
                question,
            } => Entry {
                id: format!("{ITEM}agent:{machine_id}:{session}:{}", question.id),
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

/// "Allow Fastmail: send email?", as the approval card asks it.
fn chat_label(app: &str, action: &str) -> String {
    let label = match (app.trim(), action.trim()) {
        ("", "") => "Allow this?".to_owned(),
        ("", action) => format!("Allow {action}?"),
        (app, "") => format!("Allow {app}?"),
        (app, action) => format!("Allow {app}: {action}?"),
    };
    shorten(&label)
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
pub fn badge(count: usize) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

pub fn tooltip(count: usize) -> String {
    match count {
        0 => "Beep".into(),
        1 => "Beep: 1 waits on you".into(),
        n => format!("Beep: {n} wait on you"),
    }
}

/// Keeps the list current while Beep runs: on a poke, and every `EVERY`.
pub async fn follow(app: AppHandle, beep: Arc<Beep>) {
    let list = app.state::<WaitingList>();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    loop {
        // Timing out is the usual way round: nothing poked.
        let _ = timeout(EVERY, list.wake.notified()).await;
        {
            match fetch(&beep, &fleet).await {
                Ok(entries) => {
                    if list.set(entries) {
                        crate::tray::refresh(&app);
                    }
                }
                // The tray keeps what it showed until a fetch works.
                Err(e) => log::warn!("listing what waits for the tray: {e}"),
            }
        }
        sleep(GAP).await;
    }
}

async fn fetch(beep: &Beep, fleet: &Fleet) -> Result<Vec<Entry>> {
    let (sessions, approvals) = join!(
        machine_sessions(beep, fleet),
        beep.call(ListPendingApprovals {}),
    );
    // With no server, or none linked, nothing waits there.
    let approvals = match approvals {
        Err(DesktopError::NoServer | DesktopError::Unlinked | DesktopError::Revoked) => Vec::new(),
        other => supported(other)?,
    };
    // Chats are not listed for their times, so approvals keep the server's
    // order after the agents' questions.
    Ok(entries(center::needs_you(sessions.iter(), approvals, &[])))
}

/// An empty list for a request this server or account cannot make.
fn supported<T>(listed: Result<Vec<T>>) -> Result<Vec<T>> {
    match listed {
        Err(e) if center::is_unsupported(&e) => Ok(Vec::new()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{AgentQuestion, Approval, PendingApproval};
    use uuid::Uuid;

    fn chat(app: &str, action: &str, query: &str) -> NeedsYou {
        NeedsYou::Chat {
            session_id: Uuid::nil(),
            session_title: "Lunch".into(),
            pending: PendingApproval {
                run_id: String::new(),
                query_id: query.into(),
                approval: Approval {
                    app: app.into(),
                    action: action.into(),
                    fields: Vec::new(),
                },
            },
        }
    }

    #[test]
    fn chat_approvals_read_as_the_card_asks() {
        let listed = entries(vec![chat("Fastmail", "send email", "q1")]);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].label, "Allow Fastmail: send email?");
        assert_eq!(listed[0].id, "wait:chat:q1");
        assert_eq!(
            listed[0].target,
            CenterTarget::Chat {
                session: Some(Uuid::nil())
            }
        );
        assert_eq!(chat_label("", ""), "Allow this?");
        assert_eq!(chat_label("Gmail", ""), "Allow Gmail?");
    }

    #[test]
    fn agent_questions_open_their_session() {
        let need = NeedsYou::Agent {
            machine_id: "m1".into(),
            machine_name: "build-box".into(),
            agent: "claude".into(),
            session: "s1".into(),
            title: Some("Fix tests".into()),
            question: AgentQuestion {
                id: 3,
                text: "Run cargo test".into(),
                detail: None,
                options: Vec::new(),
            },
        };
        let listed = entries(vec![need]);
        assert_eq!(listed[0].id, "wait:agent:m1:s1:3");
        assert_eq!(listed[0].label, "Run cargo test");
        assert_eq!(
            listed[0].target,
            CenterTarget::Agent(AgentTarget::asking(
                "m1",
                "build-box",
                "claude",
                "s1",
                Some("Fix tests")
            ))
        );
    }

    #[test]
    fn long_labels_are_cut_to_one_short_line() {
        let long = format!("Run\n{}", "x".repeat(100));
        let label = shorten(&long);
        assert_eq!(label.chars().count(), LABEL_MAX);
        assert!(label.starts_with("Run x"));
        assert!(label.ends_with('…'));
        assert_eq!(shorten("  short   one "), "short one");
    }

    #[test]
    fn badge_shows_only_while_something_waits() {
        assert_eq!(badge(0), None);
        assert_eq!(badge(3), Some("3".into()));
        assert_eq!(tooltip(0), "Beep");
        assert_eq!(tooltip(1), "Beep: 1 waits on you");
        assert_eq!(tooltip(2), "Beep: 2 wait on you");
    }

    #[test]
    fn the_list_says_when_it_changed() {
        let list = WaitingList::default();
        let one = entries(vec![chat("Gmail", "send", "q1")]);
        assert!(list.set(one.clone()));
        assert!(!list.set(one));
        assert!(list.target("wait:chat:q1").is_some());
        assert!(list.target("wait:chat:gone").is_none());
        assert!(list.set(Vec::new()));
    }
}
