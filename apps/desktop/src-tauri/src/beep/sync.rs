//! A copy of the user's chats on disk, kept current from the server's
//! update log: on connect the app asks for everything after the last update
//! it holds, then follows the newest update id on the events stream. Opening a chat reads
//! the copy, so it shows at once.

use crate::{
    beep::state::LinkState,
    beep::{Beep, Field},
    chat::groups::{Author, GroupChatView, GroupView, MessageView},
    chat::history::{self, Turn},
    chat::markdown,
    error::Result,
    notices::alerts,
    palette::PaletteCache,
};
use beep_node::link::reconnect::Backoff;
use beep_proto::{
    ChatInfo, ChatKind, ChatMessage, GetUpdates, MessageBody, Session, SessionMatch, Update,
    UpdatePage, search::match_snippet,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet},
    fs, mem,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

/// How often to look again while the app is not linked.
const UNLINKED_POLL: Duration = Duration::from_secs(5);
/// Sessions a search returns, as many as the server's.
const SEARCH_RESULTS: usize = 30;

/// The chats the server has sent, by id, and the last update applied.
pub struct Chats {
    dir: PathBuf,
    synced: Mutex<Synced>,
}

#[derive(Default)]
struct Synced {
    /// The copy is worth showing: it was read from disk or caught up once.
    ready: bool,
    last: i64,
    chats: HashMap<Uuid, ChatInfo>,
    messages: HashMap<Uuid, BTreeMap<i64, ChatMessage>>,
    /// Alerts the user has not replied to: sessions the lists leave out.
    alerts: HashSet<Uuid>,
}

/// `index.json`: the cursor and the chats, without their messages.
#[derive(Serialize, Deserialize)]
struct Index {
    last: i64,
    chats: Vec<ChatInfo>,
    /// The alerts the lists leave out, kept so a start with no server
    /// does not show them as chats.
    #[serde(default)]
    alerts: Vec<Uuid>,
}

/// The chats a page changed, for the bar to reload.
#[derive(Debug, Default, PartialEq)]
pub struct Changed {
    pub sessions: HashSet<Uuid>,
    pub groups: HashSet<Uuid>,
}

