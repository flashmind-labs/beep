//! Beep desktop. A tray app whose bar opens on a double tap of Option.

mod agents;
mod bar;
mod beep;
mod center;
mod chat;
mod error;
mod fleet;
mod machines;
mod navigation;
mod notices;
mod palette;
mod platform;
mod plugins;
mod projects;
mod report;
mod settings;
mod tray;

use agents::artifacts;
use beep::state::{Prefs, StateFile};
use beep_proto::port::bind_endpoint;
use fleet::Fleet;
#[cfg(desktop)]
use metrics_exporter_prometheus::PrometheusHandle;
use std::{sync::Arc, time::Duration};
#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;
use tauri::{App, AppHandle, Manager, RunEvent, WindowEvent};
#[cfg(desktop)]
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
#[cfg(desktop)]
use tauri_plugin_window_state::StateFlags;
use tokio::time::timeout;

#[cfg(desktop)]
const WINDOW_GEOMETRY: StateFlags = StateFlags::POSITION
    .union(StateFlags::SIZE)
    .union(StateFlags::MAXIMIZED);

/// How long quitting waits for what saves state: the node's sync and
/// transfer services, and gh being killed. The mesh writes each change to
/// disk as it happens; goodbyes to peers still going at this point are cut.
const QUIT_LIMIT: Duration = Duration::from_millis(300);
/// Passed to Beep when it starts as a login item, so it stays in the tray.
#[cfg(desktop)]
const LOGIN_ARG: &str = "--login";
/// How long after it starts Beep makes the bar: after Home has drawn.
#[cfg(desktop)]
const BAR_AFTER: Duration = Duration::from_secs(1);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // An agent running the app as its Beep MCP server: no window, no log.
    #[cfg(desktop)]
    if let Some(status) = machines::mcp_command::run(std::env::args_os().skip(1)) {
        std::process::exit(status);
    }

    let _log = platform::logfile::start();
    log::info!("Beep {} starting", env!("CARGO_PKG_VERSION"));
    platform::startup::stage("main");
    // Made beside the app's start, off the main thread: see `diagnostics::install`.
    #[cfg(desktop)]
    let recorder = std::thread::spawn(beep_node::link::diagnostics::install);
    let context = tauri::generate_context!();
    // SAFETY: nothing else runs yet; Tauri starts its threads below.
    #[cfg(all(unix, desktop))]
    if let Some(refresh) = unsafe { platform::login_path::adopt(&context.config().identifier) } {
        std::thread::spawn(move || refresh.run());
    }
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.manage(chat::dictation::Dictations::default());
    #[cfg(target_os = "windows")]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        if let Err(e) = center::open(app) {
            log::warn!("opening the existing window: {e}");
        }
    }));
    #[cfg(all(target_os = "macos", feature = "sparkle"))]
    let builder = if cfg!(debug_assertions) {
        builder
    } else {
        builder.plugin(tauri_plugin_sparkle_updater::init())
    };
    #[cfg(all(desktop, not(target_os = "macos")))]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_autostart::init(
        MacosLauncher::LaunchAgent,
        Some(vec![LOGIN_ARG]),
    ));
    // Command center windows open where they were last, on a screen still
    // attached; the bar places itself.
    #[cfg(desktop)]
    let builder = builder.plugin(
        tauri_plugin_window_state::Builder::new()
            .with_state_flags(WINDOW_GEOMETRY)
            .with_filter(center::is_center)
            .build(),
    );
    // The camera that scans the pairing QR code.
    #[cfg(target_os = "ios")]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_qr_scan::init());
    // The tick of a board lane settling on a card.
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_haptics::init());
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_notification::init());
    builder
        .plugin(tauri_plugin_opener::init())
        .manage(artifacts::Pages::default())
        .manage(machines::forwards::Forwards::default())
        .manage(Arc::new(chat::clipboard::ImageClipboard::default()))
        .register_asynchronous_uri_scheme_protocol(artifacts::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle();

            match artifacts::route(&app.state::<artifacts::Pages>(), &request) {
                artifacts::Route::Done(response) => responder.respond(response),
                // A file beside a page is read from its machine first.
                artifacts::Route::Read(file) => {
                    let fleet = app
                        .try_state::<Arc<Fleet>>()
                        .map(|fleet| fleet.inner().clone());
                    tauri::async_runtime::spawn(async move {
                        let response = match fleet {
                            Some(fleet) => {
                                file.serve(
                                    |at| async move { artifacts::read_sibling(&fleet, at).await },
                                )
                                .await
                            }
                            None => artifacts::unavailable(),
                        };
                        responder.respond(response);
                    });
                }
            }
        })
        .setup(move |app| {
            #[cfg(desktop)]
            let started = setup(app, recorder);
            #[cfg(mobile)]
            let started = setup(app);
            Ok(started?)
        })
        .on_page_load(|webview, _| bar::zoom::on_load(webview))
        .on_window_event(|window, event| match event {
            // Only the bar is a popover; the other windows stay put.
            WindowEvent::Focused(false)
                if window.label() == bar::LABEL && window.is_visible().unwrap_or(false) =>
            {
                bar::hide(window.app_handle());
            }
            #[cfg(mobile)]
            WindowEvent::Suspended => notices::notify::set_foreground(false),
            #[cfg(mobile)]
            WindowEvent::Resumed => notices::notify::set_foreground(true),
            WindowEvent::CloseRequested { api, .. } => {
                #[cfg(desktop)]
                if !platform::update::relaunching(window.app_handle()) {
                    center::window_session::closing(window);
                }
                if center::hide_on_close(window) {
                    api.prevent_close();
                }
            }
            WindowEvent::Destroyed => {
                let app = window.app_handle();
                let label = window.label();
                app.state::<center::AgentsShown>().clear(label);
                app.state::<agents::follow::Follows>().clear(label);
                app.state::<fleet::events::Events>().clear(label);
                app.state::<projects::live::Followed>().clear(label);
                app.state::<projects::team_live::TeamLive>()
                    .clear(app, label);
                app.state::<fleet::session_lists::SessionLists>()
                    .closed(label);
                app.state::<machines::terminals::Attached>().clear(label);
                app.state::<machines::devices::Attached>().clear(label);
                #[cfg(desktop)]
                app.state::<chat::dictation::Dictations>().clear(label);
                center::window::forget(app, label);
                center::closed(app, label);
            }
            // Minimizing, covering and backgrounding pause the page.
            _ => center::window::on_window_event(window, event),
        })
        .invoke_handler(counted(tauri::generate_handler![
            agents::accounts::agent_accounts,
            agents::accounts::cancel_claude_sign_in,
            agents::accounts::claude_sign_in,
            agents::accounts::codex_login_cancel,
            agents::accounts::codex_login_start,
            agents::accounts::codex_login_state,
            agents::accounts::revoke_agent_token,
            agents::accounts::agent_account_names,
            agents::stash::prompt_stash_list,
            agents::stash::prompt_stash_put,
            agents::stash::prompt_stash_read,
            agents::stash::prompt_stash_take,
            agents::stash::prompt_stash_remove,
            agents::accounts::save_agent_token,
            agents::artifacts::agent_artifact,
            agents::artifacts::agent_artifacts_present,
            agents::artifacts::agent_page_files,
            agents::artifacts::agent_preview_export,
            agents::custom::custom_agents,
            agents::diff::work_diff,
            agents::review::agent_review_start,
            agents::review::agent_review,
            agents::review::agent_reviews,
            agents::diff::agent_places,
            agents::diff::agent_worktrees,
            agents::diff::agent_commands,
            agents::processes::agent_stop_process,
            agents::processes::agent_stop_container,
            machines::forwards::machine_forward_port,
            agents::processes::agent_close_external,
            agents::moving::session_move,
            agents::moving::chat_move,
            agents::moving::session_move_cancel,
            agents::moving::session_move_probe,
            agents::moving::session_move_progress,
            agents::moving::session_moves,
            agents::files::agent_tree,
            agents::files::agent_tree_edit,
            agents::files::agent_lsp_definition,
            agents::files::agent_tree_save_many,
            agents::files::agent_tree_save,
            agents::files::agent_tree_text,
            agents::files::agent_file_hashes,
            agents::files::agent_tree_upload,
            agents::files::agent_discard_draft_attachment,
            agents::flsh_settings::flsh_settings_get,
            agents::flsh_settings::flsh_settings_save,
            agents::harnesses::agent_adapters_install,
            agents::harnesses::agent_harness_update,
            agents::harnesses::agent_harnesses,
            agents::harnesses::harness_settings_all,
            agents::harnesses::harness_settings_save,
            agents::harnesses::harness_settings_save_all,
            agents::harnesses::harness_compact,
            agents::harnesses::harness_compact_save,
            agents::harnesses::harness_default_model,
            agents::harnesses::harness_driver,
            agents::harnesses::harness_driver_save,
            agents::harnesses::project_harnesses,
            agents::harnesses::project_harnesses_save,
            agents::harnesses::harness_default_model_save,
            agents::harnesses::harness_model_compacts,
            agents::harnesses::harness_model_compacts_save,
            agents::instructions::instructions_get,
            agents::instructions::instructions_save,
            agents::instructions::instructions_versions,
            agents::media::agent_file,
            agents::media::agent_file_open,
            agents::media::agent_file_save,
            agents::media::agent_files_forget,
            agents::media::agent_image,
            agents::media::agent_thumb,
            agents::sessions::agent_deletable,
            agents::sessions::agent_delete,
            agents::sessions::agent_relocate,
            fleet::agents::agent_hosts,
            agents::sessions::agent_models,
            agents::sessions::agent_resume,
            agents::sessions::agent_sessions,
            agents::sessions::agent_title_save,
            agents::sessions::agent_start,
            agents::starts::agent_begin,
            agents::starts::agent_begin_retry,
            agents::sessions::agent_device,
            agents::sessions::session_folder_here,
            agents::local_file::local_file,
            agents::local_file::open_local_file,
            agents::sessions::agent_modes,
            agents::sessions::agent_tasks,
            agents::sessions::agent_view,
            agents::sessions::agent_older,
            agents::kept_transcripts::transcript_kept,
            agents::kept_transcripts::transcript_forget,
            agents::follow::agent_follow,
            fleet::events::mesh_watch,
            fleet::events::mesh_topics,
            agents::follow::agent_unfollow,
            agents::follow::agent_follow_show,
            agents::steer::agent_steer,
            agents::outbox::outbox_list,
            agents::outbox::outbox_take,
            agents::outbox::outbox_attach,
            agents::outbox::outbox_cancel,
            agents::outbox::outbox_drain,
            bar::app::allow_mac_permission,
            bar::app::mac_control,
            bar::app::open_url,
            bar::app::build_capabilities,
            bar::app::quit,
            bar::app::set_agent_notices,
            bar::app::set_node_auto_update,
            bar::app::set_developer_mode,
            bar::app::set_save_folder,
            bar::app::use_downloads,
            bar::app::set_autostart,
            bar::app::set_mac_control,
            bar::app::settings,
            bar::pasted_image::read_pasted_image,
            chat::clipboard::read_clipboard_image,
            #[cfg(desktop)]
            chat::dictation::dictation_start,
            #[cfg(desktop)]
            chat::dictation::dictation_audio,
            #[cfg(desktop)]
            chat::dictation::dictation_stop,
            #[cfg(desktop)]
            chat::dictation::dictation_cancel,
            bar::window::hide_bar,
            bar::window::resize_bar,
            bar::zoom::set_zoom_level,
            bar::zoom::zoom_level,
            center::home::notes_answer,
            notices::history::notes_dismiss,
            notices::history::skill_keep,
            notices::history::skill_reveal,
            notices::history::notes_list,
            notices::history::notes_read,
            center::home::center_answer_agent,
            center::home::center_home,
            center::home_frame::keep_home_frame,
            center::targets::center_open,
            center::targets::center_pending,
            center::usage::usage_get,
            center::view_cache::center_cached,
            center::view_cache::center_keep_pages,
            center::window::center_close_window,
            center::window::center_open_window,
            center::window::center_window_alone,
            chat::files::copy_diagram_image,
            chat::files::reveal_file,
            chat::files::save_picture,
            fleet::phone::mobile_paired,
            fleet::phone::mobile_pair,
            fleet::phone::mobile_network_changed,
            machines::add::mesh_join,
            machines::add::mesh_invite,
            machines::add::machine_search,
            machines::mcp_servers::machine_mcp_harnesses,
            machines::mcp_servers::machine_mcp_servers,
            machines::mcp_servers::save_machine_mcp_server,
            machines::mcp_servers::remove_machine_mcp_server,
            machines::mcp_servers::apply_machine_mcp_server,
            machines::add::ticket_machine,
            bar::app::open_log,
            machines::add::set_machine_search,
            machines::list::list_machines,
            machines::tools::machine_log,
            machines::list::machine_clocks,
            machines::list::node_release,
            machines::list::remove_machine,
            machines::list::rename_machine,
            machines::list::save_machine_log,
            machines::tools::update_machine,
            machines::devices::device_attach,
            machines::devices::device_boot,
            machines::devices::device_detach,
            machines::devices::device_drawn,
            machines::list::device_hosts,
            machines::devices::device_input,
            machines::devices::device_list,
            machines::devices::device_platforms,
            machines::devices::device_shutdown,
            machines::terminals::terminal_attach,
            machines::terminals::terminal_close,
            machines::terminals::terminal_detach,
            machines::terminals::terminal_input,
            machines::terminals::terminal_open,
            machines::terminals::terminal_output,
            machines::terminals::terminal_resize,
            machines::terminals::terminals_all,
            machines::terminals::terminals_list,
            machines::tools::update_machine_tool,
            machines::tools::machine_set_up,
            machines::sign_in::machine_sign_in,
            palette::search::center_find_prompts,
            palette::lists::center_palette_index,
            palette::lists::center_palette_refresh,
            platform::editors::editors,
            platform::editors::open_in_editor,
            platform::logfile::log_webview,
            platform::startup::startup_stages,
            platform::metrics::desktop_timings,
            platform::metrics::desktop_trace_enabled,
            platform::metrics::desktop_ui_metrics,
            plugins::command::plugins_placements,
            plugins::command::plugins_publish,
            plugins::command::plugins_instances,
            plugins::command::plugins_event,
            plugins::command::plugins_wanted,
            plugins::command::plugins_asset,
            plugins::command::plugins_visible,
            plugins::command::plugins_status,
            #[cfg(desktop)]
            plugins::manage::plugins_enable,
            #[cfg(desktop)]
            plugins::manage::plugins_review,
            #[cfg(desktop)]
            plugins::manage::plugins_allow,
            #[cfg(desktop)]
            plugins::manage::plugins_remove,
            #[cfg(desktop)]
            plugins::manage::plugins_stage_git,
            #[cfg(desktop)]
            plugins::manage::plugins_stage_folder,
            #[cfg(desktop)]
            plugins::manage::plugins_check_update,
            #[cfg(desktop)]
            plugins::manage::plugins_stage_update,
            #[cfg(desktop)]
            plugins::manage::plugins_confirm,
            #[cfg(desktop)]
            plugins::manage::plugins_cancel,
            #[cfg(desktop)]
            plugins::manage::plugins_open_folder,
            projects::branches::find_import_fork,
            projects::branches::preview_import,
            projects::file_sync::project_sync,
            projects::file_sync::project_syncthing_files,
            projects::file_sync::syncthing_in_use,
            projects::git_sync::project_git_move,
            projects::git_sync::project_git_switch,
            projects::file_sync::project_handoff_work,
            projects::file_sync::project_git_auto_recovery,
            projects::file_sync::project_syncthing_mode,
            projects::file_sync::project_ignored_sync,
            projects::file_sync::project_sync_rules,
            projects::git_sync::project_git_state,
            projects::file_sync::project_sync_detail,
            projects::file_sync::project_sync_pause,
            projects::file_sync::project_sync_resolve,
            projects::file_sync::project_sync_start_over,
            projects::git_items::git_items,
            projects::groups::project_group_delete,
            projects::groups::project_group_save,
            projects::groups::project_groups,
            projects::import::found_projects,
            projects::import::found_machines,
            projects::import::rescan_found,
            projects::import::host_repos,
            projects::create::create_project,
            projects::create::project_forges,
            projects::import::import_project,
            projects::import::list_machine_folder,
            projects::import::list_machine_tree,
            projects::import::open_folder,
            projects::list::pin_project_to_top,
            projects::list::projects_on_top,
            projects::list::remove_project,
            projects::live::follow_project,
            projects::live::follow_project_run,
            projects::live::unfollow_project,
            projects::list::rename_project,
            projects::list::project_rename_repositories,
            projects::page::project_add_to_machine,
            projects::page::project_change_folder,
            projects::page::project_remove_from_machine,
            projects::page::project_machines,
            projects::page::project_sessions,
            projects::page::project_sessions_forget,
            projects::page::projects_sessions,
            projects::page::project_syncthing,
            projects::branches::project_branches,
            projects::history::project_git_history,
            projects::history::project_git_files,
            projects::history::project_git_diff,
            projects::runs::center_project_lists,
            projects::runs::center_project_run,
            projects::runs::center_project_runs,
            projects::team::contact_remove,
            projects::team::contacts,
            projects::team::issue_assign,
            projects::team::team_cards,
            projects::team::team_dismiss,
            projects::team::team_invite,
            projects::team::team_invite_contact,
            projects::team::team_invite_drop,
            projects::team_live::team_invites,
            projects::team::team_join,
            projects::team::team_leave,
            projects::team::team_prepare,
            projects::team::team_readmit,
            projects::team_live::team_record,
            projects::team::team_remove,
            projects::team::team_settings,
            projects::team::team_ticket,
            projects::threads::center_pull_request_head,
            projects::threads::center_thread_detail,
            projects::threads::center_comment_thread,
            report::commands::report_collect,
            report::commands::report_issue,
            report::commands::report_issue_text,
            report::commands::report_send,
            settings::cli_login::cli_logins,
            settings::cli_login::use_cli_login,
            settings::cli_login::use_gh_account,
            settings::credentials::check_credential,
            settings::credentials::list_credentials,
            settings::credentials::api_providers,
            settings::credentials::list_github_accounts,
            settings::credentials::remove_credential,
            settings::credentials::save_credential,
            settings::credentials::set_default_github_account,
            settings::gh_sign_in::cancel_gh_sign_in,
            settings::gh_sign_in::gh_sign_in,
            settings::skill_libraries::skill_libraries_list,
            settings::skill_libraries::skill_library_add,
            settings::skill_libraries::skill_library_update,
            settings::skill_libraries::skill_library_remove,
            settings::skill_libraries::skill_library_set_global,
            settings::skill_libraries::skill_library_set_skill,
            settings::skill_libraries::skill_library_uninstall_projects,
            settings::skill_libraries::project_skills_get,
            settings::skill_libraries::project_skills_set,
            settings::skill_libraries::chat_skills_set,
            settings::skill_libraries::skill_libraries_find,
            settings::skill_libraries::skill_libraries_problems,
            settings::assistant::assistant_settings_get,
            settings::assistant::assistant_settings_save,
            settings::editor_settings_get,
            settings::editor_settings_save,
            settings::dictation_settings_get,
            settings::dictation_settings_save,
            settings::skill_sync::skill_sync_get,
            settings::skill_sync::skill_sync_set,
            settings::skill_sync::project_sync_get,
            settings::skill_sync::project_sync_set,
            settings::skill_sync::docker_containers_get,
            settings::skill_sync::docker_containers_set,
        ]))
        .build(context)
        .expect("the app failed to start")
        .run(|app, event| {
            match event {
                // Closing the last window leaves Beep in the menu bar; only
                // Quit (an exit with a code) ends it.
                RunEvent::ExitRequested {
                    code: None, api, ..
                } => {
                    if !platform::update::relaunching(app) {
                        api.prevent_exit();
                    } else {
                        #[cfg(desktop)]
                        center::window_session::checkpoint(app);
                    }
                }
                // Quitting: the windows go at once, whatever stopping takes.
                RunEvent::ExitRequested { code: Some(_), .. } => {
                    #[cfg(desktop)]
                    center::window_session::checkpoint(app);
                    hide_windows(app);
                }
                RunEvent::Exit => quit(app),
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

/// Counts each command a window runs, by name, for the desktop's metrics.
fn counted(
    handler: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        let name = invoke.message.command().to_owned();
        let handled = handler(invoke);

        if handled {
            metrics::counter!("beep_command_total", "name" => name).increment(1);
        }

        handled
    }
}

fn hide_windows(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if let Err(e) = window.hide() {
            log::debug!("hiding {label} to quit: {e}");
        }
    }
}

/// Everything quitting does, within `QUIT_LIMIT` unless an update installs.
fn quit(app: &AppHandle) {
    #[cfg(desktop)]
    center::window_session::checkpoint(app);
    #[cfg(desktop)]
    if let Some(sampling) = app.try_state::<platform::energy::Sampling>() {
        sampling.stop();
    }

    hide_windows(app);
    // The size of a project window still open.
    app.state::<Prefs>().save();
    // The sessions as they stand now, for the next start.
    fleet::session_mirror::save(
        app.state::<Arc<center::view_cache::ViewCache>>().inner(),
        &app.state::<Arc<fleet::session_mirror::SessionMirror>>()
            .current()
            .sessions(),
    );
    let claude = app
        .state::<agents::accounts::ClaudeSignIn>()
        .stop_and_wait();
    let gh = app.state::<settings::gh_sign_in::GhSignIn>().stop_on_quit();
    // The router stops, and the sync and transfer services save their state,
    // while an update installs.
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    // What this Mac has open is noted for the next start.
    let noting = app.state::<machines::this_mac::Machine>().quit();
    let stopping = tauri::async_runtime::spawn(async move {
        tokio::join!(noting, fleet.stop(), claude, gh);
    });
    // However Beep quits, a downloaded update is installed for next time.
    // With none this returns at once.
    platform::update::install(app);
    tauri::async_runtime::block_on(async move {
        match timeout(QUIT_LIMIT, stopping).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("stopping the mesh failed: {e}"),
            Err(_) => log::warn!("quitting before the mesh stopped"),
        }
    });
    // The views a window drew last are kept for the next start.
    app.state::<Arc<center::view_cache::ViewCache>>().flush();
    app.state::<Arc<agents::outbox::Outbox>>().sync();
    app.state::<Arc<agents::starts::Starts>>().sync();
    // Background tasks the agent started here end with the app.
    #[cfg(desktop)]
    beep_node::kill_tasks();
}

