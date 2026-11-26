//! What each notice a machine of the mesh sends does to the tray and the
//! windows, and when it shows as a macOS notification.

use crate::{
    agents::outbox,
    bar,
    beep::state::Prefs,
    center,
    error::Result,
    fleet::Fleet,
    fleet::{alerts::Direct, session_mirror::SessionMirror},
    notes,
    platform::notify::{self, Action},
    targets::CenterTarget,
};
use beep_fleet::Read;
use beep_mesh::machine::listing::Host;
use beep_proto::{AgentNoticeKey, Notice};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{sync::mpsc, time::sleep_until};

/// How long after a machine's stream opens its news waits for the reads of the other apps. What
/// a machine sends first may be news it held while this app was away, such as a phone in the
/// background, and another app may have read it meanwhile.
const SETTLE: Duration = Duration::from_secs(5);

/// Carries out what a notification's button asked for, in the order asked. A failure shows as
/// another notification, since nothing else is on screen; one already answered elsewhere only
/// goes to the log.
pub(crate) async fn follow_actions(app: AppHandle, mut actions: mpsc::UnboundedReceiver<Action>) {
    while let Some(action) = actions.recv().await {
        match action {
            Action::Open(target) => open(&app, target),
            Action::Answer(key, allow) => {
                if let Err(e) = answer_agent(&app, key, allow).await {
                    log::warn!("answering from a notification: {e}");
                    notify::text(&app, "Beep could not send your answer", &e.to_string());
                }
            }
            #[cfg(desktop)]
            Action::UpdateNodes => {
                tauri::async_runtime::spawn(crate::machines::node_updates::update_behind(
                    app.clone(),
                ));
            }
            #[cfg(mobile)]
            Action::UpdateNodes => {}
        }
    }
}

/// Opens `target` in the command center, from the main thread as the tray does.
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

/// Answers the question `key` names on the machine of the mesh that asked
/// it, as Allow or Deny on its notification do. The machine refuses an
/// answer to a question the session no longer asks, as a conflict.
pub(crate) async fn answer_agent(app: &AppHandle, key: AgentNoticeKey, allow: bool) -> Result<()> {
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    outbox::answer(app, &fleet, "", &key, notify::agent_reply(allow).to_owned()).await
}

/// Routes what the machines of the mesh send.
pub(crate) async fn serve(app: AppHandle, mut direct: mpsc::Receiver<Direct>) {
    let mut settle = Instant::now();

    while let Some(direct) = direct.recv().await {
        match direct {
            Direct::Notice { notice, allow_deny } => {
                match notice.session_id.parse::<AgentNoticeKey>() {
                    Ok(key) => agent_notice(&app, &key, *notice, allow_deny, settle),
                    Err(_) => log::debug!("a machine sent a notice for no agent session"),
                }
            }
            Direct::Presence { machine, sessions } => {
                let fleet = app.state::<Arc<Fleet>>();
                let host = Host {
                    id: machine.to_string(),
                    name: fleet.name_of(machine),
                };
                app.state::<Arc<SessionMirror>>()
                    .presence(machine, host, sessions);
            }
            // A build: the bell keeps it, and the first machine to tell
            // it shows it.
            Direct::Run(news) => {
                if let Some(note) = notes::run_ended(&app, &news)
                    && !note.read
                    && let Some(target) = note.target.clone()
                {
                    let read = note.mark();
                    show_unread(&app, read, settle, move |app| {
                        let body = format!("{}: {}", note.source, note.text);
                        notify::opening(app, &note.title, &body, target);
                    });
                }
            }
            Direct::Phone { machine, device } => {
                let on = app.state::<Arc<Fleet>>().name_of(machine);
                notify::text(&app, "Phone connected", &format!("{} on {on}", device.name));
            }
            // Connected again: what waits on the user may have changed
            // meanwhile.
            Direct::Connected => {
                settle = Instant::now() + SETTLE;
                let _ = app.emit("reconnected", ());
            }
        }
    }
}

/// A coding agent asked something or finished a turn. The bell keeps it, and the notice shows
/// unless agent notices are off, the session is on screen or any app read it.
/// A child's finished turn tells nothing, but a reviewer's means its review is in.
/// A question Allow and Deny cannot answer only opens its chat.
fn agent_notice(
    app: &AppHandle,
    key: &AgentNoticeKey,
    notice: Notice,
    allow_deny: bool,
    settle: Instant,
) {
    let rows = app.state::<Arc<SessionMirror>>().current();
    let link = rows.link(&key.machine_id, &key.session);
    let reviewer = link.is_some_and(|link| link.reviewer);

    if key.question.is_none() && !reviewer && link.is_some_and(|link| link.has_parent()) {
        return;
    }

    let focused = focused_center_label(app);
    let mut on_screen: Vec<&str> = focused.iter().map(String::as_str).collect();
    if bar::is_open(app) {
        on_screen.push(bar::LABEL);
    }

    let shown =
        app.state::<crate::agents::shown::AgentsShown>()
            .shows(&on_screen, key, Instant::now());
    let unread = notes::agent_notice(app, key, &notice, allow_deny, shown, reviewer);

    if !app.state::<Prefs>().agent_notices() || shown {
        return;
    }

    if let Some(read) = unread {
        show_unread(app, read, settle, move |app| {
            notify::notice(app, &notice, allow_deny, reviewer)
        });
    }
}

/// Runs `show` for the note `read` names now, or at `settle` if no app read it by then.
fn show_unread(
    app: &AppHandle,
    read: Read,
    settle: Instant,
    show: impl FnOnce(&AppHandle) + Send + 'static,
) {
    if Instant::now() >= settle {
        show(app);
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        sleep_until(settle.into()).await;

        if notes::still_unread(&app, &read) {
            show(&app);
        }
    });
}

/// The label of the command center window with focus, the main one or a
/// project's; nothing while none has focus.
fn focused_center_label(app: &AppHandle) -> Option<String> {
    app.webview_windows()
        .into_iter()
        .find(|(label, w)| center::is_center(label) && w.is_focused().unwrap_or(false))
        .map(|(label, _)| label)
}
