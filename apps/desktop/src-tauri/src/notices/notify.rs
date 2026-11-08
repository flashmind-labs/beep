//! Notices as macOS notifications. A click opens what a notice is about,
//! its chat, group or coding-agent session, in the command center; one
//! that asks for approval, or an agent's question, also has Allow and Deny.
// Off macOS a notice is plain text, so the click and answer paths go unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::{
    beep::{Beep, Decision},
    center::{self, AgentTarget, CenterTarget},
    chat::approvals::{self, ChatAnswer},
    error::{DesktopError, Result},
    fleet::{Fleet, alerts},
    notices::{history, work},
    tray::waiting::WaitingList,
};
use beep_proto::{AgentNoticeKey, AnswerAgentQuestion, AnswerGroupApproval, ApiError, Notice};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

/// The buttons on a notification that asks.
const ALLOW: &str = "Allow";
const DENY: &str = "Deny";

/// A notification with nothing to answer or open.
pub fn text(app: &AppHandle, title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    let shown = mac::text(app, title, body);
    #[cfg(not(target_os = "macos"))]
    let shown = app.notification().builder().title(title).body(body).show();
    if let Err(e) = shown {
        log::warn!("showing a notice: {e}");
    }
}

/// Supply the bundled mark instead of relying on the system's cached app icon.
#[cfg(target_os = "macos")]
fn notification_icon(app: &AppHandle) -> Option<String> {
    match app.path().resolve(
        "notification-icon.png",
        tauri::path::BaseDirectory::Resource,
    ) {
        Ok(path) => Some(path.to_string_lossy().into_owned()),
        Err(e) => {
            log::warn!("resolving the notification icon: {e}");
            None
        }
    }
}

/// Shows `notice`, which opens its chat or group when clicked and, when it
/// asks for approval, answers it from Allow or Deny.
pub fn notice(app: &AppHandle, notice: &Notice) {
    show(app, notice, click_target(notice));
}

/// Shows `notice`, an alert the assistant raised on its own, which opens it
/// in the notifications pane when clicked.
pub fn alert(app: &AppHandle, notice: &Notice) {
    let session = Uuid::parse_str(&notice.session_id).ok();
    show(app, notice, CenterTarget::Notice { session });
}

/// A notification with nothing to answer that opens `target` when clicked.
pub(crate) fn opening(app: &AppHandle, title: &str, body: &str, target: CenterTarget) {
    let notice = Notice {
        title: title.to_owned(),
        body: body.to_owned(),
        session_id: String::new(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    };
    show(app, &notice, target);
}

fn show(app: &AppHandle, notice: &Notice, target: CenterTarget) {
    #[cfg(target_os = "macos")]
    mac::show(app, notice, target);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        text(app, &notice.title, &notice.body);
    }
}

/// What a click on `notice` opens.
fn click_target(notice: &Notice) -> CenterTarget {
    if let Ok(key) = notice.session_id.parse::<AgentNoticeKey>() {
        return CenterTarget::Agent(AgentTarget::notified(&key));
    }
    if notice.group {
        return CenterTarget::Group {
            id: notice.session_id.clone(),
        };
    }
    if let Ok(session) = Uuid::parse_str(&notice.session_id) {
        return CenterTarget::Chat {
            session: Some(session),
        };
    }
    // An app that lost its login: Apps is where it signs in again.
    if notice.app.is_some() {
        return CenterTarget::Apps;
    }
    CenterTarget::Home
}

/// What Allow and Deny on a notice answer.
#[derive(Debug, Clone, PartialEq)]
enum Ask {
    /// A turn in chat `session` waits on it.
    Turn { session: String, query_id: String },
    /// Background work in chat `session` asked it.
    Work { session: String, query_id: String },
    /// Someone in group `group` asked Beep for it.
    Group { group: String, query_id: String },
    /// A coding-agent session asks it, as the key names.
    Agent(AgentNoticeKey),
}

