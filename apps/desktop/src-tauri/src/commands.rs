//! What the bar's page can ask the app to do.

use crate::{
    bar,
    beep::{
        self, BarEvent, Beep, DataImage, Decision, FileUpload, LinkView, Pick, Pictures, RunNews,
        Sent, Turn, image_uris, qr_svg,
    },
    center::{self, Chat, Shown},
    error::{DesktopError, Result},
    groups::{GroupChatView, GroupView, InviteView, MessageView},
    machine::{self, Machine},
    markdown, orb,
    state::{LinkState, NoticeVoice},
    voice::{Answered, NoticeCard, Voice, Waiting},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    Agents, AnswerGroupApproval, ApiError, CancelRun, Chart, ChartRange, ClearMemories, CreateCron,
    CreateGroup, Cron, DeleteCron, DeleteGroupReminder, DeleteMemory, DeleteSession, DeleteSkill,
    GetAgents, GetGroup, GetGroupImages, GetModels, GetRouteCard, GetSessionAgents,
    GetSessionFiles, GetSessionSources, GroupInvite, GroupPreview, GroupReminder, JoinGroup,
    LeaveGroup, ListCrons, ListGroupReminders, ListGroups, ListMemories, ListMemoryReads,
    ListMemoryScopes, ListSkills, Memory, MemoryRead, MemoryScope, Models, PeekGroup, PinGroup,
    PinSession, ReasoningLevel, RenameSession, RouteCard, RunCron, SendToGroup, SentFiles, Session,
    SessionMatch, SetAgents, SetAutoModel, SetModel, SetThinking, ShareGroupApps, ShareSession,
    SharedLink, Skill, StockChart, ToggleCron, TurnAgent, TurnSources, UndoSkill, UnshareSession,
    UpdateCron, UpdateMemory, UpdateSkill, format_group_code, group_code,
    node::desktop::Permission,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, ipc::Channel};
use tauri_plugin_autostart::ManagerExt;
use tokio::join;
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

#[derive(Serialize)]
pub struct Status {
    link: LinkState,
    session: Option<String>,
}

#[tauri::command]
pub fn status(beep: Service<'_>) -> Status {
    Status {
        link: beep.link_state(),
        session: beep.session(),
    }
}

#[tauri::command]
pub fn link_offer(beep: Service<'_>) -> Result<LinkView> {
    beep.link_offer(&crate::computer_name())
}

#[tauri::command]
pub async fn send_prompt(
    window: WebviewWindow,
    beep: Service<'_>,
    session: Option<String>,
    message: String,
    images: Vec<DataImage>,
    files: Vec<FileUpload>,
    on_event: Channel<BarEvent>,
) -> Result<String> {
    let sent = Sent {
        message,
        images,
        files,
        research: false,
        voice: None,
        spoken: false,
    };
    let session_id = beep::chat_id(session.as_deref());
    // Made current before the run, so its own notices stay quiet.
    center::remember_for(&window, &beep, Chat::Session(Some(session_id.to_string())));
    beep.post(session_id, sent, Pick::default(), on_event)
        .await?;
    Ok(session_id.to_string())
}

/// Sends the chat's `index`th prompt again on another model or thinking,
/// dropping it and the turns after it first. Admins only.
#[tauri::command]
pub async fn rerun_prompt(
    window: WebviewWindow,
    beep: Service<'_>,
    session: String,
    index: usize,
    pick: Pick,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    center::remember_for(&window, &beep, Chat::Session(Some(session.clone())));
    beep.rerun(session, index, pick, on_event).await
}

/// An approval card that was answered, for every webview to drop.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApprovalAnswered {
    session: String,
    query_id: String,
}

