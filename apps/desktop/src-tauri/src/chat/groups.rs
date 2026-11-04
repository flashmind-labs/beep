//! Group chats: how the bar draws them, and its commands to open, send to,
//! create, join, leave and invite to a group.

use crate::{
    beep::{DataImage, Field, Service, image_uris, qr_svg},
    center::{self, Chat, Shown},
    chat::{history::Pictures, markdown},
    error::{DesktopError, Result},
};
use beep_proto::{
    AnswerGroupApproval, ApiError, CreateGroup, DeleteGroupReminder, GetGroup, GetGroupImages,
    Group, GroupAuthor, GroupChat, GroupInvite, GroupMember, GroupMessage, GroupPreview,
    GroupReminder, ImageRef, JoinGroup, LeaveGroup, ListGroupReminders, ListGroups, PeekGroup,
    PinGroup, SendToGroup, ShareGroupApps, format_group_code, group_code,
};
use serde::Serialize;
use tauri::{State, WebviewWindow};
use tokio::join;
use uuid::Uuid;

/// What the bar shows for a code that finds no group.
const NO_GROUP: &str = "No group has this code. Check it, or ask for a new one.";

#[derive(Debug, Clone, Serialize)]
pub struct GroupView {
    pub id: String,
    pub name: String,
    pub members: Vec<GroupMember>,
    pub last: Option<MessageView>,
    pub updated_at: i64,
    pub beep_typing: bool,
    pub pinned: bool,
    pub apps_shared: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Author {
    Member { name: String, you: bool },
    Beep,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageView {
    pub seq: i64,
    pub author: Author,
    /// As written. People's messages show as plain text.
    pub text: String,
    /// Beep's replies as sanitized HTML, empty for people's.
    pub html: String,
    pub timestamp: i64,
    /// The pictures sent with it, filled in from `GetGroupImages`.
    pub images: Vec<ImageRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupChatView {
    pub group: GroupView,
    pub messages: Vec<MessageView>,
}

/// An invite as the bar shows it: the code to read out and its QR.
#[derive(Debug, Clone, Serialize)]
pub struct InviteView {
    pub code: String,
    pub url: String,
    pub qr_svg: String,
}

impl From<GroupMessage> for MessageView {
    fn from(m: GroupMessage) -> Self {
        let (author, html) = match m.author {
            GroupAuthor::Member { name, you } => (Author::Member { name, you }, String::new()),
            GroupAuthor::Beep {} => (Author::Beep, markdown::to_html(&m.content)),
        };
        Self {
            seq: m.seq,
            author,
            text: m.content,
            html,
            timestamp: m.timestamp,
            images: Vec::new(),
        }
    }
}

impl From<Group> for GroupView {
    fn from(g: Group) -> Self {
        Self {
            id: g.id,
            name: g.name,
            members: g.members,
            last: g.last.map(MessageView::from),
            updated_at: g.updated_at,
            beep_typing: g.beep_typing,
            pinned: g.pinned,
            apps_shared: g.apps_shared,
        }
    }
}

impl From<GroupChat> for GroupChatView {
    fn from(c: GroupChat) -> Self {
        Self {
            group: c.group.into(),
            messages: c.messages.into_iter().map(MessageView::from).collect(),
        }
    }
}

/// A tool call in a group waiting for this user's OK, which the bar shows
/// on the group.
#[derive(Debug, Clone, Serialize)]
pub struct GroupApproval {
    pub group_id: String,
    pub query_id: String,
    pub app: String,
    pub action: String,
    pub fields: Vec<Field>,
}

#[tauri::command]
pub async fn list_groups(beep: Service<'_>) -> Result<Vec<GroupView>> {
    // The copy answers at once; the server only before it first caught up.
    if let Some(groups) = beep.chats().groups() {
        return Ok(groups);
    }
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
pub async fn leave_group(beep: Service<'_>, shown: State<'_, Shown>, id: String) -> Result<()> {
    beep.call(LeaveGroup { id: id.clone() }).await?;
    if let Ok(group) = Uuid::parse_str(&id) {
        beep.chats().forget(group);
    }
    if beep.group().as_deref() == Some(id.as_str()) {
        beep.set_group(None);
    }
    shown.leave_group(&id);
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