/// The question `notice` asks, if it asks one.
fn ask(notice: &Notice) -> Option<Ask> {
    if let Ok(key) = notice.session_id.parse::<AgentNoticeKey>() {
        return key.question.is_some().then_some(Ask::Agent(key));
    }
    let pending = notice.approval.as_ref()?;
    // A resolved question is news that it stopped waiting, not a question.
    if notice.changed {
        return None;
    }
    let query_id = pending.query_id.clone();
    let session = notice.session_id.clone();
    Some(if notice.group {
        Ask::Group {
            group: session,
            query_id,
        }
    } else if pending.run_id.is_empty() {
        Ask::Work { session, query_id }
    } else {
        Ask::Turn { session, query_id }
    })
}

/// Opens `target` in the command center, from the main thread as the tray
/// does.
fn open(app: &AppHandle, target: CenterTarget) {
    let handle = app.clone();
    let opened = app.run_on_main_thread(move || {
        let pending = handle.state::<center::Pending>();
        if let Err(e) = center::open_on(&handle, &pending, target) {
            log::warn!("opening what a notification is about: {e}");
        }
    });
    if let Err(e) = opened {
        log::warn!("opening what a notification is about: {e}");
    }
}

/// Answers `ask` from a notification's button. A failure shows as another
/// notification, since nothing else is on screen; one already answered
/// elsewhere only goes to the log.
async fn answer(app: AppHandle, ask: Ask, allow: bool) {
    let agent = match &ask {
        Ask::Agent(key) => Some(key.clone()),
        _ => None,
    };
    let answered = send_answer(&app, ask, allow).await;
    app.state::<WaitingList>().poke();
    match answered {
        Ok(()) => {
            if let Some(key) = agent {
                history::answered(&app, &key);
            }
        }
        Err(DesktopError::Api(ApiError::Conflict { .. } | ApiError::NotFound {})) => {
            if let Some(key) = agent {
                history::answered(&app, &key);
            }
            log::info!("a notification answered a question that no longer waits");
        }
        Err(e) => {
            log::warn!("answering from a notification: {e}");
            text(&app, "Beep could not send your answer", &e.to_string());
        }
    }
}

async fn send_answer(app: &AppHandle, ask: Ask, allow: bool) -> Result<()> {
    let decision = if allow {
        Decision::Allow
    } else {
        Decision::Deny
    };
    let beep = app.state::<Arc<Beep>>().inner().clone();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    match ask {
        Ask::Turn { session, query_id } => {
            let answer = ChatAnswer {
                session,
                query_id,
                decision,
            };
            approvals::answer_chat(app, answer, None).await
        }
        // A withdrawn question is fine too: the work stopped or someone
        // answered it, and its resolved notice clears the card.
        Ask::Work { session, query_id } => work::answer(&beep, &session, query_id, decision)
            .await
            .map(|_| ()),
        Ask::Group { group, query_id } => {
            beep.call(AnswerGroupApproval { query_id, allow }).await?;
            if let Err(e) = app.emit("group-changed", &group) {
                log::warn!("telling the windows a group approval was answered: {e}");
            }
            Ok(())
        }
        // A machine of the mesh is asked directly; any other through the
        // server. Both refuse an answer to a question the session no
        // longer asks, as a conflict.
        Ask::Agent(key) => {
            let Some(question) = key.question else {
                return Ok(());
            };
            if alerts::reaches(&fleet, &key.machine_id) {
                return alerts::answer(&fleet, &key, agent_reply(allow)).await;
            }
            beep.call(AnswerAgentQuestion {
                machine_id: key.machine_id,
                agent: key.agent,
                session: key.session,
                question,
                reply: agent_reply(allow).into(),
            })
            .await
        }
    }
}

/// Answers the question `key` names, as Allow or Deny on its notification
/// do, from the bell's pane.
pub(crate) async fn answer_agent(app: &AppHandle, key: AgentNoticeKey, allow: bool) -> Result<()> {
    let answered = send_answer(app, Ask::Agent(key), allow).await;
    app.state::<WaitingList>().poke();
    answered
}

