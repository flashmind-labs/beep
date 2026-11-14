//! Notices as macOS notifications. A click opens what a notice is about, a
//! coding-agent session or Home, in the command center; an agent's question
//! also has Allow and Deny.
// Off macOS a notice is plain text, so the click and answer paths go unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::{
    center::{self, AgentTarget, CenterTarget},
    error::{DesktopError, Result},
    fleet::{Fleet, alerts},
    notices::history,
    tray::waiting::WaitingList,
};
use beep_proto::{AgentNoticeKey, ApiError, Notice};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;

/// A notification with nothing to answer or open.
pub fn text(app: &AppHandle, title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    mac::text(app, title, body);
    #[cfg(not(target_os = "macos"))]
    let shown = app.notification().builder().title(title).body(body).show();

    #[cfg(not(target_os = "macos"))]
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

/// Shows `notice`, which opens its session when clicked and, when it asks a
/// question, answers it from Allow or Deny.
/// A reviewer's opens beside the diff it reviewed.
pub fn notice(app: &AppHandle, notice: &Notice, reviewer: bool) {
    let target = match click_target(notice) {
        CenterTarget::Agent(target) if reviewer => CenterTarget::Agent(target.reviewing()),
        target => target,
    };
    show(app, notice, target);
}

/// A notification with nothing to answer that opens `target` when clicked.
pub(crate) fn opening(app: &AppHandle, title: &str, body: &str, target: CenterTarget) {
    let notice = Notice {
        title: title.to_owned(),
        body: body.to_owned(),
        session_id: String::new(),
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
    match notice.session_id.parse::<AgentNoticeKey>() {
        Ok(key) => CenterTarget::Agent(AgentTarget::notified(&key)),
        Err(_) => CenterTarget::Home,
    }
}

/// The coding-agent question `notice` asks, if it asks one.
fn ask(notice: &Notice) -> Option<AgentNoticeKey> {
    let key = notice.session_id.parse::<AgentNoticeKey>().ok()?;
    key.question.is_some().then_some(key)
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

/// Answers `key` from a notification's button. A failure shows as another
/// notification, since nothing else is on screen; one already answered
/// elsewhere only goes to the log.
async fn answer(app: AppHandle, key: AgentNoticeKey, allow: bool) {
    match answer_agent(&app, key.clone(), allow).await {
        Ok(()) => history::answered(&app, &key),
        Err(DesktopError::Api(ApiError::Conflict { .. } | ApiError::NotFound {})) => {
            history::answered(&app, &key);
            log::info!("a notification answered a question that no longer waits");
        }
        Err(e) => {
            log::warn!("answering from a notification: {e}");
            text(&app, "Beep could not send your answer", &e.to_string());
        }
    }
}

/// Answers the question `key` names on the machine of the mesh that asked
/// it, as Allow or Deny on its notification do. The machine refuses an
/// answer to a question the session no longer asks, as a conflict.
pub(crate) async fn answer_agent(app: &AppHandle, key: AgentNoticeKey, allow: bool) -> Result<()> {
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let answered = if key.question.is_none() {
        Ok(())
    } else if alerts::reaches(&fleet, &key.machine_id) {
        alerts::answer(&fleet, &key, agent_reply(allow)).await
    } else {
        Err(DesktopError::NotInMesh)
    };
    app.state::<WaitingList>().poke();
    answered
}

/// The reply that picks the agent's allow-once or reject-once option.
fn agent_reply(allow: bool) -> &'static str {
    if allow { "allow" } else { "deny" }
}

/// Stored with the notification, so clicks need no waiting thread or in-memory slot.
#[derive(serde::Serialize, serde::Deserialize)]
struct Route {
    target: Option<CenterTarget>,
    question: Option<String>,
}

#[derive(Clone, Copy)]
enum Interaction {
    Open,
    Allow,
    Deny,
    Ignore,
}

#[derive(Debug, PartialEq)]
enum Response {
    Open(CenterTarget),
    Answer(AgentNoticeKey, bool),
    Nothing,
}

fn response(route: Route, interaction: Interaction) -> Response {
    match interaction {
        Interaction::Open => route.target.map_or(Response::Nothing, Response::Open),
        Interaction::Allow | Interaction::Deny => {
            let question = route
                .question
                .and_then(|key| key.parse::<AgentNoticeKey>().ok())
                .filter(|key| key.question.is_some());
            question.map_or(Response::Nothing, |key| {
                Response::Answer(key, matches!(interaction, Interaction::Allow))
            })
        }
        Interaction::Ignore => Response::Nothing,
    }
}

#[cfg(target_os = "macos")]
pub(crate) use mac::init;

#[cfg(target_os = "macos")]
#[allow(deprecated)] // Keep the existing notification presentation and development identity.
mod mac {
    use super::{Interaction, Response, Route, answer, ask, notification_icon, open, response};
    use crate::center::CenterTarget;
    use beep_proto::Notice;
    use objc2::{
        AnyThread, DefinedClass, define_class, msg_send,
        rc::Retained,
        runtime::{AnyObject, ProtocolObject},
    };
    use objc2_app_kit::NSImage;
    use objc2_foundation::{
        MainThreadMarker, NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString,
        NSUserNotification, NSUserNotificationActivationType, NSUserNotificationCenter,
        NSUserNotificationCenterDelegate, ns_string,
    };
    use std::{cell::OnceCell, sync::Once};
    use tauri::AppHandle;
    use uuid::Uuid;

    thread_local! {
        // Notification Center does not retain its delegate. The main thread owns it for the app's lifetime.
        static DELEGATE: OnceCell<Retained<Delegate>> = const { OnceCell::new() };
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements. The immutable AppHandle ivar is thread-safe.
        #[unsafe(super = NSObject)]
        #[thread_kind = AnyThread]
        #[ivars = AppHandle]
        struct Delegate;

        // SAFETY: NSObjectProtocol has no additional requirements.
        unsafe impl NSObjectProtocol for Delegate {}

        // SAFETY: Methods use the native delegate signatures and only borrow their notification arguments.
        unsafe impl NSUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:didActivateNotification:))]
            fn activated(&self, center: &NSUserNotificationCenter, notification: &NSUserNotification) {
                let interaction = match notification.activationType() {
                    NSUserNotificationActivationType::ContentsClicked => Interaction::Open,
                    NSUserNotificationActivationType::ActionButtonClicked => Interaction::Allow,
                    _ => Interaction::Ignore,
                };
                self.handle(notification, interaction);
                center.removeDeliveredNotification(notification);
            }
        }

        impl Delegate {
            // SAFETY: This is the same dismissal selector and signature used by mac-notification-sys.
            #[unsafe(method(userNotificationCenter:didDismissAlert:))]
            fn dismissed(&self, center: &NSUserNotificationCenter, notification: &NSUserNotification) {
                self.handle(notification, Interaction::Deny);
                center.removeDeliveredNotification(notification);
            }
        }
    );

    impl Delegate {
        fn new(app: AppHandle) -> Retained<Self> {
            let this = Self::alloc().set_ivars(app);
            // SAFETY: The superclass is NSObject and its init has no extra arguments or preconditions.
            unsafe { msg_send![super(this), init] }
        }

        fn handle(&self, notification: &NSUserNotification, interaction: Interaction) {
            let Some(route) = notification
                .userInfo()
                .and_then(|info| info.objectForKey(ns_string!("beep-route")))
                .and_then(|value| value.downcast::<NSString>().ok())
                .and_then(|value| serde_json::from_str::<Route>(&value.to_string()).ok())
            else {
                return;
            };

            match response(route, interaction) {
                Response::Open(target) => open(self.ivars(), target),
                Response::Answer(key, allow) => {
                    tauri::async_runtime::spawn(answer(self.ivars().clone(), key, allow));
                }
                Response::Nothing => {}
            }
        }
    }

    pub(crate) fn init(app: &AppHandle) {
        let Some(_) = MainThreadMarker::new() else {
            log::warn!("notification setup needs the main thread");
            return;
        };
        sender(app);
        DELEGATE.with(|held| {
            let delegate = held.get_or_init(|| Delegate::new(app.clone()));
            // SAFETY: DELEGATE retains the delegate for the main thread's entire lifetime.
            unsafe {
                NSUserNotificationCenter::defaultUserNotificationCenter()
                    .setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
            }
        });
    }

    pub(super) fn show(app: &AppHandle, notice: &Notice, target: CenterTarget) {
        send(
            app,
            &notice.title,
            &notice.body,
            Route {
                target: Some(target),
                question: ask(notice).map(|key| key.to_string()),
            },
        );
    }

    pub(super) fn text(app: &AppHandle, title: &str, body: &str) {
        send(
            app,
            title,
            body,
            Route {
                target: None,
                question: None,
            },
        );
    }

    fn send(app: &AppHandle, title: &str, body: &str, route: Route) {
        let title = title.to_owned();
        let body = body.to_owned();
        let handle = app.clone();
        let sent = app.run_on_main_thread(move || {
            init(&handle);
            match notification(&title, &body, &route, notification_icon(&handle).as_deref()) {
                Ok(notification) => NSUserNotificationCenter::defaultUserNotificationCenter()
                    .deliverNotification(&notification),
                Err(error) => log::warn!("encoding a notification target: {error}"),
            }
        });

        if let Err(error) = sent {
            log::warn!("showing a notice: {error}");
        }
    }

    pub(super) fn notification(
        title: &str,
        body: &str,
        route: &Route,
        icon: Option<&str>,
    ) -> serde_json::Result<Retained<NSUserNotification>> {
        let encoded = serde_json::to_string(route)?;
        let notification = NSUserNotification::new();
        notification.setIdentifier(Some(&NSString::from_str(&Uuid::new_v4().to_string())));
        notification.setTitle(Some(&NSString::from_str(title)));
        notification.setInformativeText(Some(&NSString::from_str(body)));
        let value = NSString::from_str(&encoded);
        let info: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[ns_string!("beep-route")], &[&*value]);
        // SAFETY: Keys are NSString and values are property-list NSString objects.
        unsafe {
            notification.setUserInfo(Some(&info));
        }

        notification.setHasActionButton(route.question.is_some());

        if route.question.is_some() {
            notification.setActionButtonTitle(ns_string!("Allow"));
            notification.setOtherButtonTitle(ns_string!("Deny"));
            // SAFETY: The existing notification crate uses this KVC key with an NSNumber boolean.
            unsafe {
                let _: () = msg_send![&notification, setValue: &*NSNumber::numberWithBool(true), forKey: ns_string!("_showsButtons")];
            }
        }

        if let Some(icon) = icon.and_then(|path| {
            NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path))
        }) {
            // SAFETY: The existing notification crate uses these KVC keys with NSImage and NSNumber values.
            unsafe {
                let _: () = msg_send![&notification, setValue: &*icon, forKey: ns_string!("_identityImage")];
                let _: () = msg_send![&notification, setValue: &*NSNumber::numberWithBool(false), forKey: ns_string!("_identityImageHasBorder")];
            }
        }

        Ok(notification)
    }

    // Keep the identity hook for unbundled development builds; it installs no notification timers.
    fn sender(app: &AppHandle) {
        static SET: Once = Once::new();
        SET.call_once(|| {
            let identifier = app.config().identifier.clone();
            let id = if tauri::is_dev() {
                "com.apple.Terminal"
            } else {
                &identifier
            };

            if let Err(error) = mac_notification_sys::set_application(id) {
                log::debug!("naming the app notifications come from: {error}");
            }
        });
    }
}

#[cfg(test)]
#[path = "../tests/notices_notify.rs"]
mod tests;
