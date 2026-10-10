//! What the bar's page can ask the app to do.

use crate::{
    bar,
    beep::{Answer, BarEvent, Beep, LinkView, Pictures, Turn, qr_svg},
    error::{DesktopError, Result},
    groups::{GroupChatView, GroupView, InviteView, MessageView},
    state::LinkState,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AnswerGroupApproval, ApiError, ApprovalDecision, CreateGroup, DeleteGroupReminder,
    DeleteSession, GetGroup, GetGroupImages, GroupInvite, GroupPreview, GroupReminder, JoinGroup,
    LeaveGroup, ListGroupReminders, ListGroups, PeekGroup, PinGroup, PinSession, RenameSession,
    SendToGroup, Session, SessionMatch, ShareGroupApps, ShareSession, SharedLink, UnshareSession,
    format_group_code, group_code,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command, sync::Arc};
use tauri::{AppHandle, Manager, State, ipc::Channel};
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
    beep: Service<'_>,
    session: Option<String>,
    message: String,
    on_event: Channel<BarEvent>,
) -> Result<String> {
    beep.send(session, message, on_event).await
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Always,
    Deny,
}

impl From<Decision> for ApprovalDecision {
    fn from(d: Decision) -> Self {
        match d {
            Decision::Allow => Self::Allow {},
            Decision::Always => Self::Always {},
            Decision::Deny => Self::Deny {},
        }
    }
}

#[tauri::command]
pub async fn answer_approval(
    beep: Service<'_>,
    session: String,
    query_id: String,
    decision: Decision,
    on_event: Channel<BarEvent>,
) -> Result<()> {
    beep.answer(Answer {
        session,
        query_id,
        decision: decision.into(),
        channel: on_event,
    })
    .await
}

#[tauri::command]
pub async fn cancel_run(beep: Service<'_>, run_id: String) -> Result<()> {
    beep.cancel(run_id).await
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
pub async fn open_session(beep: Service<'_>, id: String) -> Result<Vec<Turn>> {
    let turns = beep.history(id.clone()).await?;
    beep.set_session(Some(id));
    beep.set_group(None);
    Ok(turns)
}

#[tauri::command]
pub fn new_chat(beep: Service<'_>) {
    beep.set_session(None);
    beep.set_group(None);
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
pub async fn delete_session(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(DeleteSession { id: id.clone() }).await?;
    if let Ok(chat) = Uuid::parse_str(&id) {
        beep.chats().forget(chat);
    }
    if beep.session().as_deref() == Some(id.as_str()) {
        beep.set_session(None);
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
pub async fn open_group(beep: Service<'_>, id: String) -> Result<GroupChatView> {
    if let Some(view) = Uuid::parse_str(&id)
        .ok()
        .and_then(|g| beep.chats().group(g))
    {
        beep.set_group(Some(id));
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
    beep.set_group(Some(id));
    Ok(view)
}

/// The bar left the group, so its messages notify again.
#[tauri::command]
pub fn close_group(beep: Service<'_>) {
    beep.set_group(None);
}

#[tauri::command]
pub async fn send_to_group(
    beep: Service<'_>,
    group_id: String,
    message: String,
) -> Result<MessageView> {
    let sent = beep
        .call(SendToGroup {
            group_id,
            request_id: Uuid::new_v4(),
            message,
            images: Vec::new(),
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

/// Opens a file Beep sent, downloading it into the cache the first time.
#[tauri::command]
pub async fn open_file(app: AppHandle, beep: Service<'_>, id: String, name: String) -> Result<()> {
    if !file_id(&id) {
        return Err(DesktopError::BadRequest("bad file".into()));
    }
    let dir = app.path().app_cache_dir()?.join("files").join(&id);
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let path = dir.join(file_name(&name));
    cached(&beep, &id, &path).await?;
    Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
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
fn file_name(name: &str) -> String {
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
pub async fn unlink(beep: Service<'_>) -> Result<()> {
    beep.unlink().await
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
    bar::hide(&app);
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
    use super::{file_name, image_mime};

    #[test]
    fn image_mime_takes_picture_types_only() {
        assert!(image_mime("image/jpeg"));
        assert!(image_mime("image/svg+xml"));
        assert!(!image_mime("image/"));
        assert!(!image_mime("text/html"));
        assert!(!image_mime("image/png;base64,x"));
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
