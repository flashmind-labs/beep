//! Request handlers, one function per request type.

use crate::{
    app::{Answer, Draws, Message},
    images::{MAX_IMAGE_BYTES, MAX_IMAGES},
    live::Subscription,
    permissions,
    protocol::*,
    router::{Cx, Router, Sink},
    store::CreateRun,
    tools::stocks::{self, StockError, Symbol},
};
use chrono::{DateTime, Utc};
use chrono_tz::{Tz, UTC};
use flashmind::cron::{CronSchedule, JobSchedule};
use std::{collections::VecDeque, time::Duration};
use tokio::{sync::broadcast::error::RecvError, time::timeout};
use uuid::Uuid;

mod agent_accounts;
mod agent_auto;
mod agent_control;
mod agent_found;
mod agent_sessions;
pub(crate) use agent_sessions::machine_sessions;
mod branch_work;
mod credentials;
mod crons;
mod custom_agents;
mod devices;
mod findings;
mod git_items;
mod grants;
mod groups;
mod harnesses;
mod instructions;
mod live;
mod location;
mod machine_tools;
mod machines;
mod memories;
mod own_nodes;
mod project_groups;
mod project_import;
pub(crate) mod project_sync;
mod project_watch;
mod projects;
mod research;
mod reviews;
mod sessions;
mod settings;
mod spend_report;
mod ssh;
mod terminals;
mod threads;
mod transcribe;
mod updates;
mod usage;
mod work;
mod work_diff;
mod work_files;

use crons::{create_cron, delete_cron, list_crons, run_cron, toggle_cron, update_cron};
use devices::{
    invite_device, join_account, link_device, list_devices, peek_invite, register_device,
    remove_device, unregister_device,
};
use grants::{list_grants, revoke_grant};
use groups::{
    answer_group_approval, create_group, delete_group_reminder, get_group, get_group_images,
    group_invite, join_group, leave_group, list_group_reminders, list_groups, peek_group,
    pin_group, send_to_group, share_group_apps,
};
use location::{
    cross_place, delete_place, list_places, set_timezone, update_location, update_place,
};
use machines::{
    create_node_token, get_node_token, list_machine_details, list_machine_stats, list_machines,
    list_updatable_machines, remove_machine, revoke_node_token, update_machine,
};
use memories::{
    clear_memories, delete_memory, delete_skill, list_memories, list_memory_reads,
    list_memory_scopes, list_skills, undo_skill, update_memory, update_skill,
};
use research::{get_research, get_research_pdf, list_researches, stop_research};
use sessions::{
    copy_share, delete_session, delete_sessions, delete_turn, delete_turns_from, existing_sessions,
    get_file, get_route, get_route_card, get_session, get_session_agents, get_session_files,
    get_session_images, get_session_sources, get_share, get_tool_labels, list_sessions,
    pin_session, rename_session, search_sessions, share_session, unshare_session,
};
use settings::{
    add_app_account, add_mcp_server, connect_app, connect_mcp_server, disconnect_app,
    finish_app_auth, finish_mcp_auth, get_agents, get_app_permissions, get_models,
    list_app_accounts, list_apps, list_mcp_servers, list_mcp_tools, remove_app_account,
    remove_mcp_server, set_agents, set_app_permissions, set_auto_model, set_mcp_tool, set_model,
    set_thinking, start_app_auth,
};

const SEND_TIMEOUT: Duration = Duration::from_secs(10);

