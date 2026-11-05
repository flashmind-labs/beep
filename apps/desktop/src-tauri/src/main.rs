//! Beep for macOS. A tray app whose bar opens on a double tap of Option.

mod agents;
mod bar;
mod beep;
mod center;
mod chat;
mod error;
mod fleet;
mod machines;
mod notices;
mod palette;
mod platform;
mod projects;
mod research;
mod settings;
mod tray;
mod voice;

use agents::artifacts;
use anyhow::Context;
use beep::{Beep, state::StateFile};
use beep_proto::client::bind_endpoint;
use fleet::Fleet;
use iroh::EndpointId;
use std::{sync::Arc, time::Duration};
use tauri::{ActivationPolicy, App, AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_deep_link::DeepLinkExt;
use tokio::time::timeout;
use voice::Voice;

/// The server's node id, baked in by build.rs.
const SERVER_NODE_ID: &str = env!("BEEP_SERVER_NODE_ID");
/// How long quitting waits for the router and the node's sync and transfer
/// services to stop. The mesh writes each change to disk as it happens.
const FLEET_STOP: Duration = Duration::from_secs(3);
/// Passed to Beep when it starts as a login item, so it stays in the tray.
const LOGIN_ARG: &str = "--login";

fn main() {
    // An agent running the app as its Beep MCP server: no window, no log.
    if let Some(status) = machines::mcp_command::run(std::env::args_os().skip(1)) {
        std::process::exit(status);
    }
    let _log = platform::logfile::start();
    log::info!("Beep {} starting", env!("CARGO_PKG_VERSION"));
    // SAFETY: nothing else runs yet; Tauri starts its threads below.
    unsafe { platform::login_path::adopt() };
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![LOGIN_ARG]),
        ))
        .manage(artifacts::Pages::default())
        .register_asynchronous_uri_scheme_protocol(artifacts::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle();
            match artifacts::route(&app.state::<artifacts::Pages>(), &request) {
                artifacts::Route::Done(response) => responder.respond(response),
                // A file beside a page is read from its machine first.
                artifacts::Route::Read(file) => {
                    let beep = app
                        .try_state::<Arc<Beep>>()
                        .map(|beep| beep.inner().clone());
                    let fleet = app
                        .try_state::<Arc<Fleet>>()
                        .map(|fleet| fleet.inner().clone());
                    tauri::async_runtime::spawn(async move {
                        let response = match (beep, fleet) {
                            (Some(beep), Some(fleet)) => {
                                file.serve(|at| async move {
                                    artifacts::read_sibling(&beep, &fleet, at).await
                                })
                                .await
                            }
                            _ => artifacts::unavailable(),
                        };
                        responder.respond(response);
                    });
                }
            }
        })
        .setup(|app| Ok(setup(app)?))
        .on_window_event(|window, event| match event {
            // Only the bar is a popover; the other windows stay put. Live
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
            WindowEvent::CloseRequested { api, .. } => {
                if center::hide_on_close(window) {
                    api.prevent_close();
                }
            }
            WindowEvent::Destroyed => {
                if center::is_center(window.label()) {
                    // Its chat is not on screen any more, so its notices must
                    // not be held back as seen.
                    window
                        .app_handle()
                        .state::<center::Shown>()
                        .clear(window.label());
                }
                window
                    .app_handle()
                    .state::<center::AgentsShown>()
                    .clear(window.label());
                center::closed(window.app_handle(), window.label());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            agents::accounts::agent_accounts,
            agents::accounts::cancel_claude_sign_in,
            agents::accounts::claude_sign_in,
            agents::accounts::codex_login_cancel,
            agents::accounts::codex_login_start,
            agents::accounts::codex_login_state,
            agents::accounts::revoke_agent_token,
            agents::accounts::save_agent_token,
            agents::artifacts::agent_artifact,
            agents::custom::custom_agents,
            agents::diff::work_diff,
            agents::diff::agent_place,
            agents::diff::agent_worktrees,
            agents::diff::agent_commands,
            agents::files::agent_tree,
            agents::files::agent_tree_save,
            agents::files::agent_tree_text,
            agents::files::agent_tree_upload,
            agents::flsh_settings::flsh_settings_get,
            agents::flsh_settings::flsh_settings_save,
            agents::harnesses::agent_harness_update,
            agents::harnesses::agent_harnesses,
            agents::harnesses::harness_settings_all,
            agents::harnesses::harness_settings_save,
            agents::harnesses::harness_settings_save_all,
            agents::harnesses::harness_compact,
            agents::harnesses::harness_compact_save,
            agents::instructions::agent_files_read,
            agents::instructions::agent_files_sync,
            agents::instructions::instructions_get,
            agents::instructions::instructions_save,
            agents::instructions::instructions_versions,
            agents::media::agent_file,
            agents::media::agent_file_open,
            agents::media::agent_file_save,
            agents::media::agent_image,
            agents::media::agent_thumb,
            agents::sessions::agent_deletable,
            agents::sessions::agent_delete,
            agents::sessions::agent_hosts,
            agents::sessions::agent_models,
            agents::sessions::agent_preview,
            agents::sessions::agent_resume,
            agents::sessions::agent_sessions,
            agents::sessions::agent_start,
            agents::sessions::agent_usage,
            agents::sessions::session_folder_here,
            agents::sessions::agent_modes,
            agents::sessions::agent_tasks,
            agents::sessions::agent_view,
            agents::steer::agent_steer,
            bar::app::allow_mac_permission,
            bar::app::link_offer,
            bar::app::mac_control,
            bar::app::open_log,
            bar::app::open_url,
            bar::app::quit,
            bar::app::set_agent_notices,
            bar::app::set_autostart,
            bar::app::set_mac_control,
            bar::app::set_notice_voice,
            bar::app::settings,
            bar::app::start_over,
            bar::app::status,
            bar::app::unlink,
            bar::live::cancel_live,
            bar::live::dismiss_notice,
            bar::live::fit_orb,
            bar::live::hold_notice,
            bar::live::leave_live,
            bar::live::live_approval,
            bar::live::live_chats,
            bar::live::live_sheet,
            bar::live::live_tap,
            bar::live::notice_card,
            bar::live::open_approval,
            bar::live::open_notice,
            bar::live::play_notice,
            bar::live::set_send_on_pause,
            bar::pasted_image::read_pasted_image,
            bar::window::hide_bar,
            bar::window::resize_bar,
            bar::window::show_bar,
            beep::devices::invite_phone,
            beep::devices::join_account,
            beep::devices::list_devices,
            beep::devices::remove_device,
            beep::devices::use_here,
            beep::login::complete_login,
            beep::login::login_hosted,
            beep::login::start_login,
            beep::login::use_own_server,
            center::home::center_answer_agent,
            center::home::center_dismiss_updates,
            center::home::center_home,
            center::home::center_open_update_chat,
            center::targets::center_open,
            center::targets::center_pending,
            center::usage::usage_get,
            center::view_cache::center_cached,
            center::view_cache::center_keep_pages,
            center::window::center_close_window,
            center::window::center_open_project_window,
            center::window::center_window_project,
            center::window::open_center,
            chat::approvals::answer_approval,
            chat::files::open_file,
            chat::files::picture,
            chat::files::reveal_file,
            chat::files::save_file,
            chat::files::save_picture,
            chat::groups::answer_group_approval,
            chat::groups::close_group,
            chat::groups::create_group,
            chat::groups::delete_group_reminder,
            chat::groups::group_invite,
            chat::groups::group_typing,
            chat::groups::join_group,
            chat::groups::leave_group,
            chat::groups::list_group_reminders,
            chat::groups::list_groups,
            chat::groups::open_group,
            chat::groups::peek_group,
            chat::groups::pin_group,
            chat::groups::send_to_group,
            chat::groups::share_group_apps,
            chat::sessions::active_run,
            chat::sessions::active_runs,
            chat::sessions::cancel_run,
            chat::sessions::delete_session,
            chat::sessions::follow_run,
            chat::sessions::list_sessions,
            chat::sessions::new_chat,
            chat::sessions::open_session,
            chat::sessions::pin_session,
            chat::sessions::rename_session,
            chat::sessions::rerun_prompt,
            chat::sessions::route_card,
            chat::sessions::search_sessions,
            chat::sessions::send_prompt,
            chat::sessions::session_agents,
            chat::sessions::session_files,
            chat::sessions::session_sources,
            chat::sessions::share_session,
            chat::sessions::stock_chart,
            chat::sessions::unshare_session,
            machines::add::mesh_join,
            machines::add::mesh_invite,
            machines::add::machine_search,
            machines::add::node_token_machine,
            machines::add::set_machine_search,
            machines::list::list_machines,
            machines::list::machine_log,
            machines::list::machine_traffic,
            machines::list::machine_clocks,
            machines::list::mesh_remove,
            machines::list::remove_machine,
            machines::list::save_machine_log,
            machines::list::updatable_machines,
            machines::list::update_machine,
            machines::terminals::terminal_attach,
            machines::terminals::terminal_close,
            machines::terminals::terminal_detach,
            machines::terminals::terminal_input,
            machines::terminals::terminal_open,
            machines::terminals::terminal_output,
            machines::terminals::terminal_resize,
            machines::terminals::terminal_start,
            machines::terminals::terminals_all,
            machines::terminals::terminals_list,
            machines::tools::tool_updates,
            machines::tools::update_machine_tool,
            notices::work::answer_work,
            notices::work::list_questions,
            palette::index::center_find_text,
            palette::lists::center_palette_index,
            palette::lists::center_palette_refresh,
            platform::editors::editors,
            platform::editors::open_in_editor,
            platform::logfile::log_webview,
            projects::branches::add_branch_worktree,
            projects::branches::find_import_fork,
            projects::branches::preview_import,
            projects::file_sync::project_sync,
            projects::file_sync::project_sync_detail,
            projects::file_sync::project_sync_pause,
            projects::file_sync::project_sync_resolve,
            projects::git_items::git_items,
            projects::groups::project_group_delete,
            projects::groups::project_group_save,
            projects::groups::project_groups,
            projects::import::found_projects,
            projects::import::host_repos,
            projects::create::create_project,
            projects::create::project_forges,
            projects::import::import_project,
            projects::import::list_machine_folder,
            projects::import::open_folder,
            projects::import::pick_folder,
            projects::list::center_projects,
            projects::list::pin_project_to_top,
            projects::list::projects_on_top,
            projects::list::remove_project,
            projects::live::follow_project,
            projects::live::unfollow_project,
            projects::overview::project_overview,
            projects::page::center_open_project,
            projects::page::center_project,
            projects::page::center_project_send,
            projects::page::center_stop_worker,
            projects::page::center_update_task,
            projects::page::project_copy,
            projects::page::project_machines,
            projects::page::project_sessions,
            projects::page::project_syncthing,
            projects::reviews::project_branches,
            projects::reviews::review_check,
            projects::reviews::review_dismiss,
            projects::reviews::review_fix,
            projects::reviews::review_get,
            projects::reviews::review_merge,
            projects::reviews::review_post,
            projects::reviews::review_start,
            projects::reviews::reviews_list,
            projects::runs::center_project_lists,
            projects::runs::center_project_run,
            projects::runs::center_project_runs,
            projects::threads::center_open_thread_chat,
            projects::threads::center_project_issues,
            projects::threads::center_project_pull_requests,
            projects::threads::center_project_threads,
            projects::threads::center_pull_request_head,
            projects::threads::center_reply_thread,
            projects::threads::center_thread_chats,
            projects::threads::center_thread_detail,
            projects::threads::center_thread_view,
            projects::threads::center_track_issue,
            research::commands::export_research,
            research::commands::list_researches,
            research::commands::open_research,
            research::commands::research_begin,
            research::commands::research_history,
            research::commands::research_pending,
            research::commands::research_send,
            research::commands::research_sessions,
            research::commands::stop_research,
            research::view::cached_research,
            research::view::get_research,
            settings::apps::add_mcp_server,
            settings::apps::app_permissions,
            settings::apps::cancel_app_sign_in,
            settings::apps::connect_app,
            settings::apps::connect_mcp_server,
            settings::apps::disconnect_app,
            settings::apps::list_app_accounts,
            settings::apps::list_apps,
            settings::apps::list_mcp_servers,
            settings::apps::list_mcp_tools,
            settings::apps::remove_app_account,
            settings::apps::remove_mcp_server,
            settings::apps::set_app_permissions,
            settings::apps::set_mcp_tool,
            settings::cli_login::cli_logins,
            settings::cli_login::use_cli_login,
            settings::cli_login::use_gh_account,
            settings::credentials::check_credential,
            settings::credentials::credential_providers,
            settings::credentials::list_credentials,
            settings::credentials::list_github_accounts,
            settings::credentials::remove_credential,
            settings::credentials::save_credential,
            settings::credentials::set_default_github_account,
            settings::gh_sign_in::cancel_gh_sign_in,
            settings::gh_sign_in::gh_sign_in,
            settings::grants::list_grants,
            settings::grants::revoke_grant,
            settings::memories::clear_memories,
            settings::memories::delete_memory,
            settings::memories::list_memories,
            settings::memories::list_memory_reads,
            settings::memories::list_memory_scopes,
            settings::memories::update_memory,
            settings::routines::create_cron,
            settings::routines::delete_cron,
            settings::routines::list_crons,
            settings::routines::run_cron,
            settings::routines::toggle_cron,
            settings::routines::update_cron,
            settings::skills::delete_skill,
            settings::skills::list_skills,
            settings::skills::undo_skill,
            settings::skills::update_skill,
            settings::models::get_agents,
            settings::models::get_models,
            settings::models::set_agents,
            settings::models::set_auto_model,
            settings::models::set_model,
            settings::models::set_thinking,
        ])
        .build(tauri::generate_context!())
        .expect("the app failed to start")
        .run(|app, event| {
            match event {
                // Closing the last window leaves Beep in the menu bar; only
                // Quit (an exit with a code) ends it.
                RunEvent::ExitRequested {
                    code: None, api, ..
                } => api.prevent_exit(),
                // However Beep quits, a downloaded update is installed for next time.
                RunEvent::Exit => {
                    platform::update::install(app);
                    // gh, if it is still waiting for a GitHub code, goes too.
                    app.state::<settings::gh_sign_in::GhSignIn>().stop_on_quit();
                    app.state::<agents::accounts::ClaudeSignIn>().stop();
                    // The router stops, and the sync and transfer services save their state.
                    let fleet = app.state::<Arc<Fleet>>().inner().clone();
                    tauri::async_runtime::block_on(async move {
                        if timeout(FLEET_STOP, fleet.stop()).await.is_err() {
                            log::warn!("the mesh did not stop in time");
                        }
                    });
                    // Background tasks the agent started here end with the app.
                    beep_node::kill_tasks();
                }
                // Opening Beep again while it runs shows the window.
                #[cfg(target_os = "macos")]
                RunEvent::Reopen { .. } => {
                    if let Err(e) = center::open(app) {
                        log::warn!("opening the window: {e}");
                    }
                }
                _ => {}
            }
        });
}

