//! Commands for live mode and the orb: taps, leaving, the sheet, and the
//! orb's approval and notice cards.

use crate::{
    bar,
    beep::Service,
    error::{DesktopError, Result},
    voice::{NoticeCard, Voice, Waiting, orb},
};
use std::sync::Arc;
use tauri::{AppHandle, State};
use uuid::Uuid;

type Live<'a> = State<'a, Arc<Voice>>;

/// The approvals the orb's card shows while live mode is on, oldest first.
#[tauri::command]
pub fn live_approval(voice: Live<'_>) -> Vec<Waiting> {
    voice.waiting()
}

/// A click on the orb's approval card: the bar opens on its chat, where the
/// whole card is.
#[tauri::command]
pub fn open_approval(app: AppHandle, beep: Service<'_>, session: String) -> Result<()> {
    Uuid::parse_str(&session).map_err(|_| DesktopError::BadRequest("not a chat".into()))?;
    beep.set_session(Some(session));
    bar::show(&app, Some("open"));
    Ok(())
}

/// The new notice the orb's card offers, for an orb opened after it came.
#[tauri::command]
pub fn notice_card(voice: Live<'_>) -> Option<NoticeCard> {
    voice.notice_card()
}

/// Play on the orb's notice card: it is read aloud as a voice note.
#[tauri::command]
pub fn play_notice(voice: Live<'_>, id: u64) {
    voice.play_card(id);
}

/// Open on the orb's notice card, or a click on it: the bar opens on its
/// chat.
#[tauri::command]
pub fn open_notice(voice: Live<'_>, id: u64) {
    voice.open_card(id);
}

/// × on the orb's notice card.
#[tauri::command]
pub fn dismiss_notice(voice: Live<'_>, id: u64) {
    voice.dismiss_card(id);
}

/// The pointer is on the orb's notice card, which keeps it up, or left it.
#[tauri::command]
pub fn hold_notice(voice: Live<'_>, id: u64, held: bool) {
    voice.hold_card(id, held);
}

/// Puts the orb's window at `width` by `height`, kept in the top right.
#[tauri::command]
pub fn fit_orb(app: AppHandle, width: f64, height: f64) -> Result<()> {
    Ok(orb::fit(&app, width, height)?)
}

/// A tap in the live view: interrupts Beep, or leaves while listening.
#[tauri::command]
pub fn live_tap(voice: Live<'_>) {
    voice.tap();
}

/// Leaves live mode, keeping what was said as a draft.
#[tauri::command]
pub fn leave_live(voice: Live<'_>) {
    voice.leave_live();
}

/// Esc or Cancel in live mode: cancels a reply being written or read out,
/// or leaves, keeping what was said.
#[tauri::command]
pub fn cancel_live(voice: Live<'_>) {
    voice.cancel();
}

/// Opens or closes the sheet of what to say in live mode; `None` toggles.
#[tauri::command]
pub fn live_sheet(voice: Live<'_>, open: Option<bool>) {
    voice.sheet(open);
}

/// Chats live mode talked in, for the mic in the chat list.
#[tauri::command]
pub fn live_chats(beep: Service<'_>) -> Vec<String> {
    beep.live_chats().iter().map(Uuid::to_string).collect()
}

/// Whether a pause sends in live mode, rather than only saying "send".
#[tauri::command]
pub fn set_send_on_pause(beep: Service<'_>, voice: Live<'_>, enabled: bool) {
    beep.set_send_on_pause(enabled);
    voice.set_end_mode();
}
