//! The tray icon and its menu: what waits on the user and the app's own
//! items, and what a click on one opens.

use super::waiting::{self, WaitingList};
use crate::{
    bar,
    center::{self, targets::REPORT},
    platform::update,
    targets::CenterTarget,
};
use tauri::{
    App, AppHandle, Manager, Wry,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

const TRAY: &str = "beep";

/// Puts the icon in the menu bar.
pub(crate) fn install(app: &App) -> tauri::Result<()> {
    let handle = app.handle();
    let icon = tauri::include_image!("icons/tray.png");
    TrayIconBuilder::with_id(TRAY)
        .icon(icon)
        .icon_as_template(true)
        .tooltip(waiting::tooltip(0))
        .menu(&menu(handle)?)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu)
        .build(app)?;
    Ok(())
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    let item = |id: &str, title: &str, enabled: bool, keys: Option<&str>| {
        menu.append(&MenuItem::with_id(app, id, title, enabled, keys)?)
    };
    let separator = || menu.append(&PredefinedMenuItem::separator(app)?);
    item("open-center", "Open Beep", true, None)?;
    item("open-bar", "Quick agent", true, None)?;
    // A menu shortcut cannot show a double tap, so it gets a line of its own.
    let shortcut = if cfg!(target_os = "windows") {
        "Or press Ctrl+Alt+Space, anywhere"
    } else {
        "Or press Option twice, anywhere"
    };
    item("hotkey", shortcut, false, None)?;
    separator()?;

    let waiting = app.state::<WaitingList>().entries();

    if !waiting.is_empty() {
        item("waiting", "Waiting", false, None)?;

        for entry in waiting.iter().take(waiting::SHOWN) {
            item(entry.id.as_str(), entry.label.as_str(), true, None)?;
        }

        if waiting.len() > waiting::SHOWN {
            let more = format!("{} more on Home", waiting.len() - waiting::SHOWN);
            item(waiting::MORE, more.as_str(), true, None)?;
        }

        separator()?;
    }

    item(REPORT, "Report a Problem", true, None)?;
    item("settings", "Settings", true, Some("CmdOrCtrl+,"))?;

    match update::ready(app) {
        Some(_) => item("restart-update", "Restart to Update Beep", true, None)?,
        None => item("check-update", "Check for Updates", true, None)?,
    }

    item("show-logs", "Show Logs", true, None)?;
    separator()?;
    item("quit", "Quit Beep", true, Some("CmdOrCtrl+q"))?;
    Ok(menu)
}

pub(crate) fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY) else {
        return;
    };
    match menu(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => log::warn!("building the tray menu: {e}"),
    }
    // The count beside the icon, while anything waits on the user.
    let count = app.state::<WaitingList>().entries().len();

    if let Err(e) = tray.set_title(waiting::badge(count)) {
        log::warn!("showing what waits beside the tray icon: {e}");
    }

    if let Err(e) = tray.set_tooltip(Some(waiting::tooltip(count))) {
        log::warn!("setting the tray tooltip: {e}");
    }
}

/// The window view a tray item opens, if it opens one.
fn tray_target(id: &str) -> Option<CenterTarget> {
    match id {
        "settings" => Some(CenterTarget::Settings),
        REPORT => Some(CenterTarget::Report),
        waiting::MORE => Some(CenterTarget::Home),
        _ => None,
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    // Something waiting opens where it is answered: its chat or agent
    // session, or Home once it no longer waits. This handler hears every
    // menu's events, so the Help menu's Report a Problem opens here too.
    let id = event.id().as_ref();
    let opening = if id.starts_with(waiting::ITEM) {
        let target = app
            .state::<WaitingList>()
            .target(id)
            .unwrap_or(CenterTarget::Home);
        Some((target, "opening what waits"))
    } else {
        tray_target(id).map(|target| (target, "opening the command center"))
    };

    if let Some((target, failure)) = opening {
        let pending = app.state::<center::Pending>();

        if let Err(e) = center::open_on(app, &pending, target) {
            log::warn!("{failure}: {e}");
        }

        return;
    }

    match event.id().as_ref() {
        "open-bar" => bar::show(app, Some("tray")),
        "open-center" => {
            if let Err(e) = center::open(app) {
                log::warn!("opening the command center: {e}");
            }
        }
        "check-update" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { update::check(&app, true).await });
        }
        "restart-update" => update::restart(app),
        "show-logs" => {
            if let Err(e) = crate::bar::app::open_log() {
                log::warn!("showing the logs: {e}");
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

#[cfg(test)]
#[path = "../tests/tray_menu.rs"]
mod tests;