impl Chats {
    /// Reads the copy kept in `dir`. A missing or unreadable one starts
    /// empty and fills from the server.
    pub fn load(dir: PathBuf) -> Self {
        let mut synced = Synced::default();
        let index = fs::read(dir.join("index.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Index>(&bytes).ok());
        if let Some(index) = index {
            synced.ready = true;
            synced.last = index.last;
            synced.alerts = index.alerts.into_iter().collect();

            for chat in index.chats {
                let messages: Vec<ChatMessage> = fs::read(chat_file(&dir, chat.id))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .unwrap_or_default();
                synced
                    .messages
                    .insert(chat.id, messages.into_iter().map(|m| (m.id, m)).collect());
                synced.chats.insert(chat.id, chat);
            }
        }
        Self {
            dir,
            synced: Mutex::new(synced),
        }
    }

    fn synced(&self) -> MutexGuard<'_, Synced> {
        self.synced.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn last(&self) -> i64 {
        self.synced().last
    }

    /// Applies `page` and saves what it changed.
    pub fn apply(&self, page: UpdatePage) -> Changed {
        let mut synced = self.synced();
        let mut changed = Changed::default();
        let mut touched = HashSet::new();
        let mut gone = HashSet::new();

        if page.reset {
            for (id, chat) in synced.chats.drain() {
                changed.note(&chat);
                gone.insert(id);
            }

            synced.messages.clear();
        }

        for update in page.updates {
            match update {
                Update::Message {
                    chat_id, message, ..
                } => {
                    synced
                        .messages
                        .entry(chat_id)
                        .or_default()
                        .insert(message.id, message);
                    touched.insert(chat_id);
                }
                Update::Deleted {
                    chat_id,
                    message_id,
                    ..
                } => {
                    if let Some(messages) = synced.messages.get_mut(&chat_id) {
                        messages.remove(&message_id);
                    }

                    touched.insert(chat_id);
                }
                Update::Chat { chat, .. } => {
                    touched.insert(chat.id);
                    gone.remove(&chat.id);
                    synced.chats.insert(chat.id, chat);
                }
                Update::ChatDeleted { chat_id, .. } => {
                    if let Some(chat) = synced.chats.remove(&chat_id) {
                        changed.note(&chat);
                    }

                    synced.messages.remove(&chat_id);
                    touched.remove(&chat_id);
                    gone.insert(chat_id);
                }
            }
        }

        for id in &touched {
            if let Some(chat) = synced.chats.get(id) {
                changed.note(chat);
            }
        }

        synced.last = page.last;
        if !page.more {
            synced.ready = true;
        }
        if let Err(e) = self.save(&synced, &touched, &gone) {
            log::warn!("saving chats: {e:#}");
        }

        changed
    }

    /// Writes the chats in `touched`, removes those in `gone`, then the
    /// index. A crash in between leaves the index behind, so the next
    /// launch asks for the same updates again.
    fn save(
        &self,
        synced: &Synced,
        touched: &HashSet<Uuid>,
        gone: &HashSet<Uuid>,
    ) -> anyhow::Result<()> {
        fs::create_dir_all(self.dir.join("chats"))?;

        for id in gone {
            let _ = fs::remove_file(chat_file(&self.dir, *id));
        }

        for id in touched {
            let messages: Vec<&ChatMessage> = synced
                .messages
                .get(id)
                .map(|m| m.values().collect())
                .unwrap_or_default();
            write(&chat_file(&self.dir, *id), &serde_json::to_vec(&messages)?)?;
        }

        let index = Index {
            last: synced.last,
            chats: synced.chats.values().cloned().collect(),
            alerts: synced.alerts.iter().copied().collect(),
        };
        write(&self.dir.join("index.json"), &serde_json::to_vec(&index)?)?;
        Ok(())
    }

    /// Drops the copy, when this computer starts over with a new key.
    pub fn clear(&self) {
        *self.synced() = Synced::default();
        let _ = fs::remove_dir_all(&self.dir);
    }

    /// Takes a chat out of the copy, after deleting or leaving it here,
    /// so the list does not show it until the log says so.
    pub fn forget(&self, id: Uuid) {
        let mut synced = self.synced();
        synced.chats.remove(&id);
        synced.messages.remove(&id);
    }

    /// Leaves `alerts`, the sessions the assistant started on its own that
    /// the user has not replied to, out of `sessions` and `search`. Says
    /// whether that changed what those show.
    pub fn hide_alerts(&self, alerts: HashSet<Uuid>) -> bool {
        let mut synced = self.synced();
        let changed = synced.alerts != alerts;
        synced.alerts = alerts;

        if changed
            && synced.ready
            && let Err(e) = self.save(&synced, &HashSet::new(), &HashSet::new())
        {
            log::warn!("keeping the alerts: {e}");
        }

        changed
    }

    /// Whether `id` is an alert the lists leave out.
    pub fn hides(&self, id: Uuid) -> bool {
        self.synced().alerts.contains(&id)
    }

    /// The sessions, pinned first then newest, or `None` before the copy
    /// is worth showing.
    pub fn sessions(&self) -> Option<Vec<Session>> {
        let synced = self.synced();

        if !synced.ready {
            return None;
        }

        let mut sessions: Vec<Session> = synced
            .chats
            .values()
            .filter(|chat| matches!(chat.kind, ChatKind::Session {}))
            .filter(|chat| !synced.alerts.contains(&chat.id))
            .map(|chat| Session {
                id: chat.id.to_string(),
                title: chat.title.clone(),
                pinned: chat.pinned,
                updated_at: synced.latest(chat.id),
            })
            .collect();
        sessions.sort_by_key(|s| (Reverse(s.pinned), Reverse(s.updated_at)));
        Some(sessions)
    }

    /// The sessions where the user or Beep wrote `query`, newest first,
    /// each with the latest message that holds it, or `None` before the
    /// copy is worth showing.
    pub fn search(&self, query: &str) -> Option<Vec<SessionMatch>> {
        let synced = self.synced();

        if !synced.ready {
            return None;
        }

        let query = query.trim();

        if query.chars().count() < 2 {
            return Some(Vec::new());
        }

        let mut found: Vec<SessionMatch> = synced
            .chats
            .values()
            .filter(|chat| matches!(chat.kind, ChatKind::Session {}))
            .filter(|chat| !synced.alerts.contains(&chat.id))
            .filter_map(|chat| {
                let messages = synced.messages.get(&chat.id)?;
                let mut snippets = Vec::new();
                let mut from_user = true;
                for term in query.split_whitespace() {
                    let (snippet, user) =
                        messages
                            .values()
                            .rev()
                            .find_map(|message| match &message.body {
                                MessageBody::User { content, .. } => {
                                    Some((match_snippet(content, term)?, true))
                                }
                                MessageBody::Assistant { content } => {
                                    Some((match_snippet(content, term)?, false))
                                }
                                _ => None,
                            })?;
                    from_user &= user;
                    if !snippets.contains(&snippet) {
                        snippets.push(snippet);
                    }
                }
                let snippet = snippets.join(" · ");
                Some(SessionMatch {
                    session: Session {
                        id: chat.id.to_string(),
                        title: chat.title.clone(),
                        pinned: chat.pinned,
                        updated_at: synced.latest(chat.id),
                    },
                    snippet,
                    from_user,
                })
            })
            .collect();
        found.sort_by_key(|found| Reverse(found.session.updated_at));
        found.truncate(SEARCH_RESULTS);
        Some(found)
    }

    /// Session `id`'s turns, or `None` when the copy does not have it.
    pub fn session(&self, id: Uuid) -> Option<Vec<Turn>> {
        let synced = self.synced();
        let chat = synced.chats.get(&id)?;

        if !synced.ready || !matches!(chat.kind, ChatKind::Session {}) {
            return None;
        }

        let messages = synced.messages.get(&id)?;
        Some(turns(messages.values()))
    }

    /// Group `id` with its messages, or `None` when the copy does not have it.
    pub fn group(&self, id: Uuid) -> Option<GroupChatView> {
        let synced = self.synced();
        let chat = synced.chats.get(&id)?;

        if !synced.ready || !matches!(chat.kind, ChatKind::Group {}) {
            return None;
        }

        let messages: Vec<MessageView> = synced
            .messages
            .get(&id)
            .into_iter()
            .flat_map(|m| m.values())
            .filter_map(group_message)
            .collect();
        Some(GroupChatView {
            group: synced.group_view(chat, messages.last().cloned()),
            messages,
        })
    }

    /// The groups, newest first, or `None` before the copy is worth
    /// showing. Whether Beep is typing is not in the copy.
    pub fn groups(&self) -> Option<Vec<GroupView>> {
        let synced = self.synced();

        if !synced.ready {
            return None;
        }

        let mut groups: Vec<GroupView> = synced
            .chats
            .values()
            .filter(|chat| matches!(chat.kind, ChatKind::Group {}))
            .map(|chat| {
                let last = synced
                    .messages
                    .get(&chat.id)
                    .and_then(|m| m.values().rev().find_map(group_message));
                synced.group_view(chat, last)
            })
            .collect();
        groups.sort_by_key(|g| Reverse(g.updated_at));
        Some(groups)
    }
}

impl Synced {
    fn group_view(&self, chat: &ChatInfo, last: Option<MessageView>) -> GroupView {
        GroupView {
            id: chat.id.to_string(),
            name: chat.title.clone(),
            members: chat.members.clone(),
            last,
            updated_at: self.latest(chat.id),
            beep_typing: false,
            pinned: chat.pinned,
            apps_shared: chat.apps_shared,
        }
    }

    /// When chat `id` last had a message, in milliseconds.
    fn latest(&self, id: Uuid) -> i64 {
        self.messages
            .get(&id)
            .and_then(|m| m.values().map(|m| m.timestamp).max())
            .unwrap_or(0)
    }
}

impl Changed {
    fn note(&mut self, chat: &ChatInfo) {
        match chat.kind {
            ChatKind::Session {} => self.sessions.insert(chat.id),
            ChatKind::Group {} => self.groups.insert(chat.id),
        };
    }
}

fn chat_file(dir: &Path, id: Uuid) -> PathBuf {
    dir.join("chats").join(format!("{id}.json"))
}

/// Writes `bytes` to `path` through a temporary file, so a reader never
/// sees half of it.
pub fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    fs::write(&partial, bytes)?;
    fs::rename(&partial, path)?;
    Ok(())
}

/// A session's messages as the bar draws them: each turn's tool steps and
/// suggested replies sit on its last reply, or on an empty one when it has
/// none, and a waiting approval follows its turn. A step from another turn
/// than the open one, such as a routine's notice with no turn, closes it, so
/// the turn before keeps its own reply and tools.
fn turns<'a>(messages: impl IntoIterator<Item = &'a ChatMessage>) -> Vec<Turn> {
    let mut out = Vec::new();
    let mut open = OpenTurn::default();

    for message in messages {
        let opens = matches!(
            message.body,
            MessageBody::User { .. } | MessageBody::Scheduled { .. }
        );
        if !opens && message.turn_id != open.id {
            open.finish(&mut out);
            open.id = message.turn_id;
        }
        match &message.body {
            MessageBody::User {
                content,
                images,
                turn,
            } => {
                open.finish(&mut out);
                open.id = message.turn_id;
                open.model = turn.model.clone();
                out.push(Turn::User {
                    text: content.clone(),
                    at: message.timestamp,
                    images: images.clone(),
                    model: turn.model.clone(),
                    thinking: turn
                        .auto
                        .as_ref()
                        .map(|pick| pick.thinking.to_string())
                        .unwrap_or_default(),
                });
            }
            MessageBody::Scheduled { label, turn } => {
                open.finish(&mut out);
                open.id = message.turn_id;
                open.model = turn.model.clone();
                out.push(Turn::Scheduled {
                    label: label.clone(),
                });
            }
            MessageBody::Assistant { content } => {
                open.reply = Some(out.len());
                out.push(Turn::Assistant {
                    html: markdown::to_html(content),
                    tools: Vec::new(),
                    replies: Vec::new(),
                    model: String::new(),
                });
            }
            MessageBody::Tool { name, output, .. } if name == history::SUGGEST_REPLIES => {
                if let Some(replies) = history::suggested_replies(output) {
                    open.replies = replies;
                }
            }
            MessageBody::Tool {
                call_id,
                name,
                output,
                success,
            } => open
                .tools
                .push(history::tool_step(name, call_id, output, *success)),
            MessageBody::Approval { query_id, approval } => {
                open.approvals.push(Turn::Approval {
                    query_id: query_id.clone(),
                    app: approval.app.clone(),
                    action: approval.action.clone(),
                    fields: approval.fields.iter().cloned().map(Field::from).collect(),
                });
            }
            MessageBody::Member { .. } | MessageBody::Beep { .. } => {}
        }
    }

    open.finish(&mut out);
    out
}

