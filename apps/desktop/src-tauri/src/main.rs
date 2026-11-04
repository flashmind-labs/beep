//! Beep for macOS. A tray app whose bar opens on a double tap of Option.

mod agents;
mod bar;
mod beep;
mod center;
mod chat;
mod error;
mod machines;
mod notices;
mod palette;
mod platform;
mod projects;
mod research;
mod settings;
mod tray;
mod voice;

use anyhow::Context;
use beep::{
    Beep,
    state::{LinkState, StateFile},
};
use iroh::EndpointId;
use std::sync::Arc;
use tauri::{ActivationPolicy, App, AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_deep_link::DeepLinkExt;
use voice::Voice;

/// The server's node id, baked in by build.rs.
const SERVER_NODE_ID: &str = env!("BEEP_SERVER_NODE_ID");
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
        .manage(agents::artifacts::Pages::default())
        .register_asynchronous_uri_scheme_protocol(
            agents::artifacts::SCHEME,
            |ctx, request, responder| {
                let app = ctx.app_handle();
                match agents::artifacts::route(&app.state::<agents::artifacts::Pages>(), &request) {
                    agents::artifacts::Route::Done(response) => responder.respond(response),
                    // A file beside a page is read from its machine first.
                    agents::artifacts::Route::Read(file) => {
                        let beep = app
                            .try_state::<Arc<Beep>>()
                            .map(|beep| beep.inner().clone());
                        tauri::async_runtime::spawn(async move {
                            let response = match beep {
                                Some(beep) => {
                                    file.serve(|at| async move {
                                        agents::artifacts::read_sibling(&beep, at).await
                                    })
                                    .await
                                }
                                None => agents::artifacts::unavailable(),
                            };
                            responder.respond(response);
                        });
                    }
                }
            },
        )
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
                research::closed(window.app_handle(), window.label());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            machines::add::create_node_token,
            platform::editors::editors,
            platform::editors::open_in_editor,
            machines::add::revoke_node_token,
            machines::add::node_token_machine,
            machines::add::ssh_hosts,
            machines::add::add_machine_over_ssh,
            machines::add::machine_search,
            machines::add::set_machine_search,
            agents::sessions::agent_hosts,
            agents::sessions::agent_sessions,
            agents::sessions::agent_start,
            agents::sessions::agent_preview,
            agents::sessions::agent_models,
            agents::sessions::agent_view,
            agents::sessions::agent_image,
            agents::sessions::agent_file,
            agents::sessions::agent_file_open,
            agents::sessions::agent_file_save,
            agents::files::agent_tree,
            agents::files::agent_tree_text,
            agents::files::agent_tree_save,
            agents::files::agent_tree_upload,
            agents::diff::work_diff,
            agents::diff::branch_diff,
            agents::artifacts::agent_artifact,
            agents::custom::custom_agents,
            agents::sessions::agent_steer,
            center::open_center,
            center::center_home,
            center::center_cached,
            center::center_keep_pages,
            center::center_close_window,
            center::center_answer_agent,
            center::center_dismiss_updates,
            center::center_open_update_chat,
            center::center_open,
            center::center_pending,
            center::center_open_project_window,
            center::center_window_project,
            palette::center_palette,
            palette::center_palette_branches,
            palette::center_palette_prs,
            palette::center_palette_sessions,
            palette::center_find,
            bar::commands::status,
            bar::commands::link_offer,
            bar::commands::send_prompt,
            bar::pasted_image::read_pasted_image,
            bar::commands::rerun_prompt,
            bar::commands::answer_approval,
            bar::commands::active_run,
            bar::commands::active_runs,
            bar::commands::follow_run,
            bar::commands::cancel_run,
            bar::commands::list_sessions,
            bar::commands::search_sessions,
            bar::commands::open_session,
            bar::commands::session_files,
            bar::commands::session_sources,
            bar::commands::new_chat,
            bar::commands::rename_session,
            bar::commands::pin_session,
            bar::commands::delete_session,
            bar::commands::share_session,
            bar::commands::unshare_session,
            bar::commands::list_groups,
            bar::commands::open_group,
            bar::commands::close_group,
            bar::commands::send_to_group,
            bar::commands::create_group,
            bar::commands::group_invite,
            bar::commands::peek_group,
            bar::commands::join_group,
            bar::commands::leave_group,
            bar::commands::pin_group,
            bar::commands::share_group_apps,
            bar::commands::list_group_reminders,
            bar::commands::delete_group_reminder,
            bar::commands::answer_group_approval,
            notices::work::answer_work,
            notices::work::list_questions,
            settings::grants::list_grants,
            settings::grants::revoke_grant,
            machines::list::list_machines,
            machines::list::remove_machine,
            machines::list::updatable_machines,
            machines::list::update_machine,
            machines::tools::tool_updates,
            machines::tools::update_machine_tool,
            settings::credentials::credential_providers,
            settings::credentials::list_credentials,
            settings::credentials::save_credential,
            settings::credentials::remove_credential,
            settings::credentials::check_credential,
            settings::credentials::list_github_accounts,
            settings::credentials::set_default_github_account,
            settings::cli_login::cli_logins,
            settings::cli_login::use_cli_login,
            settings::cli_login::use_gh_account,
            settings::gh_sign_in::gh_sign_in,
            settings::gh_sign_in::cancel_gh_sign_in,
            agents::accounts::agent_accounts,
            agents::harnesses::agent_harnesses,
            agents::harnesses::harness_settings_save,
            agents::harnesses::harness_settings_all,
            agents::harnesses::harness_settings_save_all,
            agents::harnesses::agent_harness_update,
            agents::flsh_settings::flsh_settings_get,
            agents::flsh_settings::flsh_settings_save,
            agents::accounts::save_agent_token,
            agents::accounts::revoke_agent_token,
            agents::accounts::claude_sign_in,
            agents::accounts::cancel_claude_sign_in,
            agents::accounts::codex_login_start,
            agents::accounts::codex_login_state,
            agents::accounts::codex_login_cancel,
            projects::import_project,
            projects::list_machine_folder,
            projects::pick_folder,
            projects::found_projects,
            projects::host_repos,
            projects::project_sessions,
            projects::project_machines,
            projects::project_syncthing,
            projects::file_sync::project_sync,
            projects::file_sync::project_sync_detail,
            projects::file_sync::project_sync_pause,
            projects::file_sync::project_sync_resolve,
            projects::git_items::git_items,
            projects::open_folder,
            projects::center_projects,
            projects::projects_on_top,
            projects::groups::project_groups,
            projects::groups::project_group_save,
            projects::groups::project_group_delete,
            projects::overview::project_overview,
            agents::instructions::instructions_get,
            agents::instructions::instructions_save,
            agents::instructions::instructions_versions,
            agents::instructions::agent_files_read,
            agents::instructions::agent_files_sync,
            machines::terminals::terminals_list,
            machines::terminals::terminals_all,
            center::usage::usage_get,
            machines::terminals::terminal_start,
            machines::terminals::terminal_output,
            machines::terminals::terminal_close,
            machines::terminals::terminal_open,
            machines::terminals::terminal_attach,
            machines::terminals::terminal_input,
            machines::terminals::terminal_resize,
            machines::terminals::terminal_detach,
            projects::threads::center_project_issues,
            projects::threads::center_track_issue,
            projects::threads::center_project_threads,
            projects::threads::center_project_pull_requests,
            projects::threads::center_pull_request_head,
            projects::threads::center_reply_thread,
            projects::threads::center_open_thread_chat,
            projects::threads::center_thread_chats,
            projects::threads::center_thread_view,
            projects::threads::center_thread_detail,
            projects::runs::center_project_lists,
            projects::runs::center_project_runs,
            projects::runs::center_project_run,
            projects::live::follow_project,
            projects::live::unfollow_project,
            projects::reviews::project_branches,
            projects::branches::preview_import,
            projects::branches::find_import_fork,
            projects::branches::add_branch_worktree,
            projects::reviews::reviews_list,
            projects::reviews::review_start,
            projects::reviews::review_get,
            projects::reviews::review_check,
            projects::reviews::review_dismiss,
            projects::reviews::review_fix,
            projects::reviews::review_post,
            projects::reviews::review_merge,
            projects::pin_project_to_top,
            projects::remove_project,
            projects::center_open_project,
            projects::center_project,
            projects::center_project_send,
            projects::center_update_task,
            projects::center_stop_worker,
            agents::sessions::agent_resume,
            bar::commands::open_file,
            bar::commands::picture,
            bar::commands::save_file,
            bar::commands::save_picture,
            bar::commands::reveal_file,
            bar::commands::unlink,
            settings::apps::list_apps,
            settings::apps::connect_app,
            settings::apps::cancel_app_sign_in,
            settings::apps::disconnect_app,
            settings::apps::list_app_accounts,
            settings::apps::remove_app_account,
            settings::apps::app_permissions,
            settings::apps::set_app_permissions,
            settings::apps::list_mcp_servers,
            settings::apps::add_mcp_server,
            settings::apps::connect_mcp_server,
            settings::apps::remove_mcp_server,
            settings::apps::list_mcp_tools,
            settings::apps::set_mcp_tool,
            beep::devices::use_here,
            beep::devices::list_devices,
            beep::devices::invite_phone,
            beep::devices::remove_device,
            beep::devices::join_account,
            bar::commands::settings,
            bar::commands::set_autostart,
            bar::commands::mac_control,
            bar::commands::set_mac_control,
            bar::commands::allow_mac_permission,
            bar::commands::set_notice_voice,
            bar::commands::notice_card,
            bar::commands::play_notice,
            bar::commands::open_notice,
            bar::commands::dismiss_notice,
            bar::commands::hold_notice,
            bar::commands::live_tap,
            bar::commands::leave_live,
            bar::commands::cancel_live,
            bar::commands::show_bar,
            bar::commands::live_approval,
            bar::commands::open_approval,
            bar::commands::fit_orb,
            bar::commands::live_sheet,
            bar::commands::set_send_on_pause,
            bar::commands::set_agent_notices,
            bar::commands::live_chats,
            bar::commands::start_over,
            bar::commands::resize_bar,
            bar::commands::hide_bar,
            bar::commands::quit,
            bar::commands::open_url,
            bar::commands::open_log,
            platform::logfile::log_webview,
            bar::commands::group_typing,
            bar::commands::list_memories,
            bar::commands::list_memory_scopes,
            bar::commands::list_memory_reads,
            bar::commands::update_memory,
            bar::commands::delete_memory,
            bar::commands::clear_memories,
            bar::commands::list_crons,
            bar::commands::create_cron,
            bar::commands::toggle_cron,
            bar::commands::update_cron,
            bar::commands::delete_cron,
            bar::commands::run_cron,
            bar::commands::list_skills,
            bar::commands::update_skill,
            bar::commands::delete_skill,
            bar::commands::undo_skill,
            bar::commands::get_agents,
            bar::commands::set_agents,
            bar::commands::session_agents,
            bar::commands::get_models,
            bar::commands::stock_chart,
            bar::commands::route_card,
            bar::commands::set_model,
            bar::commands::set_thinking,
            bar::commands::set_auto_model,
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

fn setup(app: &mut App) -> anyhow::Result<()> {
    app.set_activation_policy(ActivationPolicy::Accessory);
    let handle = app.handle().clone();
    let dir = beep::identity::data_dir(&handle)?;
    let server: EndpointId = SERVER_NODE_ID
        .parse()
        .context("built without a server node id; set BEEP_NODE_ID")?;
    let identity = beep::identity::load_or_create(&dir.join("identity.key"))?;
    let file = StateFile::new(dir.join("state.json"));
    let chats = beep::sync::Chats::load(dir.join("sync"));
    let views = Arc::new(center::view_cache::ViewCache::load(
        dir.join("views.json"),
        env!("CARGO_PKG_VERSION"),
    ));
    app.manage(Arc::new(center::home_sessions::AgentSessions::new(&views)));
    app.manage(views);
    let (beep, notices) =
        tauri::async_runtime::block_on(Beep::start(identity, server, file, chats))?;
    app.manage(beep.clone());
    app.manage(tray::Recent::default());
    app.manage(projects::live::Followed::default());
    app.manage(research::Pending::default());
    app.manage(research::Cache::default());
    app.manage(settings::apps::SignIn::default());
    app.manage(settings::gh_sign_in::GhSignIn::default());
    app.manage(agents::accounts::ClaudeSignIn::default());
    app.manage(platform::update::Updates::default());
    app.manage(machines::this_mac::Machine::new(&dir.join("node")));
    app.manage(center::Shown::default());
    app.manage(center::AgentsShown::default());
    app.manage(center::Pending::default());
    app.manage(palette::PaletteCache::default());
    app.manage(tray::waiting::WaitingList::default());
    app.manage(machines::terminals::Attached::default());
    app.state::<machines::this_mac::Machine>().sync(&beep);
    let voice = Voice::new(handle.clone(), beep.clone());
    app.manage(voice.clone());

    tauri::async_runtime::spawn(notices::serve(
        handle.clone(),
        beep.clone(),
        voice.clone(),
        notices,
    ));
    tauri::async_runtime::spawn(beep::sync::follow(handle.clone(), beep.clone()));
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
