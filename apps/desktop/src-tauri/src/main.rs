//! Beep for macOS. A tray app whose bar opens on a double tap of Option.

mod add_machine;
mod agent_accounts;
mod agents;
mod apps;
mod audio;
mod bar;
mod beep;
mod cards;
mod center;
mod cli_login;
mod commands;
mod credentials;
mod desktop;
mod devices;
mod error;
mod file_sync;
mod gh_sign_in;
mod git_items;
mod grants;
mod groups;
mod hotkey;
mod identity;
mod instructions;
mod logfile;
mod login_path;
mod machine;
mod machines;
mod markdown;
mod news;
mod orb;
mod overview;
mod palette;
mod presence;
mod products;
mod projects;
mod research;
mod reviews;
mod state;
mod sync;
mod terminals;
mod tool_labels;
mod update;
mod voice;
mod work;

use anyhow::Context;
use beep::{Beep, Field, Notices};
use beep_proto::{Notice, Session, link::MAX_NAME};
use groups::GroupApproval;
use iroh::EndpointId;
use state::{LinkState, NoticeVoice, StateFile};
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
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_notification::NotificationExt;
use voice::Voice;

/// The server's node id, baked in by build.rs.
const SERVER_NODE_ID: &str = env!("BEEP_SERVER_NODE_ID");
const TRAY: &str = "beep";
/// Passed to Beep when it starts as a login item, so it stays in the tray.
const LOGIN_ARG: &str = "--login";
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![LOGIN_ARG]),
        ))
        .setup(|app| Ok(setup(app)?))
        .on_window_event(|window, event| match event {
            // Only the bar is a popover; research windows stay put. Live
            // mode goes on in the orb, so the microphone never runs unseen.
            WindowEvent::Focused(false)
                if window.label() == bar::LABEL && window.is_visible().unwrap_or(false) =>
            {
                bar::hide(window.app_handle());
                if let Some(voice) = window.app_handle().try_state::<Arc<Voice>>()
                    && voice.is_live()
                {
                    voice.minimize();
                }
            }
            WindowEvent::Destroyed => {
                if window.label() == center::LABEL {
                    // Its chat is not on screen any more, so its notices must
                    // not be held back as seen.
                    window.app_handle().state::<center::Shown>().clear();
                }
                research::closed(window.app_handle(), window.label());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            add_machine::create_node_token,
            add_machine::revoke_node_token,
            add_machine::node_token_machine,
            agents::agent_hosts,
            agents::agent_sessions,
            agents::agent_start,
            agents::agent_preview,
            agents::agent_view,
            agents::agent_steer,
            center::open_center,
            center::center_home,
            center::center_answer_agent,
            center::center_dismiss_updates,
            center::center_open,
            center::center_pending,
            palette::center_palette,
            palette::center_palette_branches,
            palette::center_find,
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
            commands::session_sources,
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
            work::answer_work,
            work::list_questions,
            grants::list_grants,
            grants::revoke_grant,
            machines::list_machines,
            credentials::credential_providers,
            credentials::list_credentials,
            credentials::save_credential,
            credentials::remove_credential,
            credentials::list_github_accounts,
            credentials::set_default_github_account,
            cli_login::cli_logins,
            cli_login::use_cli_login,
            cli_login::use_gh_account,
            gh_sign_in::gh_sign_in,
            gh_sign_in::cancel_gh_sign_in,
            agent_accounts::agent_accounts,
            agent_accounts::save_agent_token,
            agent_accounts::revoke_agent_token,
            agent_accounts::claude_sign_in,
            agent_accounts::cancel_claude_sign_in,
            agent_accounts::codex_login_start,
            agent_accounts::codex_login_state,
            agent_accounts::codex_login_cancel,
            projects::import_project,
            projects::list_machine_folder,
            projects::pick_folder,
            projects::found_projects,
            projects::project_sessions,
            projects::project_machines,
            file_sync::project_sync,
            git_items::git_items,
            projects::open_folder,
            projects::center_projects,
            projects::projects_on_top,
            overview::project_overview,
            instructions::instructions_get,
            instructions::instructions_save,
            instructions::instructions_versions,
            instructions::agent_files_read,
            instructions::agent_files_sync,
            terminals::terminals_list,
            terminals::terminal_start,
            terminals::terminal_output,
            terminals::terminal_close,
            reviews::project_branches,
            reviews::reviews_list,
            reviews::review_start,
            reviews::review_get,
            reviews::review_dismiss,
            reviews::review_fix,
            reviews::review_post,
            reviews::review_merge,
            projects::pin_project_to_top,
            projects::center_open_project,
            projects::center_project,
            projects::center_project_send,
            projects::center_update_task,
            projects::center_stop_worker,
            agents::agent_resume,
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
            commands::set_notice_voice,
            commands::notice_card,
            commands::play_notice,
            commands::open_notice,
            commands::dismiss_notice,
            commands::hold_notice,
            commands::live_tap,
            commands::leave_live,
            commands::cancel_live,
            commands::show_bar,
            commands::live_approval,
            commands::open_approval,
            commands::fit_orb,
            commands::live_sheet,
            commands::set_send_on_pause,
            commands::live_chats,
            commands::start_over,
            commands::resize_bar,
            commands::hide_bar,
            commands::quit,
            commands::open_url,
            commands::open_log,
            commands::group_typing,
            commands::list_memories,
            commands::list_memory_scopes,
            commands::list_memory_reads,
            commands::update_memory,
            commands::delete_memory,
            commands::clear_memories,
            commands::list_crons,
            commands::create_cron,
            commands::toggle_cron,
            commands::update_cron,
            commands::delete_cron,
            commands::run_cron,
            commands::list_skills,
            commands::update_skill,
            commands::delete_skill,
            commands::undo_skill,
            commands::get_agents,
            commands::set_agents,
            commands::session_agents,
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
            match event {
                // However Beep quits, a downloaded update is installed for next time.
                RunEvent::Exit => {
                    update::install(app);
                    // gh, if it is still waiting for a GitHub code, goes too.
                    app.state::<gh_sign_in::GhSignIn>().stop_on_quit();
                    app.state::<agent_accounts::ClaudeSignIn>().stop();
                    // Background tasks the agent started here end with the app.
                    beep_node::kill_tasks();
                }
                // Opening Beep again while it runs shows the window.
                #[cfg(target_os = "macos")]
                RunEvent::Reopen { .. } => {
                    if app.state::<Arc<Beep>>().link_state() == LinkState::Linked
                        && let Err(e) = center::open(app)
                    {
                        log::warn!("opening the window: {e}");
                    }
                }
                _ => {}
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
    app.manage(gh_sign_in::GhSignIn::default());
    app.manage(agent_accounts::ClaudeSignIn::default());
    app.manage(update::Updates::default());
    app.manage(machine::Machine::new(&dir.join("node")));
    app.manage(center::Shown::default());
    app.manage(center::Pending::default());
    app.manage(palette::PaletteCache::default());
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
    } else if !std::env::args().any(|arg| arg == LOGIN_ARG) {
        // Opened by the user, not as a login item: show the window.
        if let Err(e) = center::open(&handle) {
            log::warn!("opening the window at launch: {e}");
        }
    }
    refresh_login_item(&handle);
    Ok(())
}

/// Login items made before they carried `LOGIN_ARG` open the window at
/// login; enabling again rewrites them with it.
fn refresh_login_item(app: &AppHandle) {
    let manager = app.autolaunch();
    match manager.is_enabled() {
        Ok(true) => {
            if let Err(e) = manager.enable() {
                log::warn!("refreshing the login item: {e}");
            }
        }
        Ok(false) => {}
        Err(e) => log::warn!("reading the login item: {e}"),
    }
}

/// The server pushes notices to every key on the account. The first one
/// this key gets means the phone linked it.
async fn serve_notices(app: AppHandle, beep: Arc<Beep>, voice: Arc<Voice>, mut notices: Notices) {
    while let Some(notice) = notices.recv().await {
        if beep.link_state() == LinkState::Unlinked {
            beep.set_link_state(LinkState::Linked);
            became_linked(&app);
        }
        // Checked before `changed`, which a resolved question also sets.
        if let Some(query_id) = notice.resolved_question() {
            let resolved = work::Resolved {
                session_id: notice.session_id.clone(),
                query_id: query_id.to_owned(),
            };
            let _ = app.emit("work-resolved", resolved);
            continue;
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
        // Background work asks only through a notice, so its chat keeps a
        // card for the question.
        if let Some(question) =
            work::WorkQuestion::from_notice(&notice.session_id, notice.approval.as_ref())
        {
            let _ = app.emit("work-question", question);
        }
        let showing = app.state::<center::Shown>().get();
        if center::on_screen(
            &seen(&app, beep.session(), showing.session),
            &notice.session_id,
        ) {
            continue;
        }
        match voice.wants(&notice) {
            NoticeVoice::Aloud => {
                tauri::async_runtime::spawn(voice.clone().speak_notice(notice));
            }
            NoticeVoice::Ask => voice.offer(notice),
            NoticeVoice::Off => notify(&app, &notice),
        }
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
    let showing = app.state::<center::Shown>().get();
    if center::on_screen(&seen(app, beep.group(), showing.group), &id) && notice.approval.is_none()
    {
        return;
    }
    notify(app, &notice);
}

/// What is on screen: the bar's chat while the bar is open, the command
/// center's while its window has focus.
fn seen(app: &AppHandle, bar: Option<String>, window: Option<String>) -> center::Seen {
    let focused = app
        .get_webview_window(center::LABEL)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    center::Seen {
        bar: bar.filter(|_| bar::is_open(app)),
        window: window.filter(|_| focused),
    }
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

/// The window view a tray item opens once linked, if it opens one.
fn tray_target(id: &str) -> Option<center::CenterTarget> {
    match id {
        "apps" => Some(center::CenterTarget::Apps),
        "memories" => Some(center::CenterTarget::Memories),
        "routines" => Some(center::CenterTarget::Routines),
        "skills" => Some(center::CenterTarget::Skills),
        "settings" => Some(center::CenterTarget::Settings),
        _ => None,
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
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
    }
}
