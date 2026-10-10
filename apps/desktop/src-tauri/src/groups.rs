//! Group chats as the bar draws them. The wire's `GroupAuthor` is a tuple,
//! so these name its parts for the page.

use crate::{beep::Field, markdown};
use beep_proto::{Group, GroupAuthor, GroupChat, GroupMember, GroupMessage, ImageRef};
use serde::Serialize;

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
