//! Notices as macOS notifications. A click opens what a notice is about, a
//! coding-agent session or Home, in the command center; an agent's question
//! also has Allow and Deny, and a beep-node release has Update.
// Off macOS a notice is plain text, so the click and answer paths go unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::{
    markdown::plain_text,
    targets::{AgentTarget, CenterTarget},
};
use beep_proto::{
    AgentNoticeKey, Notice,
    node::agent::{AnswerOption, OptionKind},
};
use tauri::{AppHandle, Manager};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;
use tokio::sync::mpsc;

/// Whether the phone app is on screen, where a notice is already in the bell
/// and a system notification only repeats it. Android skips the first resume.
#[cfg(mobile)]
static FOREGROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Notes the phone app going to the background or coming back.
#[cfg(mobile)]
pub(crate) fn set_foreground(on: bool) {
    FOREGROUND.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A notification with nothing to answer or open.
pub fn text(app: &AppHandle, title: &str, body: &str) {
    send(
        app,
        title,
        body,
        Route {
            target: None,
            buttons: None,
        },
    );
}

/// A notification that a beep-node release is out: Update updates the
/// machines behind, and a click opens Machines.
pub(crate) fn update_nodes(app: &AppHandle, title: &str, body: &str) {
    send(
        app,
        title,
        body,
        Route {
            target: Some(CenterTarget::Machines),
            buttons: Some(Buttons::UpdateNodes),
        },
    );
}

/// Shows a notification; off macOS it is plain text, without its click
/// target and buttons.
fn send(app: &AppHandle, title: &str, body: &str, route: Route) {
    #[cfg(mobile)]
    if FOREGROUND.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    #[cfg(target_os = "macos")]
    mac::send(app, title, body, route);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = route;
        let shown = app.notification().builder().title(title).body(body).show();

        if let Err(e) = shown {
            log::warn!("showing a notice: {e}");
        }
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
/// question `allow_deny` says Allow or Deny answers, answers it from them.
/// A reviewer's opens beside the diff it reviewed.
pub fn notice(app: &AppHandle, notice: &Notice, allow_deny: bool, reviewer: bool) {
    let target = match click_target(notice) {
        CenterTarget::Agent(target) if reviewer => CenterTarget::Agent(target.reviewing()),
        target => target,
    };
    let buttons = ask(notice)
        .filter(|_| allow_deny)
        .map(|key| Buttons::Answer(key.to_string()));
    send(
        app,
        &notice.title,
        &plain_text(&notice.body),
        Route {
            target: Some(target),
            buttons,
        },
    );
}

/// Whether Allow and Deny answer a question with `options`, as the machine
/// reads them: one option allows once, one rejects once, and every option is
/// a permission. A choice among answers opens the chat instead.
pub(crate) fn allow_deny(options: &[AnswerOption]) -> bool {
    let count = |kind: OptionKind| options.iter().filter(|o| o.kind == kind).count();

    count(OptionKind::AllowOnce) == 1
        && count(OptionKind::RejectOnce) == 1
        && options.iter().all(|o| o.kind != OptionKind::Other)
}

/// A notification with nothing to answer that opens `target` when clicked.
pub(crate) fn opening(app: &AppHandle, title: &str, body: &str, target: CenterTarget) {
    send(
        app,
        title,
        body,
        Route {
            target: Some(target),
            buttons: None,
        },
    );
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

/// What a click or a button on a notification asks for, which `route::follow_actions` carries out.
pub(crate) enum Action {
    Open(CenterTarget),
    Answer(AgentNoticeKey, bool),
    UpdateNodes,
}

/// Takes the actions to `route::follow_actions`.
pub(crate) struct Actions(mpsc::UnboundedSender<Action>);

impl Actions {
    pub(crate) fn new() -> (Self, mpsc::UnboundedReceiver<Action>) {
        let (sender, actions) = mpsc::unbounded_channel();
        (Self(sender), actions)
    }
}

/// Asks for `action`.
fn ring(app: &AppHandle, action: Action) {
    // The receiver lives as long as the app.
    let _ = app.state::<Actions>().0.send(action);
}

/// The reply that picks the agent's allow-once or reject-once option.
pub(crate) fn agent_reply(allow: bool) -> &'static str {
    if allow { "allow" } else { "deny" }
}

/// Stored with the notification, so clicks need no waiting thread or in-memory slot.
#[derive(serde::Serialize, serde::Deserialize)]
struct Route {
    target: Option<CenterTarget>,
    buttons: Option<Buttons>,
}

/// What a notification's two buttons do.
#[derive(serde::Serialize, serde::Deserialize)]
enum Buttons {
    /// Allow and Deny answer the agent question this key names.
    Answer(String),
    /// Update updates the machines behind on beep-node; Later does nothing.
    UpdateNodes,
}

impl Buttons {
    /// The action button's title and the other one's.
    fn titles(&self) -> (&'static str, &'static str) {
        match self {
            Self::Answer(_) => ("Allow", "Deny"),
            Self::UpdateNodes => ("Update", "Later"),
        }
    }
}

/// What the user did to a notification: clicked it, its action button
/// (Allow, Update) or its other one (Deny, Later, or dismissing it).
#[derive(Clone, Copy)]
enum Interaction {
    Open,
    Action,
    Other,
    Ignore,
}

#[derive(Debug, PartialEq)]
enum Response {
    Open(CenterTarget),
    Answer(AgentNoticeKey, bool),
    UpdateNodes,
    Nothing,
}

fn response(route: Route, interaction: Interaction) -> Response {
    match (interaction, route.buttons) {
        (Interaction::Open, _) => route.target.map_or(Response::Nothing, Response::Open),
        (Interaction::Action | Interaction::Other, Some(Buttons::Answer(key))) => key
            .parse::<AgentNoticeKey>()
            .ok()
            .filter(|key| key.question.is_some())
            .map_or(Response::Nothing, |key| {
                Response::Answer(key, matches!(interaction, Interaction::Action))
            }),
        (Interaction::Action, Some(Buttons::UpdateNodes)) => Response::UpdateNodes,
        _ => Response::Nothing,
    }
}

#[cfg(target_os = "macos")]
pub(crate) use mac::init;

#[cfg(target_os = "macos")]
#[allow(deprecated)] // Keep the existing notification presentation and development identity.
mod mac {
    use super::{Action, Interaction, Response, Route, notification_icon, response, ring};
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
                    NSUserNotificationActivationType::ActionButtonClicked => Interaction::Action,
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
                self.handle(notification, Interaction::Other);
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
                Response::Open(target) => ring(self.ivars(), Action::Open(target)),
                Response::Answer(key, allow) => ring(self.ivars(), Action::Answer(key, allow)),
                Response::UpdateNodes => ring(self.ivars(), Action::UpdateNodes),
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

    pub(super) fn send(app: &AppHandle, title: &str, body: &str, route: Route) {
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

        let titles = route.buttons.as_ref().map(super::Buttons::titles);
        notification.setHasActionButton(titles.is_some());

        if let Some((action, other)) = titles {
            notification.setActionButtonTitle(&NSString::from_str(action));
            notification.setOtherButtonTitle(&NSString::from_str(other));
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
#[path = "../tests/platform_notify.rs"]
mod tests;