/// Answers an approval card, from the bar, the orb or the command center,
/// and streams the run it resumes into `on_event`.
#[tauri::command]
pub async fn answer_approval(
    app: AppHandle,
    beep: Service<'_>,
    voice: State<'_, Arc<Voice>>,
    session: String,
    query_id: String,
    decision: Decision,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    let run = beep
        .approve(&session, query_id.clone(), decision.into())
        .await?;
    // Every webview showing the card drops it.
    let answered = ApprovalAnswered {
        session: session.clone(),
        query_id: query_id.clone(),
    };
    if let Err(e) = app.emit("approval-answered", answered) {
        log::warn!("telling the windows an approval was answered: {e}");
    }
    // Whichever of the bar and the orb answered, the other drops its card,
    // and live mode reads the rest of the reply out when it waits on it.
    let news = voice.answered(Answered {
        session,
        query_id,
        run: run.clone(),
        decision,
    });
    let followed = beep.follow_answered(run, on_event, news.as_ref()).await;
    if let (Err(e), Some(news)) = (&followed, &news) {
        news.tell(RunNews::Failed(e.to_string()));
    }
    followed
}

/// What the orb's approval card shows, if live mode waits on one.
#[tauri::command]
pub fn live_approval(voice: State<'_, Arc<Voice>>) -> Option<Waiting> {
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
pub fn notice_card(voice: State<'_, Arc<Voice>>) -> Option<NoticeCard> {
    voice.notice_card()
}

/// Play on the orb's notice card: it is read aloud as a voice note.
#[tauri::command]
pub fn play_notice(voice: State<'_, Arc<Voice>>, id: u64) {
    voice.play_card(id);
}

/// Open on the orb's notice card, or a click on it: the bar opens on its
/// chat.
#[tauri::command]
pub fn open_notice(voice: State<'_, Arc<Voice>>, id: u64) {
    voice.open_card(id);
}

/// × on the orb's notice card.
#[tauri::command]
pub fn dismiss_notice(voice: State<'_, Arc<Voice>>, id: u64) {
    voice.dismiss_card(id);
}

/// The pointer is on the orb's notice card, which keeps it up, or left it.
#[tauri::command]
pub fn hold_notice(voice: State<'_, Arc<Voice>>, id: u64, held: bool) {
    voice.hold_card(id, held);
}

/// Puts the orb's window at `width` by `height`, kept in the top right.
#[tauri::command]
pub fn fit_orb(app: AppHandle, width: f64, height: f64) -> Result<()> {
    Ok(orb::fit(&app, width, height)?)
}

#[tauri::command]
pub async fn active_run(beep: Service<'_>, session: String) -> Result<Option<String>> {
    beep.active_run(&session).await
}

#[tauri::command]
pub async fn follow_run(
    beep: Service<'_>,
    run_id: String,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    beep.follow_run(run_id, on_event).await
}

#[tauri::command]
pub async fn cancel_run(beep: Service<'_>, run_id: String) -> Result<()> {
    beep.call(CancelRun { id: run_id }).await
}

#[tauri::command]
pub async fn list_sessions(beep: Service<'_>) -> Result<Vec<Session>> {
    beep.sessions().await
}

#[tauri::command]
pub async fn search_sessions(beep: Service<'_>, query: String) -> Result<Vec<SessionMatch>> {
    beep.search(query).await
}

#[tauri::command]
pub async fn open_session(
    window: WebviewWindow,
    beep: Service<'_>,
    id: String,
) -> Result<Vec<Turn>> {
    // Made current before loading, so leaving the chat meanwhile is not undone.
    center::remember_for(&window, &beep, Chat::Session(Some(id.clone())));
    beep.history(id).await
}

/// The files other than pictures sent in a session, by the `at` of the
/// user turn they came with.
#[tauri::command]
pub async fn session_files(beep: Service<'_>, id: String) -> Result<Vec<SentFiles>> {
    beep.call(GetSessionFiles { session_id: id }).await
}

/// The web pages each turn of a session used, by the `at` of its user
/// turn, for the source chips under its reply.
#[tauri::command]
pub async fn session_sources(beep: Service<'_>, id: String) -> Result<Vec<TurnSources>> {
    let session_id =
        Uuid::parse_str(&id).map_err(|_| DesktopError::BadRequest("not a chat".into()))?;
    beep.call(GetSessionSources { session_id }).await
}

#[tauri::command]
pub fn new_chat(window: WebviewWindow, beep: Service<'_>) {
    center::remember_for(&window, &beep, Chat::Session(None));
}

#[tauri::command]
pub async fn rename_session(beep: Service<'_>, id: String, title: String) -> Result<()> {
    beep.call(RenameSession { id, title }).await
}

#[tauri::command]
pub async fn pin_session(beep: Service<'_>, id: String, pinned: bool) -> Result<()> {
    beep.call(PinSession { id, pinned }).await
}

#[tauri::command]
pub async fn delete_session(beep: Service<'_>, shown: State<'_, Shown>, id: String) -> Result<()> {
    beep.call(DeleteSession { id: id.clone() }).await?;
    if let Ok(chat) = Uuid::parse_str(&id) {
        beep.chats().forget(chat);
    }
    if beep.session().as_deref() == Some(id.as_str()) {
        beep.set_session(None);
    }
    if shown.get().session.as_deref() == Some(id.as_str()) {
        shown.set_session(None);
    }
    Ok(())
}

#[tauri::command]
pub async fn share_session(beep: Service<'_>, id: String) -> Result<SharedLink> {
    beep.call(ShareSession { id }).await
}

#[tauri::command]
pub async fn unshare_session(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(UnshareSession { id }).await
}

// ---------- Groups ----------

/// What the bar shows for a code that finds no group.
const NO_GROUP: &str = "No group has this code. Check it, or ask for a new one.";

#[tauri::command]
pub async fn list_groups(beep: Service<'_>) -> Result<Vec<GroupView>> {
    let groups = beep.call(ListGroups {}).await?;
    Ok(groups.into_iter().map(GroupView::from).collect())
}

#[tauri::command]
pub async fn open_group(
    window: WebviewWindow,
    beep: Service<'_>,
    id: String,
) -> Result<GroupChatView> {
    // Made current before loading, so closing the group meanwhile is not undone.
    center::remember_for(&window, &beep, Chat::Group(Some(id.clone())));
    if let Some(view) = Uuid::parse_str(&id)
        .ok()
        .and_then(|g| beep.chats().group(g))
    {
        return Ok(view);
    }
    let (chat, images) = join!(
        beep.call(GetGroup { id: id.clone() }),
        beep.call(GetGroupImages {
            group_id: id.clone()
        }),
    );
    let mut view = GroupChatView::from(chat?);
    let mut pictures = Pictures::new(images);
    for message in &mut view.messages {
        message.images = pictures.take(message.seq);
    }
    Ok(view)
}

/// The webview left the group, so its messages notify again.
#[tauri::command]
pub fn close_group(window: WebviewWindow, beep: Service<'_>) {
    center::remember_for(&window, &beep, Chat::Group(None));
}

#[tauri::command]
pub async fn send_to_group(
    beep: Service<'_>,
    group_id: String,
    message: String,
    images: Vec<DataImage>,
) -> Result<MessageView> {
    let images = image_uris(images)?;
    let sent = beep
        .call(SendToGroup {
            group_id,
            request_id: Uuid::new_v4(),
            message,
            images,
        })
        .await?;
    Ok(sent.into())
}

#[tauri::command]
pub async fn create_group(
    beep: Service<'_>,
    name: String,
    member_name: String,
) -> Result<GroupView> {
    let group = beep
        .call(CreateGroup {
            id: Uuid::new_v4(),
            name,
            member_name,
        })
        .await?;
    Ok(group.into())
}

#[tauri::command]
pub async fn group_invite(beep: Service<'_>, id: String, reset: bool) -> Result<InviteView> {
    let link = beep.call(GroupInvite { id, reset }).await?;
    Ok(InviteView {
        code: format_group_code(&link.token),
        qr_svg: qr_svg(&link.url)?,
        url: link.url,
    })
}

/// The group a typed code or pasted link opens, before joining it.
#[derive(Serialize)]
pub struct Found {
    token: String,
    preview: GroupPreview,
}

#[tauri::command]
pub async fn peek_group(beep: Service<'_>, code: String) -> Result<Found> {
    let token = group_code(&code).ok_or_else(|| DesktopError::BadRequest(NO_GROUP.into()))?;
    match beep
        .call(PeekGroup {
            token: token.clone(),
        })
        .await
    {
        Ok(preview) => Ok(Found { token, preview }),
        Err(DesktopError::Api(ApiError::NotFound {})) => {
            Err(DesktopError::BadRequest(NO_GROUP.into()))
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn join_group(
    beep: Service<'_>,
    token: String,
    member_name: String,
) -> Result<GroupView> {
    let name = member_name.clone();
    match beep.call(JoinGroup { token, member_name }).await {
        Ok(group) => Ok(group.into()),
        Err(DesktopError::Api(ApiError::Conflict { .. })) => Err(DesktopError::BadRequest(
            format!("Someone in the group already goes by {name}"),
        )),
        Err(DesktopError::Api(ApiError::NotFound {})) => Err(DesktopError::BadRequest(
            "This code no longer works. Ask for a new one.".into(),
        )),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn leave_group(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(LeaveGroup { id: id.clone() }).await?;
    if let Ok(group) = Uuid::parse_str(&id) {
        beep.chats().forget(group);
    }
    beep.set_group(None);
    Ok(())
}

#[tauri::command]
pub async fn pin_group(beep: Service<'_>, id: String, pinned: bool) -> Result<()> {
    beep.call(PinGroup { id, pinned }).await
}

#[tauri::command]
pub async fn share_group_apps(beep: Service<'_>, id: String, shared: bool) -> Result<()> {
    beep.call(ShareGroupApps { id, shared }).await
}

#[tauri::command]
pub async fn list_group_reminders(
    beep: Service<'_>,
    group_id: String,
) -> Result<Vec<GroupReminder>> {
    beep.call(ListGroupReminders { group_id }).await
}

#[tauri::command]
pub async fn delete_group_reminder(beep: Service<'_>, group_id: String, id: String) -> Result<()> {
    beep.call(DeleteGroupReminder { group_id, id }).await
}

#[tauri::command]
pub async fn answer_group_approval(beep: Service<'_>, query_id: String, allow: bool) -> Result<()> {
    beep.call(AnswerGroupApproval { query_id, allow }).await
}

// ---------- Files ----------

/// What the bar shows for a file or picture the server no longer has.
const GONE: &str = "No longer available";

/// Whether `id` names a file and nothing else, so it is safe in a path.
fn file_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Downloads file `id` to `path` unless an earlier download left it there.
async fn cached(beep: &Beep, id: &str, path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    match beep.download(id, path).await {
        Err(DesktopError::Api(ApiError::NotFound {})) => Err(DesktopError::BadRequest(GONE.into())),
        other => other,
    }
}

/// File `id` in the cache, downloaded the first time it is asked for.
async fn cached_file(app: &AppHandle, beep: &Beep, id: &str, name: &str) -> Result<PathBuf> {
    if !file_id(id) {
        return Err(DesktopError::BadRequest("bad file".into()));
    }
    let dir = app.path().app_cache_dir()?.join("files").join(id);
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let path = dir.join(file_name(name));
    cached(beep, id, &path).await?;
    Ok(path)
}

/// Opens a file Beep sent, downloading it into the cache the first time.
#[tauri::command]
pub async fn open_file(app: AppHandle, beep: Service<'_>, id: String, name: String) -> Result<()> {
    let path = cached_file(&app, &beep, &id, &name).await?;
    Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// Saves a file Beep sent to Downloads, under a name not taken there yet,
/// and returns where it went.
#[tauri::command]
pub async fn save_file(
    app: AppHandle,
    beep: Service<'_>,
    id: String,
    name: String,
) -> Result<PathBuf> {
    let from = cached_file(&app, &beep, &id, &name).await?;
    let to = free_name(&app.path().download_dir()?, &file_name(&name));
    fs::copy(&from, &to).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Shows a file `save_file` put in Downloads in Finder.
#[tauri::command]
pub fn reveal_file(app: AppHandle, path: PathBuf) -> Result<()> {
    let downloads = app.path().download_dir()?;
    if path.parent() != Some(downloads.as_path()) || !path.is_file() {
        return Err(DesktopError::BadRequest("not a saved file".into()));
    }
    Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// `name` in `dir`, or the first of `name 2`, `name 3` and so on that is
/// free, the way Finder numbers a copy.
pub(crate) fn free_name(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2..1000)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} {}{ext}", Uuid::new_v4())))
}

/// A picture someone sent, as a data URL the bar can draw. Its bytes are
/// kept in the cache, so each one downloads once.
#[tauri::command]
pub async fn picture(
    app: AppHandle,
    beep: Service<'_>,
    id: String,
    mime: String,
) -> Result<String> {
    if !file_id(&id) || !image_mime(&mime) {
        return Err(DesktopError::BadRequest("bad picture".into()));
    }
    let dir = app.path().app_cache_dir()?.join("pictures");
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let path = dir.join(&id);
    cached(&beep, &id, &path).await?;
    let bytes = fs::read(&path).map_err(anyhow::Error::from)?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

/// Whether `mime` is a picture type plain enough to put in a data URL.
fn image_mime(mime: &str) -> bool {
    mime.strip_prefix("image/").is_some_and(|sub| {
        !sub.is_empty()
            && sub
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
    })
}

/// `name` without anything that would leave the file's folder.
pub(crate) fn file_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .trim_start_matches('.');
    if base.is_empty() {
        "file".into()
    } else {
        base.into()
    }
}

#[tauri::command]
pub async fn unlink(app: AppHandle, beep: Service<'_>) -> Result<()> {
    beep.unlink().await?;
    app.state::<Machine>().sync(&beep);
    let _ = app.emit("link-state", beep.link_state());
    crate::refresh_tray(&app);
    Ok(())
}

#[derive(Serialize)]
pub struct Settings {
    name: String,
    version: String,
    autostart: bool,
    notice_voice: NoticeVoice,
    /// In live mode a pause sends, rather than only "send".
    send_on_pause: bool,
}

#[tauri::command]
pub fn settings(app: AppHandle, beep: Service<'_>) -> Settings {
    Settings {
        name: crate::computer_name(),
        version: app.package_info().version.to_string(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        notice_voice: beep.notice_voice(),
        send_on_pause: beep.send_on_pause(),
    }
}

/// Voice notes: ask first, read aloud right away, or off.
#[tauri::command]
pub fn set_notice_voice(beep: Service<'_>, choice: NoticeVoice) {
    beep.set_notice_voice(choice);
}

/// Whether a pause sends in live mode, rather than only saying "send".
#[tauri::command]
pub fn set_send_on_pause(beep: Service<'_>, voice: State<'_, Arc<Voice>>, enabled: bool) {
    beep.set_send_on_pause(enabled);
    voice.set_end_mode();
}

/// A tap in the live view: interrupts Beep, or leaves while listening.
#[tauri::command]
pub fn live_tap(voice: State<'_, Arc<Voice>>) {
    voice.tap();
}

/// Leaves live mode, keeping what was said as a draft.
#[tauri::command]
pub fn leave_live(voice: State<'_, Arc<Voice>>) {
    voice.leave_live();
}

/// Esc or Cancel in live mode: cancels a reply being written or read out,
/// or leaves, keeping what was said.
#[tauri::command]
pub fn cancel_live(voice: State<'_, Arc<Voice>>) {
    voice.cancel();
}

/// Opens or closes the sheet of what to say in live mode; `None` toggles.
#[tauri::command]
pub fn live_sheet(voice: State<'_, Arc<Voice>>, open: Option<bool>) {
    voice.sheet(open);
}

/// Chats live mode talked in, for the mic in the chat list.
#[tauri::command]
pub fn live_chats(beep: Service<'_>) -> Vec<String> {
    beep.live_chats().iter().map(Uuid::to_string).collect()
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<()> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| DesktopError::Other(e.into()))
}

#[tauri::command]
pub fn mac_control(beep: Service<'_>, machine: State<'_, Machine>) -> machine::View {
    machine.view(&beep)
}

#[tauri::command]
pub fn set_mac_control(
    beep: Service<'_>,
    machine: State<'_, Machine>,
    enabled: bool,
) -> machine::View {
    beep.set_use_mac(enabled);
    machine.sync(&beep);
    machine.view(&beep)
}

#[tauri::command]
pub fn allow_mac_permission(permission: Permission) {
    machine::allow(permission);
}

/// Forgets this computer's key and starts again with a new one, the only
/// way back after the phone removed it.
#[tauri::command]
pub fn start_over(app: AppHandle, beep: Service<'_>) -> Result<()> {
    beep.chats().clear();
    crate::forget_identity(&app)?;
    app.restart()
}

#[tauri::command]
pub fn resize_bar(app: AppHandle, height: f64) -> Result<()> {
    Ok(bar::resize(&app, height)?)
}

#[tauri::command]
pub fn hide_bar(app: AppHandle) {
    bar::dismiss(&app);
}

/// A click on the orb: the bar comes back in its place.
#[tauri::command]
pub fn show_bar(app: AppHandle) {
    bar::show(&app, None);
}

/// The app has no menu bar of its own, so the bar handles ⌘Q.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/// Opens a link from a reply in the browser, never in the bar.
#[tauri::command]
pub fn open_url(url: String) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(DesktopError::BadRequest("only web links open".into()));
    }
    Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{file_name, free_name, image_mime};
    use std::{env, fs};
    use uuid::Uuid;

    #[test]
    fn image_mime_takes_picture_types_only() {
        assert!(image_mime("image/jpeg"));
        assert!(image_mime("image/svg+xml"));
        assert!(!image_mime("image/"));
        assert!(!image_mime("text/html"));
        assert!(!image_mime("image/png;base64,x"));
    }

    #[test]
    fn free_name_numbers_a_taken_name() {
        let dir = env::temp_dir().join(format!("beep-free-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("the temp folder is writable");
        assert_eq!(free_name(&dir, "chart.png"), dir.join("chart.png"));
        fs::write(dir.join("chart.png"), b"x").expect("the temp folder is writable");
        assert_eq!(free_name(&dir, "chart.png"), dir.join("chart 2.png"));
        fs::write(dir.join("notes"), b"x").expect("the temp folder is writable");
        assert_eq!(free_name(&dir, "notes"), dir.join("notes 2"));
        fs::remove_dir_all(&dir).expect("the temp folder is removable");
    }

    #[test]
    fn file_name_stays_in_its_folder() {
        assert_eq!(file_name("Pass.pdf"), "Pass.pdf");
        assert_eq!(file_name("../../etc/passwd"), "passwd");
        assert_eq!(file_name(".hidden"), "hidden");
        assert_eq!(file_name(".."), "file");
        assert_eq!(file_name(""), "file");
    }
}

/// Whether Beep is writing in group `group_id`, which the local copy does
/// not know. The bar asks while it waits for Beep, instead of reloading
/// the whole group.
#[tauri::command]
pub async fn group_typing(beep: Service<'_>, group_id: String) -> Result<bool> {
    let groups = beep.call(ListGroups {}).await?;
    Ok(groups
        .into_iter()
        .any(|group| group.id == group_id && group.beep_typing))
}

/// A memory as the bar shows it.
#[derive(Serialize)]
pub struct MemoryView {
    id: String,
    content: String,
    category: String,
    /// `user`, `learned`, `email` or `slack`.
    source: &'static str,
    updated: String,
    valid_from: Option<String>,
    valid_until: Option<String>,
    quote: Option<String>,
}

impl From<Memory> for MemoryView {
    fn from(m: Memory) -> Self {
        Self {
            id: m.id,
            content: m.content,
            category: m.category,
            source: m.source.as_str(),
            updated: m.updated,
            valid_from: m.valid_from,
            valid_until: m.valid_until,
            quote: m.quote,
        }
    }
}

#[tauri::command]
pub async fn list_memories(beep: Service<'_>) -> Result<Vec<MemoryView>> {
    let memories = beep.call(ListMemories {}).await?;
    Ok(memories.into_iter().map(MemoryView::from).collect())
}

/// What a memory is about when it is not the user.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ScopeView {
    /// A machine by its key, with its name while the machine is known.
    Machine {
        id: String,
        name: Option<String>,
    },
    Project {
        name: String,
    },
    /// One of the user's accounts, by app, such as `github`.
    Account {
        app: String,
    },
}

impl From<MemoryScope> for ScopeView {
    fn from(scope: MemoryScope) -> Self {
        match scope {
            MemoryScope::Machine { id, name } => Self::Machine { id, name },
            MemoryScope::Project { name } => Self::Project { name },
            MemoryScope::Account { app } => Self::Account { app },
        }
    }
}

/// A memory's subject, by `Memory::id`.
#[derive(Serialize)]
pub struct MemoryScopeView {
    id: String,
    scope: ScopeView,
}

/// What each memory that is not about the user is about. A memory missing
/// here is about the user.
#[tauri::command]
pub async fn list_memory_scopes(beep: Service<'_>) -> Result<Vec<MemoryScopeView>> {
    let scoped = beep.call(ListMemoryScopes {}).await?;
    Ok(scoped
        .into_iter()
        .map(|memory| MemoryScopeView {
            id: memory.id,
            scope: memory.scope.into(),
        })
        .collect())
}

/// The reads of mail and calendars that added or confirmed memories, newest
/// first.
#[tauri::command]
pub async fn list_memory_reads(beep: Service<'_>) -> Result<Vec<MemoryRead>> {
    beep.call(ListMemoryReads {}).await
}

#[tauri::command]
pub async fn update_memory(beep: Service<'_>, id: String, content: String) -> Result<()> {
    beep.call(UpdateMemory { id, content }).await
}

#[tauri::command]
pub async fn delete_memory(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(DeleteMemory { id }).await
}

#[tauri::command]
pub async fn clear_memories(beep: Service<'_>) -> Result<u64> {
    beep.call(ClearMemories {}).await
}

#[tauri::command]
pub async fn list_crons(beep: Service<'_>) -> Result<Vec<Cron>> {
    beep.call(ListCrons {}).await
}

#[tauri::command]
pub async fn create_cron(
    beep: Service<'_>,
    label: String,
    prompt: String,
    schedule: String,
) -> Result<String> {
    beep.call(CreateCron {
        label,
        prompt,
        schedule,
    })
    .await
}

#[tauri::command]
pub async fn toggle_cron(beep: Service<'_>, id: String) -> Result<bool> {
    beep.call(ToggleCron { id }).await
}

/// Renames routine `id` or changes when it runs. `None` leaves that part as it is.
#[tauri::command]
pub async fn update_cron(
    beep: Service<'_>,
    id: String,
    label: Option<String>,
    schedule: Option<String>,
) -> Result<()> {
    beep.call(UpdateCron {
        id,
        enabled: None,
        label,
        schedule,
    })
    .await
}

#[tauri::command]
pub async fn delete_cron(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(DeleteCron { id }).await
}

/// Runs routine `id` now and returns the session it runs in.
#[tauri::command]
pub async fn run_cron(beep: Service<'_>, id: String) -> Result<String> {
    beep.call(RunCron { id }).await
}

/// A skill as the bar shows it: the server's fields, and the body drawn as HTML.
#[derive(Serialize)]
pub struct SkillView {
    #[serde(flatten)]
    skill: Skill,
    html: String,
}

impl From<Skill> for SkillView {
    fn from(skill: Skill) -> Self {
        let html = markdown::to_html(&skill.body);
        Self { skill, html }
    }
}

/// The changes the skill editor sends; a field left out stays as it is.
#[derive(Deserialize)]
pub struct SkillChange {
    name: Option<String>,
    description: Option<String>,
    body: Option<String>,
    enabled: Option<bool>,
}

#[tauri::command]
pub async fn list_skills(beep: Service<'_>) -> Result<Vec<SkillView>> {
    let skills = beep.call(ListSkills {}).await?;
    Ok(skills.into_iter().map(SkillView::from).collect())
}

#[tauri::command]
pub async fn update_skill(beep: Service<'_>, id: String, change: SkillChange) -> Result<SkillView> {
    let skill = beep
        .call(UpdateSkill {
            id,
            name: change.name,
            description: change.description,
            body: change.body,
            enabled: change.enabled,
        })
        .await?;
    Ok(skill.into())
}

#[tauri::command]
pub async fn delete_skill(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(DeleteSkill { id }).await
}

/// Puts back the version before the skill's last change.
#[tauri::command]
pub async fn undo_skill(beep: Service<'_>, id: String) -> Result<SkillView> {
    Ok(beep.call(UndoSkill { id }).await?.into())
}

/// Whether agents are on, so the model view can say what Auto does. Admins only.
#[tauri::command]
pub async fn get_agents(beep: Service<'_>) -> Result<Agents> {
    beep.call(GetAgents {}).await
}

/// Turns agents on or off for everyone. Admins only.
#[tauri::command]
pub async fn set_agents(beep: Service<'_>, enabled: bool) -> Result<Agents> {
    beep.call(SetAgents { enabled }).await
}

/// The agent each turn of session `id` ran as, by the `at` of its user turn.
#[tauri::command]
pub async fn session_agents(beep: Service<'_>, id: String) -> Result<Vec<TurnAgent>> {
    beep.call(GetSessionAgents { session_id: id }).await
}

/// The caller's model settings. `choices` is empty unless they are an
/// admin, and the bar shows the picker only when it is not.
#[tauri::command]
pub async fn get_models(beep: Service<'_>) -> Result<Models> {
    beep.call(GetModels {}).await
}

#[tauri::command]
pub async fn set_model(beep: Service<'_>, id: String) -> Result<Models> {
    beep.call(SetModel { id }).await
}

#[tauri::command]
pub async fn set_thinking(beep: Service<'_>, level: ReasoningLevel) -> Result<Models> {
    beep.call(SetThinking { level }).await
}

/// A stock chart over another range, for a chart's range buttons.
#[tauri::command]
pub async fn stock_chart(beep: Service<'_>, symbol: String, range: ChartRange) -> Result<Chart> {
    beep.call(StockChart { symbol, range }).await
}

/// The transit card a `get_route` call planned.
#[tauri::command]
pub async fn route_card(beep: Service<'_>, call_id: String) -> Result<RouteCard> {
    beep.call(GetRouteCard { call_id }).await
}

#[tauri::command]
pub async fn set_auto_model(beep: Service<'_>, enabled: bool) -> Result<Models> {
    beep.call(SetAutoModel { enabled }).await
}