pub fn router() -> Router {
    Router::default()
        .call(hello)
        .call(health)
        .stream(chat)
        .call(get_run)
        .call(active_run)
        .call(cancel_run)
        .stream(get_events)
        .call(device_result)
        .call(list_sessions)
        .call(get_session)
        .call(rename_session)
        .call(delete_session)
        .call(delete_sessions)
        .call(pin_session)
        .call(list_memories)
        .call(delete_memory)
        .call(update_memory)
        .call(clear_memories)
        .call(list_crons)
        .call(create_cron)
        .call(toggle_cron)
        .call(delete_cron)
        .call(update_cron)
        .call(run_cron)
        .call(register_device)
        .call(unregister_device)
        .call(update_location)
        .call(set_timezone)
        .stream(stream_notifications)
        .call(list_apps)
        .call(connect_app)
        .call(disconnect_app)
        .call(start_app_auth)
        .call(finish_app_auth)
        .call(list_app_accounts)
        .call(add_app_account)
        .call(remove_app_account)
        .call(suggest_prompts)
        .call(get_app_permissions)
        .call(set_app_permissions)
        .call(answer_approval)
        .call(get_models)
        .call(set_model)
        .call(set_thinking)
        .call(existing_sessions)
        .call(delete_turn)
        .call(delete_turns_from)
        .call(search_sessions)
        .call(set_auto_model)
        .call(get_agents)
        .call(set_agents)
        .call(link_device)
        .call(list_devices)
        .call(remove_device)
        .call(invite_device)
        .call(peek_invite)
        .call(join_account)
        .call(share_session)
        .call(unshare_session)
        .call(get_share)
        .call(copy_share)
        .call(create_group)
        .call(list_groups)
        .call(get_group)
        .call(send_to_group)
        .call(group_invite)
        .call(peek_group)
        .call(join_group)
        .call(leave_group)
        .call(pin_group)
        .call(share_group_apps)
        .call(answer_group_approval)
        .call(get_file)
        .call(list_group_reminders)
        .call(delete_group_reminder)
        .call(list_places)
        .call(delete_place)
        .call(update_place)
        .call(cross_place)
        .call(list_memory_reads)
        .call(get_session_images)
        .call(get_session_files)
        .call(get_session_agents)
        .call(get_tool_labels)
        .call(get_session_sources)
        .call(get_route)
        .call(get_route_card)
        .call(list_mcp_servers)
        .call(add_mcp_server)
        .call(connect_mcp_server)
        .call(finish_mcp_auth)
        .call(remove_mcp_server)
        .call(list_mcp_tools)
        .call(set_mcp_tool)
        .call(get_group_images)
        .call(get_updates)
        .call(list_skills)
        .call(update_skill)
        .call(delete_skill)
        .call(undo_skill)
        .stream(stream_updates)
        .call(stock_chart)
        .call(get_research)
        .call(stop_research)
        .call(get_research_pdf)
        .call(list_researches)
        .call(speak_notice)
        .call(list_machines)
        .call(list_memory_scopes)
        .duplex(transcribe::transcribe)
        .call(live::spoken_turns)
        .call(live::say)
        .call(live::filler)
        .call(work::list_work)
        .call(work::cancel_work)
        .call(work::list_questions)
        .call(list_grants)
        .call(revoke_grant)
        .call(findings::list_findings)
        .call(findings::update_finding)
        .call(list_machine_details)
        .call(list_machine_stats)
        .call(agent_sessions::list_agent_sessions)
        .call(agent_sessions::answer_agent_question)
        .call(agent_control::list_agent_hosts)
        .call(agent_control::start_agent_session)
        .call(agent_control::preview_agent_machine)
        .call(agent_control::get_agent_session)
        .call(agent_control::steer_agent_session)
        .call(work::list_pending_approvals)
        .call(projects::get_project_agent)
        .call(projects::list_project_tasks)
        .call(projects::list_project_workers)
        .call(projects::update_project_task)
        .call(projects::stop_project_worker)
        .call(projects::list_projects)
        .call(projects::list_project_overview)
        .call(instructions::get_instructions)
        .call(instructions::save_instructions)
        .call(instructions::list_instructions_versions)
        .call(instructions::read_agent_files)
        .call(instructions::sync_agent_files)
        .call(terminals::list_terminals)
        .call(terminals::start_terminal)
        .call(terminals::get_terminal)
        .call(terminals::close_terminal)
        .call(terminals::open_terminal)
        .call(terminals::list_all_terminals)
        .call(usage::get_usage)
        .call(usage::get_usage_report)
        .call(spend_report::get_spend_report)
        .duplex(terminals::attach::attach_terminal)
        .call(reviews::list_project_branches)
        .call(reviews::start_branch_review)
        .call(reviews::get_branch_review)
        .call(reviews::check_branch_review)
        .call(reviews::list_branch_reviews)
        .call(reviews::dismiss_review_finding)
        .call(reviews::fix_review_findings)
        .call(reviews::post_branch_review)
        .call(reviews::merge_reviewed_branch)
        .call(agent_accounts::get_agent_accounts)
        .call(agent_accounts::save_agent_token)
        .call(agent_accounts::revoke_agent_token)
        .call(agent_accounts::start_codex_login)
        .call(agent_accounts::get_codex_login)
        .call(agent_accounts::cancel_codex_login)
        .call(projects::list_project_issues)
        .call(projects::track_project_issue)
        .call(projects::dismiss_project_issue)
        .call(project_sync::set_project_sync)
        .call(project_sync::list_project_sync)
        .call(project_sync::get_sync_skipped)
        .call(project_sync::pin_project)
        .call(project_sync::pin_project_to_top)
        .call(project_sync::list_projects_on_top)
        .call(project_groups::list_project_groups)
        .call(project_groups::save_project_group)
        .call(project_groups::delete_project_group)
        .call(updates::list_updates)
        .call(updates::dismiss_updates)
        .call(updates::open_update_chat)
        .call(create_node_token)
        .call(revoke_node_token)
        .call(remove_machine)
        .call(update_machine)
        .call(list_updatable_machines)
        .call(get_node_token)
        .call(machine_tools::list_tool_updates)
        .call(machine_tools::update_machine_tool)
        .call(ssh::list_ssh_hosts)
        .call(ssh::add_machine_over_ssh)
        .call(ssh::get_machine_search)
        .call(ssh::set_machine_search)
        .call(credentials::list_credentials)
        .call(credentials::save_credential)
        .call(credentials::remove_credential)
        .call(credentials::list_github_accounts)
        .call(credentials::save_github_account)
        .call(credentials::set_default_github_account)
        .call(project_import::import_project)
        .call(project_import::list_imported_projects)
        .call(project_import::list_machine_folder)
        .call(project_import::list_project_machines)
        .call(project_import::preview_import)
        .call(project_import::find_import_fork)
        .call(branch_work::add_branch_worktree)
        .call(agent_found::list_found_folders)
        .call(project_import::list_host_repos)
        .call(agent_found::list_project_sessions)
        .call(agent_control::resume_agent_session)
        .call(git_items::list_git_items)
        .call(git_items::list_labeled_git_items)
        .call(threads::list_project_threads)
        .call(threads::reply_to_thread)
        .call(threads::open_thread_chat)
        .call(threads::list_thread_chats)
        .call(agent_control::list_agent_images)
        .call(agent_control::get_agent_image)
        .call(agent_control::get_agent_file)
        .call(work_files::list_work_folder)
        .call(work_files::list_work_tree)
        .call(work_files::read_work_file)
        .call(work_files::write_work_file)
        .call(work_diff::get_work_diff)
        .call(work_diff::get_branch_diff)
        .call(credentials::check_credential)
        .call(custom_agents::list_custom_agents)
        .call(agent_control::list_agent_models)
        .call(project_import::remove_project)
        .call(project_sync::list_project_syncthing)
        .call(projects::list_project_pull_requests)
        .call(projects::get_pull_request_head)
        .call(list_active_runs)
        .call(threads::get_project_thread)
        .call(threads::get_thread_detail)
        .call(harnesses::list_agent_harnesses)
        .call(harnesses::save_harness_settings)
        .call(harnesses::update_agent_harness)
        .call(harnesses::get_flsh_settings)
        .call(harnesses::save_flsh_settings)
        .call(harnesses::list_harness_settings)
        .call(harnesses::save_harness_settings_all)
        .call(project_sync::get_project_sync_detail)
        .call(project_sync::pause_project_sync)
        .call(project_sync::resolve_sync_conflict)
        .call(project_watch::get_project_lists)
        .call(project_watch::get_labeled_project_lists)
        .call(project_watch::list_project_runs)
        .call(project_watch::get_project_run)
        .call(project_watch::list_project_runs_all)
        .call(project_watch::get_project_run_on)
        .stream(project_watch::watch_project)
}