/// The reply that picks the agent's allow-once or reject-once option.
fn agent_reply(allow: bool) -> &'static str {
    if allow { "allow" } else { "deny" }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{ALLOW, DENY, answer, ask, notification_icon, open};
    use crate::center::CenterTarget;
    use beep_proto::Notice;
    use mac_notification_sys::{
        MainButton, Notification, NotificationResponse, error::NotificationResult,
    };
    use std::{
        sync::{
            Once,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };
    use tauri::AppHandle;

    /// Notifications waiting for a click at once. Each holds a thread until
    /// it is clicked or cleared from Notification Center; past this many a
    /// notice shows without a click.
    const WAITERS: usize = 16;

    static WAITING: AtomicUsize = AtomicUsize::new(0);

    /// One of `WAITERS`, given back when dropped.
    struct Slot;

    impl Slot {
        fn take() -> Option<Self> {
            // Past the cap the slot is handed straight back.
            if WAITING.fetch_add(1, Ordering::Relaxed) >= WAITERS {
                WAITING.fetch_sub(1, Ordering::Relaxed);
                return None;
            }
            Some(Self)
        }
    }

    impl Drop for Slot {
        fn drop(&mut self) {
            WAITING.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// What the user did with a notification.
    enum Reply {
        Open,
        Answer(bool),
        Nothing,
    }

    fn reply(response: NotificationResponse, asks: bool) -> Reply {
        match response {
            NotificationResponse::Click => Reply::Open,
            NotificationResponse::ActionButton(title) if asks && title == ALLOW => {
                Reply::Answer(true)
            }
            NotificationResponse::CloseButton(title) if asks && title == DENY => {
                Reply::Answer(false)
            }
            _ => Reply::Nothing,
        }
    }

    pub(super) fn show(app: &AppHandle, notice: &Notice, target: CenterTarget) {
        let Some(slot) = Slot::take() else {
            return super::text(app, &notice.title, &notice.body);
        };
        let asked = ask(notice);
        let title = notice.title.clone();
        let body = notice.body.clone();
        let handle = app.clone();
        let spawned = thread::Builder::new()
            .name("notification".into())
            .spawn(move || {
                let _slot = slot;
                let asks = asked.is_some();
                let response = match deliver(&handle, &title, &body, asks) {
                    Ok(response) => response,
                    Err(e) => {
                        log::warn!("showing a notice: {e}");
                        return;
                    }
                };
                match (reply(response, asks), asked) {
                    (Reply::Open, _) => open(&handle, target),
                    (Reply::Answer(allow), Some(ask)) => {
                        tauri::async_runtime::spawn(answer(handle, ask, allow));
                    }
                    _ => {}
                }
            });
        if let Err(e) = spawned {
            log::warn!("waiting on a notification: {e}");
            super::text(app, &notice.title, &notice.body);
        }
    }

    pub(super) fn text(app: &AppHandle, title: &str, body: &str) -> NotificationResult<()> {
        sender(app);
        let icon = notification_icon(app);
        notification(title, body, icon.as_deref())
            .asynchronous(true)
            .send()
            .map(|_| ())
    }

    fn notification<'a>(title: &'a str, body: &'a str, icon: Option<&'a str>) -> Notification<'a> {
        let mut notification = Notification::new();
        notification.title(title).message(body);
        if let Some(icon) = icon {
            notification.app_icon(icon);
        }
        notification
    }

    /// Shows a notification and blocks until it is clicked, answered or
    /// cleared.
    fn deliver(
        app: &AppHandle,
        title: &str,
        body: &str,
        asks: bool,
    ) -> NotificationResult<NotificationResponse> {
        sender(app);
        let icon = notification_icon(app);
        let mut notification = notification(title, body, icon.as_deref());
        notification.wait_for_click(true);
        if asks {
            notification
                .main_button(MainButton::SingleAction(ALLOW))
                .close_button(DENY);
        }
        notification.send()
    }

    /// Notifications come from Beep, as the notification plugin's do;
    /// from Terminal in development, where Beep has no bundle.
    fn sender(app: &AppHandle) {
        static SET: Once = Once::new();
        SET.call_once(|| {
            let identifier = app.config().identifier.clone();
            let id = if tauri::is_dev() {
                "com.apple.Terminal"
            } else {
                identifier.as_str()
            };
            // The plugin may have set the same one first, which is fine.
            if let Err(e) = mac_notification_sys::set_application(id) {
                log::debug!("naming the app notifications come from: {e}");
            }
        });
    }
}

#[cfg(test)]
#[path = "../tests/notices_notify.rs"]
mod tests;