fn setup(app: &mut App) -> anyhow::Result<()> {
    beep_node::machine::raise_open_files();
    app.set_activation_policy(ActivationPolicy::Accessory);
    let handle = app.handle().clone();
    let dir = beep::identity::data_dir(&handle)?;
    let identity = beep::identity::load_or_create(&dir.join("identity.key"))?;
    let file = StateFile::new(dir.join("state.json"));
    // The server chosen with a link code, or the one built in; the app
    // works without one, in the mesh.
    let server: Option<EndpointId> = match file.load().server {
        Some(server) => Some(server),
        None if SERVER_NODE_ID.is_empty() => None,
        None => Some(
            SERVER_NODE_ID
                .parse()
                .context("the server node id built in is not valid")?,
        ),
    };
    let chats = beep::sync::Chats::load(dir.join("sync"));
    let views = Arc::new(center::view_cache::ViewCache::load(
        dir.join("views.json"),
        env!("CARGO_PKG_VERSION"),
    ));
    app.manage(Arc::new(center::home_sessions::AgentSessions::new(&views)));
    app.manage(palette::PaletteCache::load(&views)?);
    app.manage(views);
    let endpoint = tauri::async_runtime::block_on(bind_endpoint(
        beep::identity::secret_key(&identity),
        beep_mesh::transport_config(),
    ))?;
    let (beep, notices) = Beep::start(endpoint.clone(), identity, server, file, chats);
    app.manage(beep.clone());
    // This Mac's node, and the mesh it is in, which opens in the background.
    let machine = machines::this_mac::Machine::new(&dir.join("node"), beep.linked());
    let fleet = Arc::new(Fleet::new(
        endpoint,
        &dir.join("node"),
        machine.name(),
        machine.offers(),
    ));
    app.manage(fleet.clone());
    // What the projects pages read, kept for the run of the app.
    app.manage(beep_fleet::Session::new(
        fleet.clone(),
        beep_fleet::Options::default(),
        projects::live::changed(handle.clone()),
    )?);
    let (opening, push) = (fleet.clone(), beep.push());
    tauri::async_runtime::spawn(async move { opening.run(push).await });
    // An account with a server moves into the mesh in the background, after Home has drawn.
    fleet::migrate::start(handle.clone());
    app.manage(machine);
    app.manage(machines::list::Outside::default());
    app.manage(tray::Recent::default());
    app.manage(projects::live::Followed::default());
    app.manage(research::Pending::default());
    app.manage(research::Cache::default());
    app.manage(settings::apps::SignIn::default());
    app.manage(settings::gh_sign_in::GhSignIn::default());
    app.manage(agents::accounts::ClaudeSignIn::default());
    app.manage(platform::update::Updates::default());
    app.manage(center::Shown::default());
    app.manage(center::AgentsShown::default());
    app.manage(center::Pending::default());
    app.manage(tray::waiting::WaitingList::default());
    app.manage(machines::terminals::Attached::default());
    tauri::async_runtime::spawn(machines::list::retry_unlinks(beep.clone()));
    app.state::<machines::this_mac::Machine>().start();
    app.state::<machines::this_mac::Machine>().sync(&beep);
    let voice = Voice::new(handle.clone(), beep.clone());
    app.manage(voice.clone());

    let (following, direct) = fleet::alerts::start(fleet.clone());
    tauri::async_runtime::spawn(following);
    tauri::async_runtime::spawn(notices::serve(
        handle.clone(),
        beep.clone(),
        voice.clone(),
        notices,
        direct,
    ));
    tauri::async_runtime::spawn(beep::sync::follow(handle.clone(), beep.clone()));
    tauri::async_runtime::spawn(palette::lists::follow(handle.clone(), beep.clone()));
    tauri::async_runtime::spawn(platform::update::follow(handle.clone()));
    tauri::async_runtime::spawn(tray::waiting::follow(handle.clone(), beep.clone()));
    voice::hotkey::start(move |input| voice.on_input(input));

    // Sign-ins come back as beep:// links, while running or as the reason
    // macOS started Beep.
    let links = handle.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            settings::apps::open_link(&links, url.as_str());
        }
    });
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            settings::apps::open_link(&handle, url.as_str());
        }
    }

    tray::install(app, &beep)?;

    // The app works with no subscription, so the first launch lands on Home like any other.
    if !std::env::args().any(|arg| arg == LOGIN_ARG) {
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
