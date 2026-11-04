//! The tray icon and its menu: Recent chats, what waits on the user and
//! the app's own items. Entry points: `menu`, `on_menu`, `refresh`.

pub(crate) mod waiting;

use crate::{
    bar,
    beep::{Beep, state::LinkState},
    center,
    machines::this_mac::Machine,
    platform::update,
};
use beep_proto::Session;
use std::{
    cmp::Reverse,
    sync::{Arc, Mutex, PoisonError},
};
use tauri::{
    App, AppHandle, Emitter, Manager,
    image::Image,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
};
use waiting::WaitingList;

pub(crate) const TRAY: &str = "beep";
/// Chats listed under Recent in the tray menu.
const RECENT: usize = 5;
/// Menu ids of Recent's items start with this, then the session id.
const CHAT_ITEM: &str = "chat:";

/// The latest chats, kept so the tray menu can be built without waiting on
/// the server.
#[derive(Default)]
pub(crate) struct Recent(Mutex<Vec<Session>>);

/// Puts the icon in the menu bar and fetches Recent for its menu.
pub(crate) fn install(app: &App, beep: &Beep) -> tauri::Result<()> {
    let handle = app.handle();
    let icon = Image::from_bytes(include_bytes!("../../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY)
        .icon(icon)
        .icon_as_template(true)
        .tooltip(waiting::tooltip(0))
        .menu(&menu(handle, beep)?)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu)
        .build(app)?;
    tauri::async_runtime::spawn(refresh_recent(handle.clone()));
    Ok(())
}

fn menu(app: &AppHandle, beep: &Beep) -> tauri::Result<Menu<tauri::Wry>> {
    let link = beep.link_state();
    let status = match link {
        LinkState::Linked => "Linked to your phone",
        LinkState::Unlinked => "Not linked yet",
        LinkState::Revoked => "Removed from the account",
    };
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "open-center",
        "Open Beep",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "open-bar",
        "Quick chat",
        true,
        None::<&str>,
    )?)?;
    // A menu shortcut cannot show a double tap, so it gets a line of its own.
    menu.append(&MenuItem::with_id(
        app,
        "hotkey",
        "Or press Option twice, anywhere",
        false,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "status",
        status,
        false,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let linked = link == LinkState::Linked;
    menu.append(&MenuItem::with_id(
        app,
        "new",
        "New chat",
        linked,
        Some("cmd+n"),
    )?)?;
    let chats = app
        .state::<Recent>()
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let recent = Submenu::with_id(app, "recent", "Recent", linked && !chats.is_empty())?;
    for chat in &chats {
        let title = if chat.title.is_empty() {
            "New chat"
        } else {
            chat.title.as_str()
        };
        recent.append(&MenuItem::with_id(
            app,
            format!("{CHAT_ITEM}{}", chat.id),
            title,
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&recent)?;
    for (id, title, keys) in [
        ("memories", "Memories", "cmd+m"),
        ("routines", "Routines", "cmd+j"),
        ("skills", "Skills", "cmd+l"),
        ("research", "Research", "cmd+r"),
    ] {
        menu.append(&MenuItem::with_id(app, id, title, linked, Some(keys))?)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "apps",
        "Apps",
        linked,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let waiting = app.state::<waiting::WaitingList>().entries();
    if linked && !waiting.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "waiting",
            "Waiting",
            false,
            None::<&str>,
        )?)?;
        for entry in waiting.iter().take(waiting::SHOWN) {
            menu.append(&MenuItem::with_id(
                app,
                entry.id.as_str(),
                entry.label.as_str(),
                true,
                None::<&str>,
            )?)?;
        }
        if waiting.len() > waiting::SHOWN {
            let more = format!("{} more on Home", waiting.len() - waiting::SHOWN);
            menu.append(&MenuItem::with_id(
                app,
                waiting::MORE,
                more,
                true,
                None::<&str>,
            )?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if link != LinkState::Linked {
        menu.append(&MenuItem::with_id(
            app,
            "link",
            "Link to phone…",
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "settings",
        "Settings…",
        true,
        Some("cmd+,"),
    )?)?;
    let (id, title) = match platform::update::ready(app) {
        Some(_) => ("restart-update", "Restart to Update Beep"),
        None => ("check-update", "Check for Updates…"),
    };
    menu.append(&MenuItem::with_id(app, id, title, true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "show-logs",
        "Show Logs",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit Beep",
        true,
        Some("cmd+q"),
    )?)?;
    Ok(menu)
}

pub(crate) fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY) else {
        return;
    };
    let beep = app.state::<Arc<Beep>>();
    match menu(app, &beep) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => log::warn!("building the tray menu: {e}"),
    }
    // The count beside the icon, while anything waits on the user.
    let count = if beep.link_state() == LinkState::Linked {
        app.state::<waiting::WaitingList>().entries().len()
    } else {
        0
    };
    if let Err(e) = tray.set_title(waiting::badge(count)) {
        log::warn!("showing what waits beside the tray icon: {e}");
    }
    if let Err(e) = tray.set_tooltip(Some(waiting::tooltip(count))) {
        log::warn!("setting the tray tooltip: {e}");
    }
}

/// Tells the bar and the tray this Mac now signs into an account.
pub(crate) fn became_linked(app: &AppHandle) {
    let _ = app.emit("link-state", LinkState::Linked);
    app.state::<machines::this_mac::Machine>()
        .sync(&app.state::<Arc<Beep>>());
    refresh(app);
    tauri::async_runtime::spawn(refresh_recent(app.clone()));
    app.state::<waiting::WaitingList>().poke();
}

/// Fetches the latest chats for Recent and rebuilds the menu.
pub(crate) async fn refresh_recent(app: AppHandle) {
    let beep = app.state::<Arc<Beep>>().inner().clone();
    if beep.link_state() != LinkState::Linked {
        return;
    }
    match beep.sessions().await {
        Ok(mut chats) => {
            // The server lists pinned chats first; Recent is by time.
            chats.sort_by_key(|c| Reverse(c.updated_at));
            chats.truncate(RECENT);
            *app.state::<Recent>()
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = chats;
            refresh(&app);
        }
        Err(e) => log::warn!("listing chats for the tray: {e}"),
    }
}

/// The window view a tray item opens once linked, if it opens one.
fn tray_target(id: &str) -> Option<center::CenterTarget> {
    match id {
        "apps" => Some(center::CenterTarget::Apps),
        "memories" => Some(center::CenterTarget::Memories),
        "routines" => Some(center::CenterTarget::Routines),
        "skills" => Some(center::CenterTarget::Skills),
        "settings" => Some(center::CenterTarget::Settings),
        waiting::MORE => Some(center::CenterTarget::Home),
        _ => None,
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    // Something waiting opens where it is answered: its chat or agent
    // session, or Home once it no longer waits.
    if event.id().as_ref().starts_with(waiting::ITEM) {
        let target = app
            .state::<waiting::WaitingList>()
            .target(event.id().as_ref())
            .unwrap_or(center::CenterTarget::Home);
        let pending = app.state::<center::Pending>();
        if let Err(e) = center::open_on(app, &pending, target) {
            log::warn!("opening what waits: {e}");
        }
        return;
    }
    if let Some(id) = event.id().as_ref().strip_prefix(CHAT_ITEM) {
        app.state::<Arc<Beep>>().set_session(Some(id.to_owned()));
        bar::show(app, Some("open"));
        return;
    }
    if let Some(target) = tray_target(event.id().as_ref())
        && app.state::<Arc<Beep>>().link_state() == LinkState::Linked
    {
        let pending = app.state::<center::Pending>();
        if let Err(e) = center::open_on(app, &pending, target) {
            log::warn!("opening the command center: {e}");
        }
        return;
    }
    match event.id().as_ref() {
        "open-bar" => bar::show(app, Some("tray")),
        // Before linking, the bar shows the code to link with.
        "open-center" if app.state::<Arc<Beep>>().link_state() != LinkState::Linked => {
            bar::show(app, Some("link"))
        }
        "open-center" => {
            if let Err(e) = center::open(app) {
                log::warn!("opening the command center: {e}");
            }
        }
        "new" => {
            app.state::<Arc<Beep>>().set_session(None);
            bar::show(app, Some("new"));
        }
        // Before linking these stay in the bar, which shows the code to link
        // with instead.
        view @ ("apps" | "link" | "memories" | "research" | "routines" | "skills" | "settings") => {
            bar::show(app, Some(view))
        }
        "check-update" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { platform::update::check(&app, true).await });
        }
        "restart-update" => platform::update::restart(app),
        "show-logs" => {
            if let Err(e) = crate::bar::commands::open_log() {
                log::warn!("showing the logs: {e}");
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_items_name_their_view() {
        assert_eq!(
            tray_target("memories"),
            Some(center::CenterTarget::Memories)
        );
        assert_eq!(
            tray_target("settings"),
            Some(center::CenterTarget::Settings)
        );
        assert_eq!(tray_target("research"), None);
        assert_eq!(tray_target("check-update"), None);
        assert_eq!(tray_target(waiting::MORE), Some(center::CenterTarget::Home));
    }
}