type Reply<T> = Result<T, ApiError>;

/// The transport reads the opening `Hello`; a later one changes nothing.
async fn hello(_: Cx, _: Hello) -> Reply<()> {
    Ok(())
}

async fn health(_: Cx, _: Health) -> Reply<()> {
    Ok(())
}

async fn chat(cx: Cx, mut req: Chat, sink: Sink<Event>) -> Reply<()> {
    let untyped = req.message.trim().is_empty();
    if (untyped && req.voice.is_none()) || req.message.len() > 32768 {
        return Err(ApiError::bad_request("message must be 1-32768 bytes"));
    }
    if req.voice.is_some() && req.research {
        return Err(ApiError::bad_request("a research starts from text"));
    }
    check_images(&req.images)?;
    check_files(&req.files)?;
    let model = match &req.model {
        Some(id) => Some(cx.app.models.find(id).ok_or(ApiError::NotFound {})?.clone()),
        None => None,
    };
    // Transcribed last: a refused or retried message costs no transcription.
    if let Some(voice) = &req.voice {
        let request = req.request_id.to_string();
        match cx.user.run_for_spoken(req.session_id, &request).await? {
            Some(CreateRun::Created(run) | CreateRun::Existing(run)) => {
                return follow(&cx, run, 0, &sink).await;
            }
            Some(CreateRun::Conflict) => {
                return Err(ApiError::Conflict {
                    message: "session busy".into(),
                });
            }
            None => {}
        }
        let said = cx.app.transcribe(&cx.user, voice).await?;
        req.message = spoken_message(&req.message, &said);
    }
    if req.research {
        if cx.app.models.research().is_none() {
            return Err(ApiError::bad_request(
                "research is not set up on this server",
            ));
        }
        cx.user.start_research(req.session_id, &req.message).await?;
    }
    let message = Message {
        session_id: req.session_id,
        request_id: req.request_id,
        message: req.message,
        cron_id: None,
        scheduled: None,
        always_sent: false,
        woken: None,
        images: req.images,
        files: req.files,
        answer: None,
        model,
        thinking: req.thinking,
        origin: cx.device,
        platform: req.platform,
        draws: draws(&cx),
        finished_research: false,
        voice: req.voice,
        spoken: req.spoken,
    };
    match cx.app.submit(&cx.user, message).await? {
        CreateRun::Created(run) | CreateRun::Existing(run) => follow(&cx, run, 0, &sink).await,
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}

/// The transcript, after anything typed with it.
fn spoken_message(typed: &str, said: &str) -> String {
    match typed.trim() {
        "" => said.to_owned(),
        typed => format!("{typed}\n\n{said}"),
    }
}

/// A notice, or a reply, as a voice note.
async fn speak_notice(cx: Cx, SpeakNotice { title, body }: SpeakNotice) -> Reply<VoiceNote> {
    if title.len() + body.len() > 32768 {
        return Err(ApiError::bad_request("a notice is at most 32768 bytes"));
    }
    if title.trim().is_empty() && body.trim().is_empty() {
        return Err(ApiError::bad_request("nothing to say"));
    }
    cx.app.speak_notice(&cx.user, &title, &body).await
}

/// What a run started on this connection can show; only the macOS app draws charts.
fn draws(cx: &Cx) -> Draws {
    let desktop = cx.device == (DeviceKind::Desktop {});
    Draws {
        charts: desktop && cx.supports(Feature::CHARTS),
        cards: cx.supports(Feature::CARDS),
        sources: cx.supports(Feature::SOURCES),
    }
}

/// One symbol's chart over another range, for a stock chart's range buttons.
async fn stock_chart(_: Cx, req: StockChart) -> Reply<Chart> {
    let symbol = Symbol::parse(&req.symbol).map_err(|e| ApiError::bad_request(e.to_string()))?;
    match stocks::fetch(&[symbol], req.range).await {
        Ok(histories) => Ok(stocks::chart(req.range, &histories)),
        Err(StockError::NotFound(_) | StockError::NoPrices(_)) => Err(ApiError::NotFound {}),
        Err(e) => {
            log::warn!("stock chart failed: {e}; symbol={}", req.symbol);
            Err(ApiError::Internal {})
        }
    }
}

/// Refuses too many images, or one too large.
fn check_images(images: &[String]) -> Reply<()> {
    if images.len() > MAX_IMAGES {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_IMAGES} images per message"
        )));
    }
    if images.iter().any(|image| image.len() > MAX_IMAGE_BYTES) {
        return Err(ApiError::bad_request(format!(
            "each image must be at most {} MiB",
            MAX_IMAGE_BYTES >> 20
        )));
    }
    Ok(())
}

