//! What each notice the server pushes does to the tray, the windows and
//! live mode, and when it shows as a macOS notification.

use super::{alerts, notify, work};
use crate::{
    bar,
    beep::{
        Beep, Field, Notices,
        state::{LinkState, NoticeVoice},
    },
    center,
    chat::groups::GroupApproval,
    fleet::alerts::{Direct, Heard, Via},
    tray::{self, waiting::WaitingList},
    voice::Voice,
};
use beep_proto::{AgentNoticeKey, Notice};
use std::{sync::Arc, time::Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{select, sync::mpsc};
use uuid::Uuid;

/// Routes what the server pushes and what machines in the mesh send
/// directly. The server pushes notices to every key on the account; the
/// first one this key gets means the phone linked it. A coding-agent notice
/// that comes both ways shows once.
pub(crate) async fn serve(
    app: AppHandle,
    beep: Arc<Beep>,
    voice: Arc<Voice>,
    mut notices: Notices,
    mut direct: mpsc::Receiver<Direct>,
) {
    let mut heard = Heard::default();
    loop {
        let (notice, via) = select! {
            Some(notice) = notices.recv() => (notice, Via::Server),
            Some(direct) = direct.recv() => match direct {
                Direct::Notice(notice) => (*notice, Via::Machine),
                // Connected again: what the bar shows from outside the log,
                // such as questions from background work, may have changed
                // meanwhile, and so may what waits on the user.
                Direct::Connected => {
                    let _ = app.emit("reconnected", ());
                    app.state::<WaitingList>().poke();
                    continue;
                }
            },
            else => return,
        };
        if matches!(via, Via::Server) && beep.link_state() == LinkState::Unlinked {
            beep.set_link_state(LinkState::Linked);
            tray::became_linked(&app);
        }
        if let Ok(key) = notice.session_id.parse::<AgentNoticeKey>()
            && !heard.first(&key, via, Instant::now())
        {
            continue;
        }
        route(&app, &beep, &voice, notice);
    }
}

/// What `notice` does to the tray, the windows and live mode.
fn route(app: &AppHandle, beep: &Arc<Beep>, voice: &Arc<Voice>, notice: Notice) {
    // What waits on the user may have changed; the tray looks again.
    if notice.approval.is_some() || (notice.changed && !notice.group) {
        app.state::<WaitingList>().poke();
    }
    // Checked before `changed`, which a resolved question also sets.
    if let Some(query_id) = notice.resolved_question() {
        let resolved = work::Resolved {
            session_id: notice.session_id.clone(),
            query_id: query_id.to_owned(),
        };
        let _ = app.emit("work-resolved", resolved);
        voice.withdrawn(query_id);
        return;
    }
    if notice.group {
        group_notice(app, beep, notice);
        return;
    }
    if notice.changed {
        // The assistant started or ended an alert if the session is empty.
        if notice.session_id.is_empty() {
            tauri::async_runtime::spawn(alerts::refresh(app.clone()));
        }
        let _ = app.emit("sessions-changed", notice.session_id);
        tauri::async_runtime::spawn(tray::refresh_recent(app.clone()));
        return;
    }
    if let Ok(key) = notice.session_id.parse::<AgentNoticeKey>() {
        agent_notice(app, beep, &key, &notice);
        return;
    }
    // Background work asks only through a notice, so its chat keeps a
    // card for the question.
    if let Some(question) =
        work::WorkQuestion::from_notice(&notice.session_id, notice.approval.as_ref())
    {
        let _ = app.emit("work-question", question);
    }
    // A chat finished on its own may be an alert, which is a notification
    // and never a chat opening by itself: the server says. A question
    // waiting on an approval is a chat's, and goes to every device as ever.
    if notice.approval.is_none() && Uuid::parse_str(&notice.session_id).is_ok() {
        tauri::async_runtime::spawn(alerts::arrived(
            app.clone(),
            beep.clone(),
            voice.clone(),
            notice,
        ));
        return;
    }
    chat_notice(app, beep, voice, notice);
}

/// A notice for a chat: nothing while it is on screen, else the macOS
/// notification, or the offer of a voice note.
pub(super) fn chat_notice(app: &AppHandle, beep: &Beep, voice: &Arc<Voice>, notice: Notice) {
    let showing = focused_center(app);
    if center::on_screen(
        &seen(app, beep.session(), showing.session),
        &notice.session_id,
    ) {
        return;
    }
    // While live mode is on in the orb, an approval asks there.
    let Some(notice) = voice.approval_notice(notice) else {
        return;
    };
    match voice.wants(&notice) {
        NoticeVoice::Aloud => {
            tauri::async_runtime::spawn(voice.clone().speak_notice(notice));
        }
        NoticeVoice::Ask => voice.offer(notice),
        NoticeVoice::Off => notify::notice(app, &notice),
    }
}

/// A message or change in a group. The bar refreshes it, and a message
/// notifies unless the bar shows that group.
fn group_notice(app: &AppHandle, beep: &Beep, notice: Notice) {
    let id = notice.session_id.clone();
    let _ = app.emit("group-changed", &id);
    if let Some(pending) = &notice.approval {
        let _ = app.emit(
            "group-approval",
            GroupApproval {
                group_id: id.clone(),
                query_id: pending.query_id.clone(),
                app: pending.approval.app.clone(),
                action: pending.approval.action.clone(),
                fields: pending
                    .approval
                    .fields
                    .iter()
                    .cloned()
                    .map(Field::from)
                    .collect(),
            },
        );
    }
    if notice.changed || notice.title.is_empty() {
        return;
    }
    // An approval still notifies: the bar may be on another chat by then.
    let showing = focused_center(app);
    if center::on_screen(&seen(app, beep.group(), showing.group), &id) && notice.approval.is_none()
    {
        return;
    }
    notify::notice(app, &notice);
}

/// A coding agent asked something or finished a turn. The tray looks at
/// what waits again, and the notice shows unless agent notices are off or
/// the session is on screen.
fn agent_notice(app: &AppHandle, beep: &Beep, key: &AgentNoticeKey, notice: &Notice) {
    app.state::<WaitingList>().poke();
    if !beep.agent_notices() {
        return;
    }
    let focused = focused_center_label(app);
    let mut on_screen: Vec<&str> = focused.iter().map(String::as_str).collect();
    if bar::is_open(app) {
        on_screen.push(bar::LABEL);
    }
    let shown = app.state::<center::AgentsShown>();
    if shown.shows(&on_screen, key, Instant::now()) {
        return;
    }
    notify::notice(app, notice);
}

/// The label of the command center window with focus, the main one or a
/// project's; nothing while none has focus.
fn focused_center_label(app: &AppHandle) -> Option<String> {
    app.webview_windows()
        .into_iter()
        .find(|(label, w)| center::is_center(label) && w.is_focused().unwrap_or(false))
        .map(|(label, _)| label)
}

/// What the command center window with focus shows, the main one or a
/// project's; nothing while none has focus.
fn focused_center(app: &AppHandle) -> center::Showing {
    match focused_center_label(app) {
        Some(label) => app.state::<center::Shown>().get(&label),
        None => center::Showing::default(),
    }
}

/// What is on screen: the bar's chat while the bar is open, and `window`,
/// what the focused command center window shows.
fn seen(app: &AppHandle, bar: Option<String>, window: Option<String>) -> center::Seen {
    center::Seen {
        bar: bar.filter(|_| bar::is_open(app)),
        window,
    }
}