fn setup(
    app: &mut App,
    #[cfg(desktop)] recorder: std::thread::JoinHandle<anyhow::Result<PrometheusHandle>>,
) -> anyhow::Result<()> {
    platform::startup::stage("setup");
    // Kept as state so it lives as long as the app.
    #[cfg(target_os = "android")]
    if let Some(guard) = platform::logfile::start_android(&beep::identity::data_dir(app.handle())?)
    {
        app.manage(std::sync::Mutex::new(guard));
    }
    let (actions, asked) = notices::notify::Actions::new();
    tauri::async_runtime::spawn(notices::follow_actions(app.handle().clone(), asked));
    app.manage(actions);
    #[cfg(target_os = "macos")]
    notices::notify::init(app.handle());
    #[cfg(all(unix, desktop))]
    beep_node::machine::raise_open_files();
    #[cfg(target_os = "macos")]
    app.set_activation_policy(ActivationPolicy::Accessory);
    #[cfg(target_os = "windows")]
    {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
        app.handle().plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state == ShortcutState::Pressed {
                        bar::toggle(app);
                    }
                })
                .build(),
        )?;

        if let Err(e) = app.global_shortcut().register("Ctrl+Alt+Space") {
            log::warn!("could not register Ctrl+Alt+Space: {e}");
        }
    }
    let handle = app.handle().clone();
    #[cfg(desktop)]
    let as_login_item = std::env::args().any(|arg| arg == LOGIN_ARG);
    #[cfg(mobile)]
    let as_login_item = false;

    // Opened by the user, not as a login item: show the window. It is made first, so its
    // page's process starts while the rest is set up; the page asks for nothing before then.
    if !as_login_item && let Err(e) = center::open(&handle) {
        log::warn!("opening the window at launch: {e}");
    }
    platform::startup::stage("window built");

    let dir = beep::identity::data_dir(&handle)?;
    let identity = beep::identity::load_or_create(&dir.join("identity.key"))?;
    let prefs = Prefs::load(StateFile::new(dir.join("state.json")));
    let views = Arc::new(center::view_cache::ViewCache::load(
        dir.join("views.json"),
        env!("CARGO_PKG_VERSION"),
    ));

    app.manage(palette::PaletteCache::load(&views)?);
    #[cfg(desktop)]
    app.manage(Arc::new(palette::copies::Copies::load(&dir)));
    app.manage(views);
    app.manage(Arc::new(agents::kept_transcripts::KeptTranscripts::new(
        dir.join("transcripts"),
    )));
    let cache = app.path().app_cache_dir()?;
    app.manage(Arc::new(agents::outbox::Outbox::load(
        dir.join("outbox"),
        cache.join("outbox"),
    )));
    app.manage(Arc::new(agents::starts::Starts::load(
        dir.join("starts"),
        cache.join("starts"),
    )));
    let endpoint = tauri::async_runtime::block_on(bind_endpoint(
        beep::identity::secret_key(&identity),
        beep_mesh::transport_config(),
    ))?;
    #[cfg(desktop)]
    tauri::async_runtime::spawn_blocking({
        let (app, endpoint, file) = (
            handle.clone(),
            endpoint.clone(),
            dir.join("node/metrics.log"),
        );

        move || {
            let started = recorder
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("installing the recorder panicked")))
                .and_then(|recorder| {
                    beep_node::link::diagnostics::start(recorder, &endpoint, &file)
                });

            match started {
                Ok(task) => {
                    app.manage(task);

                    if let Some(sampling) = platform::energy::start(app.clone()) {
                        app.manage(sampling);
                    }
                }
                Err(e) => log::warn!("metrics unavailable: {e:#}"),
            }
        }
    });
    #[cfg(mobile)]
    match platform::metrics::PhoneMetrics::start(&dir.join("metrics")) {
        Ok(metrics) => {
            app.manage(metrics);
        }
        Err(e) => log::warn!("metrics unavailable: {e:#}"),
    }
    app.manage(prefs);
    #[cfg(desktop)]
    app.manage(center::window_session::WindowSession::load(
        dir.join("windows.json"),
    ));
    #[cfg(desktop)]
    app.manage(plugins::Installer::new());
    #[cfg(desktop)]
    match plugins::start(&handle, &dir) {
        Ok(host) => {
            app.manage(host);
        }
        Err(e) => log::warn!("plugins unavailable: {e}"),
    }
    // This Mac's node, and the mesh it is in, which opens in the background.
    #[cfg(desktop)]
    let machine =
        machines::this_mac::Machine::new(&dir.join("node"), Some(projects::agent::tools(&handle)));
    #[cfg(desktop)]
    let fleet = Arc::new(Fleet::new(
        endpoint,
        &dir.join("node"),
        machine.name(),
        machine.offers(),
    ));
    // A phone embeds no node: it observes the mesh a desktop pairs it with, if any.
    #[cfg(mobile)]
    let machine = machines::this_mac::Machine;
    #[cfg(mobile)]
    let fleet = {
        let mesh = dir.join("workspace").join(endpoint.id().to_string());
        Arc::new(Fleet::observer(endpoint, &mesh))
    };
    app.manage(fleet.clone());
    app.manage(fleet::session_lists::SessionLists::start(
        handle.clone(),
        fleet.clone(),
    ));
    let opened_files = handle.path().app_cache_dir()?.join("agent-files");
    let files = {
        let files = Arc::new(agents::file_cache::FileCache::new(
            handle.path().app_cache_dir()?.join("chat-files"),
        ));
        app.manage(files.clone());
        files
    };
    tauri::async_runtime::spawn(async move {
        loop {
            agents::media::sweep_open_files(&opened_files).await;
            files.sweep().await;
            tokio::time::sleep(agents::media::SWEEP_EVERY).await;
        }
    });
    // What the projects pages read, kept for the run of the app.
    app.manage(beep_fleet::Session::new(
        fleet.clone(),
        beep_fleet::Options {
            usage_file: Some(dir.join("usage.msgpack")),
            ..beep_fleet::Options::default()
        },
        projects::live::changed(handle.clone()),
    )?);
    let opening = fleet.clone();
    tauri::async_runtime::spawn(async move { opening.run().await });
    app.manage(machine);
    app.manage(machines::list::MachineLists::default());
    app.manage(machines::add::Tickets::default());
    app.manage(agents::files_live::FilesLive::default());
    let cache = handle
        .state::<Arc<center::view_cache::ViewCache>>()
        .inner()
        .clone();
    let mirror = tauri::async_runtime::block_on(async {
        fleet::session_mirror::SessionMirror::start(
            fleet::session_mirror::Desk {
                app: handle.clone(),
                fleet: fleet.clone(),
            },
            fleet::session_mirror::kept(&cache),
        )
    });
    let kept = cache.clone();
    tauri::async_runtime::spawn(fleet::session_mirror::keep(mirror.watch(), move |rows| {
        fleet::session_mirror::save(&kept, rows)
    }));
    let home_signal = Arc::new(fleet::home_signal::HomeSignal::new());
    tauri::async_runtime::spawn(fleet::event_views::follow_signal(
        handle.clone(),
        home_signal.follow(),
    ));
    app.manage(home_signal);
    let (views_signal, changes) = fleet::view_signal::ViewSignal::new();
    app.manage(views_signal);
    tauri::async_runtime::spawn(fleet::event_views::follow_rows(
        handle.clone(),
        mirror.watch(),
    ));
    app.state::<fleet::session_lists::SessionLists>()
        .follow(mirror.watch());
    app.manage(Arc::new(mirror));
    app.manage(projects::live::Followed::default());
    app.manage(projects::team_live::TeamLive::default());
    app.manage(notices::Looks::new());
    app.manage(settings::gh_sign_in::GhSignIn::default());
    app.manage(agents::accounts::ClaudeSignIn::default());
    app.manage(platform::update::Updates::default());
    app.manage(center::AgentsShown::default());
    app.manage(center::window::OnScreen::default());
    app.manage(agents::follow::Follows::default());
    app.manage(fleet::events::Events::default());
    app.manage(fleet::event_views::Views::default());
    app.manage(center::home::HomeReads::default());
    // What it applies is managed above: a change handled before that would panic it.
    tauri::async_runtime::spawn(fleet::event_views::follow_changes(handle.clone(), changes));
    fleet::event_views::listen(&handle);
    app.manage(center::Pending::default());
    app.manage(tray::waiting::WaitingList::default());
    app.manage(Arc::new(notices::history::Notes::load(
        dir.join("notes.json"),
    )));
    app.manage(machines::terminals::Attached::default());
    app.manage(machines::devices::Attached::default());
    app.state::<machines::this_mac::Machine>().start();
    app.state::<machines::this_mac::Machine>()
        .sync(&app.state::<Prefs>());

    // Messages kept from the last run go once their machines answer.
    agents::outbox::resume(&handle);
    // Starts kept from the last run go once their machines are online.
    agents::starts::resume(&handle);

    let (following, direct) = fleet::alerts::start(fleet.clone());
    tauri::async_runtime::spawn(following);
    tauri::async_runtime::spawn(notices::serve(handle.clone(), direct));
    tauri::async_runtime::spawn(palette::lists::follow(handle.clone()));
    tauri::async_runtime::spawn(platform::update::follow(handle.clone()));
    #[cfg(desktop)]
    tauri::async_runtime::spawn(machines::node_updates::follow(handle.clone()));
    tauri::async_runtime::spawn(tray::waiting::follow(handle.clone()));
    let desks = app.state::<Arc<beep_fleet::Session>>().inner().clone();
    tauri::async_runtime::spawn(settings::skill_libraries::follow(
        app.state::<Arc<Fleet>>().inner().clone(),
        desks.clone(),
    ));
    tauri::async_runtime::spawn(notices::follow(handle.clone(), fleet.clone(), desks));
    tauri::async_runtime::spawn(notices::history::follow_reads(
        handle.clone(),
        fleet.clone(),
    ));
    tauri::async_runtime::spawn(agents::sessions::follow_titles(
        handle.clone(),
        fleet.clone(),
    ));
    #[cfg(desktop)]
    bar::hotkey::start(handle.clone());

    #[cfg(desktop)]
    center::app_menu::install(app)?;
    tray::install(app)?;

    // The bar is a desktop popover; a phone has the one window. It is made once Home has
    // drawn, so its hidden page does not load ahead of Home, and zooms as it loads.
    #[cfg(desktop)]
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(BAR_AFTER).await;

        // Off the main thread, as Windows needs of a window made while the app runs.
        match tauri::async_runtime::spawn_blocking(move || build_bar(&handle)).await {
            Ok(Ok(())) => platform::startup::stage("bar built"),
            Ok(Err(e)) => log::warn!("making the bar: {e:#}"),
            Err(e) => log::warn!("making the bar stopped: {e}"),
        }
    });

    #[cfg(desktop)]
    center::window::restore_detached(app.handle());
    #[cfg(desktop)]
    refresh_login_item(app.handle());
    platform::startup::stage("setup done");
    Ok(())
}

#[cfg(desktop)]
fn build_bar(app: &AppHandle) -> anyhow::Result<()> {
    let bar = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == bar::LABEL)
        .ok_or_else(|| anyhow::anyhow!("bar window config missing"))?;
    navigation::guard(tauri::WebviewWindowBuilder::from_config(app, bar)?).build()?;
    Ok(())
}

/// Login items made before they carried `LOGIN_ARG` open the window at
/// login; enabling again rewrites them with it.
#[cfg(desktop)]
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