/// Refuses files past `MAX_UPLOAD` in all, or one without a name.
fn check_files(files: &[Upload]) -> Reply<()> {
    if files.len() > MAX_IMAGES {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_IMAGES} files per message"
        )));
    }
    if files.iter().map(|file| file.data.len()).sum::<usize>() > MAX_UPLOAD {
        return Err(ApiError::bad_request(format!(
            "files may be at most {} MiB in all",
            MAX_UPLOAD >> 20
        )));
    }
    if files
        .iter()
        .any(|file| file.name.trim().is_empty() || file.name.len() > 255 || file.mime.len() > 255)
    {
        return Err(ApiError::bad_request("each file needs a name"));
    }
    Ok(())
}

async fn get_run(cx: Cx, GetRun { id }: GetRun) -> Reply<Run> {
    owned_run(&cx, &id).await
}

async fn active_run(cx: Cx, ActiveRun { session_id }: ActiveRun) -> Reply<Option<Run>> {
    Ok(cx.user.active_run(session_id).await?)
}

async fn list_active_runs(cx: Cx, _: ListActiveRuns) -> Reply<Vec<Run>> {
    Ok(cx.user.active_runs().await?)
}

async fn cancel_run(cx: Cx, CancelRun { id }: CancelRun) -> Reply<()> {
    owned_run(&cx, &id).await?;
    cx.app.cancel(&id).await;
    Ok(())
}