#[derive(Default)]
struct OpenTurn {
    /// The message that opened the turn, `None` for steps outside any turn.
    id: Option<i64>,
    /// Where the turn's latest reply is in the output.
    reply: Option<usize>,
    tools: Vec<history::ToolStep>,
    replies: Vec<String>,
    approvals: Vec<Turn>,
    /// The model that answered, from the message that opened the turn.
    model: String,
}

impl OpenTurn {
    fn finish(&mut self, out: &mut Vec<Turn>) {
        let turn = mem::take(self);

        match turn.reply.and_then(|at| out.get_mut(at)) {
            Some(Turn::Assistant {
                tools,
                replies,
                model,
                ..
            }) => {
                *tools = turn.tools;
                *replies = turn.replies;
                *model = turn.model;
            }
            _ if turn.tools.is_empty() && turn.replies.is_empty() => {}
            _ => out.push(Turn::Assistant {
                html: String::new(),
                tools: turn.tools,
                replies: turn.replies,
                model: turn.model,
            }),
        }

        out.extend(turn.approvals);
    }
}

fn group_message(message: &ChatMessage) -> Option<MessageView> {
    let (author, text, html, images) = match &message.body {
        MessageBody::Member {
            name,
            you,
            content,
            images,
        } => (
            Author::Member {
                name: name.clone(),
                you: *you,
            },
            content.clone(),
            String::new(),
            images.clone(),
        ),
        MessageBody::Beep { content } => (
            Author::Beep,
            content.clone(),
            markdown::to_html(content),
            Vec::new(),
        ),
        _ => return None,
    };
    Some(MessageView {
        seq: message.id,
        author,
        text,
        html,
        timestamp: message.timestamp,
        images,
    })
}

