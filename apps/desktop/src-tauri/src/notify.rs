//! Notices as macOS notifications. A click opens what a notice is about,
//! its chat or group, in the command center; one that asks for approval
//! also has Allow and Deny.
// Off macOS a notice is plain text, so the click and answer paths go unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::{
    beep::{Beep, Decision},
    center::{self, CenterTarget},
    commands::{self, ChatAnswer},
    error::{DesktopError, Result},
    waiting::WaitingList,
    work,
};
use beep_proto::{AnswerGroupApproval, ApiError, Notice};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

/// The buttons on a notification that asks.
const ALLOW: &str = "Allow";
const DENY: &str = "Deny";

/// A notification with nothing to answer or open.
pub fn text(app: &AppHandle, title: &str, body: &str) {
    let shown = app.notification().builder().title(title).body(body).show();
    if let Err(e) = shown {
        log::warn!("showing a notice: {e}");
    }
}

/// Shows `notice`, which opens its chat or group when clicked and, when it
/// asks for approval, answers it from Allow or Deny.
pub fn notice(app: &AppHandle, notice: &Notice) {
    #[cfg(target_os = "macos")]
    mac::show(app, notice);
    #[cfg(not(target_os = "macos"))]
    text(app, &notice.title, &notice.body);
}

/// What a click on `notice` opens.
fn click_target(notice: &Notice) -> CenterTarget {
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
}

/// The question `notice` asks, if it asks one.
fn ask(notice: &Notice) -> Option<Ask> {
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
    let answered = send_answer(&app, ask, allow).await;
    app.state::<WaitingList>().poke();
    match answered {
        Ok(()) => {}
        Err(DesktopError::Api(ApiError::Conflict { .. } | ApiError::NotFound {})) => {
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
    match ask {
        Ask::Turn { session, query_id } => {
            let answer = ChatAnswer {
                session,
                query_id,
                decision,
            };
            commands::answer_chat(app, answer, None).await
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
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{ALLOW, DENY, answer, ask, click_target, open, text};
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
            WAITING
                .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    (n < WAITERS).then_some(n + 1)
                })
                .ok()
                .map(|_| Self)
        }
    }

    impl Drop for Slot {
        fn drop(&mut self) {
            WAITING.fetch_sub(1, Ordering::AcqRel);
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

    pub(super) fn show(app: &AppHandle, notice: &Notice) {
        let Some(slot) = Slot::take() else {
            return text(app, &notice.title, &notice.body);
        };
        let target = click_target(notice);
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
            text(app, &notice.title, &notice.body);
        }
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
        let mut notification = Notification::new();
        notification.title(title).message(body).wait_for_click(true);
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
mod tests {
    use super::*;
    use beep_proto::{Approval, PendingApproval};

    fn sample(session: &str, run: Option<&str>, group: bool) -> Notice {
        Notice {
            title: "Allow Fastmail: send email?".into(),
            body: "To Sam".into(),
            session_id: session.into(),
            app: None,
            approval: run.map(|run| PendingApproval {
                run_id: run.into(),
                query_id: "q1".into(),
                approval: Approval {
                    app: "Fastmail".into(),
                    action: "send email".into(),
                    fields: Vec::new(),
                },
            }),
            changed: false,
            group,
        }
    }

    #[test]
    fn a_click_opens_the_chat_or_group() {
        let id = Uuid::new_v4();
        assert_eq!(
            click_target(&sample(&id.to_string(), None, false)),
            CenterTarget::Chat { session: Some(id) }
        );
        assert_eq!(
            click_target(&sample("g1", None, true)),
            CenterTarget::Group { id: "g1".into() }
        );
        let mut lost = sample("", None, false);
        lost.app = Some("gmail".into());
        assert_eq!(click_target(&lost), CenterTarget::Apps);
        assert_eq!(click_target(&sample("", None, false)), CenterTarget::Home);
    }

    #[test]
    fn approvals_ask_whoever_waits_on_them() {
        let id = Uuid::new_v4().to_string();
        assert_eq!(ask(&sample(&id, None, false)), None);
        assert_eq!(
            ask(&sample(&id, Some("run1"), false)),
            Some(Ask::Turn {
                session: id.clone(),
                query_id: "q1".into()
            })
        );
        assert_eq!(
            ask(&sample(&id, Some(""), false)),
            Some(Ask::Work {
                session: id.clone(),
                query_id: "q1".into()
            })
        );
        assert_eq!(
            ask(&sample("g1", Some(""), true)),
            Some(Ask::Group {
                group: "g1".into(),
                query_id: "q1".into()
            })
        );
        let mut resolved = sample(&id, Some(""), false);
        resolved.changed = true;
        assert_eq!(ask(&resolved), None);
    }
}
