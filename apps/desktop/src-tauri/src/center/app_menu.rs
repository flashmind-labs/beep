//! The app's menu bar: the default menus, with Report a Problem in Help.
//! The tray's menu handler hears every menu's events, so it opens the sheet
//! for this item too.

use crate::tray::REPORT;
use tauri::{
    App,
    menu::{HELP_SUBMENU_ID, Menu, MenuItem, Submenu},
};

pub(crate) fn install(app: &App) -> tauri::Result<()> {
    let handle = app.handle();
    let menu = Menu::default(handle)?;
    let report = MenuItem::with_id(handle, REPORT, "Report a Problem…", true, None::<&str>)?;
    let help = menu.get(HELP_SUBMENU_ID);
    match help.as_ref().and_then(|item| item.as_submenu()) {
        Some(help) => help.append(&report)?,
        None => menu.append(&Submenu::with_items(handle, "Help", true, &[&report])?)?,
    }
    handle.set_menu(menu)?;
    Ok(())
}
