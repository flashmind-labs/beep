//! Beep for macOS. A tray app whose bar opens on a double tap of Option.

mod apps;
mod audio;
mod bar;
mod beep;
mod cards;
mod commands;
mod desktop;
mod devices;
mod error;
mod groups;
mod hotkey;
mod identity;
mod listen;
mod logfile;
mod login_path;
mod mac_files;
mod machine;
mod markdown;
mod news;
mod presence;
mod products;
mod research;
mod state;
mod sync;
mod update;
mod voice;

use anyhow::Context;
use beep::{Beep, Field, Notices};
use beep_proto::{Notice, Session, link::MAX_NAME};
use groups::GroupApproval;
use iroh::EndpointId;
use state::{LinkState, StateFile};
use std::{
    cmp::Reverse,
    fs,
    io::ErrorKind,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, PoisonError},
};
use tauri::{
    ActivationPolicy, App, AppHandle, Emitter, Manager, RunEvent, WindowEvent,
    image::Image,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_notification::NotificationExt;
use voice::Voice;

/// The server's node id, baked in by build.rs.
const SERVER_NODE_ID: &str = env!("BEEP_SERVER_NODE_ID");
const TRAY: &str = "beep";
/// Chats listed under Recent in the tray menu.
const RECENT: usize = 5;
/// Menu ids of Recent's items start with this, then the session id.
const CHAT_ITEM: &str = "chat:";

/// The latest chats, kept so the tray menu can be built without waiting on
/// the server.
#[derive(Default)]
struct Recent(Mutex<Vec<Session>>);

fn main() {
    logfile::start();
    log::info!("Beep {} starting", env!("CARGO_PKG_VERSION"));
    // SAFETY: nothing else runs yet; Tauri starts its threads below.
    unsafe { login_path::adopt() };
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| Ok(setup(app)?))
        .on_window_event(|window, event| match event {
            // Only the bar is a popover; research windows stay put.
            WindowEvent::Focused(false) if window.label() == bar::LABEL => {
                let _ = window.hide();
            }
            WindowEvent::Destroyed => research::closed(window.app_handle(), window.label()),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::link_offer,
            commands::send_prompt,
            commands::rerun_prompt,
            commands::answer_approval,
            commands::active_run,
            commands::follow_run,
            commands::cancel_run,
            commands::list_sessions,
            commands::search_sessions,
            commands::open_session,
            commands::session_files,
            commands::new_chat,
            commands::rename_session,
            commands::pin_session,
            commands::delete_session,
            commands::share_session,
            commands::unshare_session,
            commands::list_groups,
            commands::open_group,
            commands::close_group,
            commands::send_to_group,
            commands::create_group,
            commands::group_invite,
            commands::peek_group,
            commands::join_group,
            commands::leave_group,
            commands::pin_group,
            commands::share_group_apps,
            commands::list_group_reminders,
            commands::delete_group_reminder,
            commands::answer_group_approval,
            commands::open_file,
            commands::picture,
            commands::save_file,
            commands::reveal_file,
            commands::unlink,
            apps::list_apps,
            apps::connect_app,
            apps::cancel_app_sign_in,
            apps::disconnect_app,
            apps::list_app_accounts,
            apps::remove_app_account,
            apps::app_permissions,
            apps::set_app_permissions,
            apps::list_mcp_servers,
            apps::add_mcp_server,
            apps::connect_mcp_server,
            apps::remove_mcp_server,
            apps::list_mcp_tools,
            apps::set_mcp_tool,
            devices::use_here,
            devices::list_devices,
            devices::invite_phone,
            devices::remove_device,
            devices::join_account,
            commands::settings,
            commands::set_autostart,
            commands::mac_control,
            commands::set_mac_control,
            commands::allow_mac_permission,
            commands::set_voice_notes,
            commands::live_tap,
            commands::leave_live,
            commands::continue_live,
            commands::set_send_on_pause,
            commands::spoken_turns,
            commands::live_chats,
            commands::start_over,
            commands::resize_bar,
            commands::hide_bar,
            commands::quit,
            commands::open_url,
            commands::group_typing,
            commands::list_memories,
            commands::update_memory,
            commands::delete_memory,
            commands::clear_memories,
            commands::list_crons,
            commands::create_cron,
            commands::toggle_cron,
            commands::delete_cron,
            commands::run_cron,
            commands::list_skills,
            commands::update_skill,
            commands::delete_skill,
            commands::undo_skill,
            commands::get_agents,
            commands::get_models,
            commands::stock_chart,
            commands::route_card,
            commands::set_model,
            commands::set_thinking,
            commands::set_auto_model,
            research::research_begin,
            research::open_research,
            research::research_sessions,
            research::list_researches,
            research::research_pending,
            research::research_history,
            research::research_send,
            research::get_research,
            research::cached_research,
            research::stop_research,
            research::export_research,
        ])
        .build(tauri::generate_context!())
        .expect("the app failed to start")
        .run(|app, event| {
            // However Beep quits, a downloaded update is installed for next time.
            if let RunEvent::Exit = event {
                update::install(app);
            }
        });
}

fn data_dir(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_data_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
    Ok(dir)
}