/// Resumes a run's events after `after`, the last `seq` the client saw.
async fn get_events(cx: Cx, req: GetEvents, sink: Sink<Event>) -> Reply<()> {
    let GetEvents { run_id, after } = req;
    if after < 0 {
        return Err(ApiError::bad_request("after must be >= 0"));
    }
    let run = owned_run(&cx, &run_id).await?;
    follow(&cx, run, after, &sink).await
}

/// Sends the run's events after `after` until it finishes; a finished run keeps only its status.
async fn follow(cx: &Cx, run: Run, mut after: i64, sink: &Sink<Event>) -> Reply<()> {
    let Some(live) = cx.app.live(&run.id).await else {
        let finished = Event {
            seq: after + 1,
            event: RunEvent::RunFinished {
                status: run.status,
                updated_at: None,
            },
        };
        sink.send(&finished).await?;
        return Ok(());
    };

    let Subscription {
        backlog,
        live: mut rx,
    } = live.subscribe(after);
    let mut pending = VecDeque::from(backlog);

    loop {
        let event = match pending.pop_front() {
            Some(event) => event,
            None => match rx.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => {
                    pending.extend(live.since(after));
                    continue;
                }
                Err(RecvError::Closed) => return Ok(()),
            },
        };
        if event.seq <= after {
            continue;
        }

        after = event.seq;
        // Only an app that plays voice notes reads a transcript.
        if matches!(event.event, RunEvent::Transcript { .. }) && !cx.supports(Feature::VOICE) {
            continue;
        }
        // Nor does an app from before source chips read a reply's sources.
        if matches!(event.event, RunEvent::Sources { .. }) && !cx.supports(Feature::SOURCES) {
            continue;
        }
        let finished = matches!(event.event, RunEvent::RunFinished { .. });
        sink.send(&event).await?;
        if finished {
            return Ok(());
        }
    }
}

async fn device_result(cx: Cx, req: DeviceResult) -> Reply<bool> {
    owned_run(&cx, &req.run_id).await?;
    Ok(cx.app.resolve_device_query(&req.query_id, req.answer).await)
}