/// Keeps the copy current for the app's life: catches up whenever the
/// server says its log moved, and tells the bar which chats changed.
pub async fn follow(app: AppHandle, beep: Arc<Beep>) {
    // Soon after it broke off: the user is at this Mac.
    let mut backoff = Backoff::eager();

    loop {
        if beep.link_state() != LinkState::Linked {
            tokio::time::sleep(UNLINKED_POLL).await;
            continue;
        }

        if let Err(e) = follow_once(&app, &beep).await {
            log::info!("following chat updates: {e}");
        }

        tokio::time::sleep(backoff.pause()).await;
    }
}

async fn follow_once(app: &AppHandle, beep: &Beep) -> Result<()> {
    // The lists must leave the alerts out from the first chat they show.
    alerts::refresh(app).await;
    let mut newest = beep.watch_updates().await?;
    // Ends once the stream is no longer followed, as after a sign-out.
    while newest.changed().await.is_ok() {
        let last = *newest.borrow_and_update();

        match last {
            Some(last) if last != beep.chats().last() => catch_up(app, beep).await?,
            Some(_) => {}
            None => return Ok(()),
        }
    }

    Ok(())
}

/// Asks for pages until the copy holds every update, then rebuilds the
/// tray's Recent from it when a chat changed.
async fn catch_up(app: &AppHandle, beep: &Beep) -> Result<()> {
    let mut sessions_changed = false;
    let mut alert_changed = false;

    loop {
        let after = beep.chats().last();
        let page = beep.call(GetUpdates { after }).await?;
        let more = page.more;
        let changed = beep.chats().apply(page);

        for id in changed.sessions {
            sessions_changed = true;
            // A reply from another device makes an alert a chat.
            alert_changed |= beep.chats().hides(id);
            let _ = app.emit("sessions-changed", id.to_string());
        }

        for id in changed.groups {
            let _ = app.emit("group-changed", id.to_string());
        }

        if !more {
            if alert_changed {
                alerts::refresh(app).await;
            }

            if sessions_changed {
                app.state::<PaletteCache>().chats_changed();
                tauri::async_runtime::spawn(crate::tray::refresh_recent(app.clone()));
            }

            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "../tests/beep_sync.rs"]
mod tests;