fn setup(app: &mut App) -> anyhow::Result<()> {
    app.set_activation_policy(ActivationPolicy::Accessory);
    let handle = app.handle().clone();
    let dir = data_dir(&handle)?;
    let server: EndpointId = SERVER_NODE_ID
        .parse()
        .context("built without a server node id; set BEEP_NODE_ID")?;
    let identity = identity::load_or_create(&dir.join("identity.key"))?;
    let file = StateFile::new(dir.join("state.json"));
    let chats = sync::Chats::load(dir.join("sync"));
    let (beep, notices) =
        tauri::async_runtime::block_on(Beep::start(identity, server, file, chats))?;
    app.manage(beep.clone());
    app.manage(Recent::default());
    app.manage(research::Pending::default());
    app.manage(research::Cache::default());
    app.manage(apps::SignIn::default());
    app.manage(update::Updates::default());
    app.manage(machine::Machine::default());
    app.state::<machine::Machine>().sync(&beep);
    let voice = Voice::new(handle.clone(), beep.clone());
    app.manage(voice.clone());

    tauri::async_runtime::spawn(serve_notices(
        handle.clone(),
        beep.clone(),
        voice.clone(),
        notices,
    ));
    tauri::async_runtime::spawn(sync::follow(handle.clone(), beep.clone()));
    tauri::async_runtime::spawn(update::follow(handle.clone()));
    hotkey::start(move |input| voice.on_input(input));

    // Sign-ins come back as beep:// links, while running or as the reason
    // macOS started Beep.
    let links = handle.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            apps::open_link(&links, url.as_str());
        }
    });
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            apps::open_link(&handle, url.as_str());
        }
    }

    let menu = tray_menu(&handle, &beep)?;
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Beep")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu)
        .build(app)?;
    tauri::async_runtime::spawn(refresh_recent(handle.clone()));

    // First launch, or a link still to finish: show the code.
    if beep.link_state() != LinkState::Linked {
        bar::show(&handle, Some("link"));
    }
    Ok(())
}

/// The server pushes notices to every key on the account. The first one
/// this key gets means the phone linked it.
async fn serve_notices(app: AppHandle, beep: Arc<Beep>, voice: Arc<Voice>, mut notices: Notices) {
    while let Some(notice) = notices.recv().await {
        if beep.link_state() == LinkState::Unlinked {
            beep.set_link_state(LinkState::Linked);
            became_linked(&app);
        }
        if notice.group {
            group_notice(&app, &beep, notice);
            continue;
        }
        if notice.changed {
            let _ = app.emit("sessions-changed", notice.session_id);
            tauri::async_runtime::spawn(refresh_recent(app.clone()));
            continue;
        }
        if bar::is_open(&app) && beep.session().as_deref() == Some(notice.session_id.as_str()) {
            continue;
        }
        if voice.wants(&notice) {
            tauri::async_runtime::spawn(voice.clone().speak_notice(notice));
            continue;
        }
        notify(&app, &notice);
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
    if bar::is_open(app)
        && beep.group().as_deref() == Some(id.as_str())
        && notice.approval.is_none()
    {
        return;
    }
    notify(app, &notice);
}

fn notify(app: &AppHandle, notice: &Notice) {
    notify_text(app, &notice.title, &notice.body);
}

fn notify_text(app: &AppHandle, title: &str, body: &str) {
    let shown = app.notification().builder().title(title).body(body).show();
    if let Err(e) = shown {
        log::warn!("showing a notice: {e}");
    }
}

fn tray_menu(app: &AppHandle, beep: &Beep) -> tauri::Result<Menu<tauri::Wry>> {
    let link = beep.link_state();
    let status = match link {
        LinkState::Linked => "Linked to your phone",
        LinkState::Unlinked => "Not linked yet",
        LinkState::Revoked => "Removed from the account",
    };
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "open-bar",
        "Open Beep",
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
    let (id, title) = match update::ready(app) {
        Some(_) => ("restart-update", "Restart to Update Beep"),
        None => ("check-update", "Check for Updates…"),
    };
    menu.append(&MenuItem::with_id(app, id, title, true, None::<&str>)?)?;
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

fn refresh_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY) else {
        return;
    };
    let beep = app.state::<Arc<Beep>>();
    match tray_menu(app, &beep) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => log::warn!("building the tray menu: {e}"),
    }
}

/// Tells the bar and the tray this Mac now signs into an account.
pub fn became_linked(app: &AppHandle) {
    let _ = app.emit("link-state", LinkState::Linked);
    app.state::<machine::Machine>()
        .sync(&app.state::<Arc<Beep>>());
    refresh_tray(app);
    tauri::async_runtime::spawn(refresh_recent(app.clone()));
}

/// Fetches the latest chats for Recent and rebuilds the menu.
async fn refresh_recent(app: AppHandle) {
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
            refresh_tray(&app);
        }
        Err(e) => log::warn!("listing chats for the tray: {e}"),
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    if let Some(id) = event.id().as_ref().strip_prefix(CHAT_ITEM) {
        app.state::<Arc<Beep>>().set_session(Some(id.to_owned()));
        bar::show(app, Some("open"));
        return;
    }
    match event.id().as_ref() {
        "open-bar" => bar::show(app, Some("tray")),
        "new" => {
            app.state::<Arc<Beep>>().set_session(None);
            bar::show(app, Some("new"));
        }
        view @ ("apps" | "link" | "memories" | "research" | "routines" | "skills" | "settings") => {
            bar::show(app, Some(view))
        }
        "check-update" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { update::check(&app, true).await });
        }
        "restart-update" => update::restart(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

/// The name the phone shows for this Mac.
pub fn computer_name() -> String {
    let mut name = Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Mac".into());
    name.truncate(name.floor_char_boundary(MAX_NAME));
    name
}

/// Deletes this computer's key and state, so the next launch makes new ones.
pub fn forget_identity(app: &AppHandle) -> anyhow::Result<()> {
    let dir = data_dir(app)?;
    for file in ["identity.key", "state.json"] {
        match fs::remove_file(dir.join(file)) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