/// Resumes a session stopped on a question; answering again returns the first answer's run.
async fn answer_approval(cx: Cx, req: AnswerApproval) -> Reply<Run> {
    let AnswerApproval {
        session_id,
        query_id,
        decision,
        for_session,
    } = req;
    // A call of background work waits on its own; nothing resumes.
    if cx
        .app
        .subagents
        .approvals
        .answer(&cx.user, &query_id, decision, for_session)
        .await?
    {
        return Err(ApiError::Conflict {
            message: "answered; the background work goes on".into(),
        });
    }
    let message = Message {
        session_id,
        request_id: Uuid::nil(),
        message: decision.as_str().to_owned(),
        cron_id: None,
        scheduled: None,
        always_sent: false,
        woken: None,
        images: Vec::new(),
        files: Vec::new(),
        answer: Some(Answer {
            call_id: query_id.clone(),
            decision,
            for_session,
        }),
        model: None,
        thinking: None,
        origin: cx.device,
        platform: None,
        draws: draws(&cx),
        finished_research: false,
        voice: None,
        spoken: false,
    };
    let conversation = cx.user.context(session_id).await?;
    let open = conversation.map(|context| permissions::open_calls(&context.conversation));
    let waiting = open
        .and_then(|open| open.into_iter().next())
        .is_some_and(|call| call.id == query_id);
    let created = if waiting {
        cx.app.submit(&cx.user, message).await?
    } else {
        cx.user
            .existing_run(session_id, &message.request(), &message.message)
            .await?
            .ok_or_else(|| ApiError::Conflict {
                message: "nothing is waiting for this answer".into(),
            })?
    };
    match created {
        CreateRun::Created(run) | CreateRun::Existing(run) => Ok(run),
        CreateRun::Conflict => Err(ApiError::Conflict {
            message: "session busy".into(),
        }),
    }
}

async fn suggest_prompts(
    cx: Cx,
    SuggestPrompts { refresh }: SuggestPrompts,
) -> Reply<PromptSuggestions> {
    Ok(cx.app.suggest_prompts(&cx.user, refresh).await)
}

/// Pushes the user's notices until the connection closes.
async fn stream_notifications(cx: Cx, _: StreamNotifications, sink: Sink<Notice>) -> Reply<()> {
    let mut rx = cx.app.notifications.subscribe();
    // An app from before resolved questions would only refresh the chat.
    let resolves = cx.supports(Feature::QUESTIONS);
    loop {
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            received = rx.recv() => match received {
                Ok(event) if event.user == cx.user.id()
                    && (resolves || event.notice.resolved_question().is_none()) => {
                    let Some(notice) = event.for_device(&cx.connection.remote_id(), &cx.device) else {
                        continue;
                    };
                    match timeout(SEND_TIMEOUT, sink.send(notice)).await {
                        Ok(sent) => sent?,
                        Err(_) => return Ok(()),
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }
}

/// The caller's updates after `after`, a page at a time.
async fn get_updates(cx: Cx, GetUpdates { after }: GetUpdates) -> Reply<UpdatePage> {
    Ok(cx.user.updates_page(after).await?)
}

/// Sends the caller's newest update id now and each time it moves.
async fn stream_updates(cx: Cx, _: StreamUpdates, sink: Sink<i64>) -> Reply<()> {
    let mut last = cx.user.watch_updates().await?;
    loop {
        let newest = *last.borrow_and_update();
        match timeout(SEND_TIMEOUT, sink.send(&newest)).await {
            Ok(sent) => sent?,
            Err(_) => return Ok(()),
        }
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            changed = last.changed() => if changed.is_err() {
                return Ok(());
            },
        }
    }
}

/// A chat id from a request; one that is not a UUID names no chat.
fn chat_id(id: &str) -> Reply<Uuid> {
    Uuid::parse_str(id).map_err(|_| ApiError::NotFound {})
}

/// The run, if it belongs to the caller.
async fn owned_run(cx: &Cx, id: &str) -> Reply<Run> {
    cx.user.run(id).await?.ok_or(ApiError::NotFound {})
}

fn found(found: bool) -> Reply<()> {
    if found {
        Ok(())
    } else {
        Err(ApiError::NotFound {})
    }
}

/// When `schedule` next fires, reading a cron expression in `tz_name`.
fn next_run(schedule: &JobSchedule, tz_name: &str) -> Option<DateTime<Utc>> {
    let expression = match schedule {
        JobSchedule::Once(at) => return Some(*at),
        JobSchedule::Cron(expression) => expression,
        JobSchedule::OnWake { .. } => return None,
    };
    let tz: Tz = tz_name.parse().unwrap_or(UTC);
    CronSchedule::parse(expression)
        .ok()?
        .next_after_in_tz(&Utc::now(), tz)
        .map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_op_is_routed_once() {
        super::router().check().unwrap();
    }
}
