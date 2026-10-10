//! A copy of the user's chats on disk, kept current from the server's
//! update log: on connect the app asks for everything after the last update
//! it holds, then follows `StreamUpdates` for more. Opening a chat reads
//! the copy, so it shows at once.

use crate::{
    beep::{self, Beep, Turn},
    error::Result,
    groups::{Author, GroupChatView, GroupView, MessageView},
    markdown,
    state::LinkState,
};
use beep_proto::{
    ChatInfo, ChatKind, ChatMessage, GetUpdates, MessageBody, Session, SessionMatch, StreamUpdates,
    Update, UpdatePage, search::match_snippet,
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
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

/// How long to wait before following the log again after it broke off.
const RETRY: Duration = Duration::from_secs(15);
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
}

/// `index.json`: the cursor and the chats, without their messages.
#[derive(Serialize, Deserialize)]
struct Index {
    last: i64,
    chats: Vec<ChatInfo>,
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
            .filter_map(|chat| {
                let (snippet, from_user) = synced.messages.get(&chat.id)?.values().rev().find_map(
                    |message| match &message.body {
                        MessageBody::User { content, .. } => {
                            Some((match_snippet(content, query)?, true))
                        }
                        MessageBody::Assistant { content } => {
                            Some((match_snippet(content, query)?, false))
                        }
                        _ => None,
                    },
                )?;
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
            group: GroupView {
                id: id.to_string(),
                name: chat.title.clone(),
                members: chat.members.clone(),
                last: messages.last().cloned(),
                updated_at: synced.latest(id),
                beep_typing: false,
                pinned: chat.pinned,
                apps_shared: chat.apps_shared,
            },
            messages,
        })
    }
}

impl Synced {
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
fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    fs::write(&partial, bytes)?;
    fs::rename(&partial, path)?;
    Ok(())
}

/// A session's messages as the bar draws them: each turn's tool steps and
/// suggested replies sit on its last reply, or on an empty one when it has
/// none, and a waiting approval follows its turn.
fn turns<'a>(messages: impl IntoIterator<Item = &'a ChatMessage>) -> Vec<Turn> {
    let mut out = Vec::new();
    let mut open = OpenTurn::default();
    for message in messages {
        match &message.body {
            MessageBody::User {
                content, images, ..
            } => {
                open.finish(&mut out);
                out.push(Turn::User {
                    text: content.clone(),
                    images: images.clone(),
                });
            }
            MessageBody::Scheduled { label, .. } => {
                open.finish(&mut out);
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
                });
            }
            MessageBody::Tool {
                name,
                output,
                success,
                ..
            } => match beep::suggested(name, output) {
                Some(replies) => open.replies = replies,
                None if name == beep::SUGGEST_REPLIES => {}
                None => open.tools.push(beep::tool_step(name, output, *success)),
            },
            MessageBody::Approval { query_id, approval } => {
                open.approvals.push(Turn::Approval {
                    query_id: query_id.clone(),
                    app: approval.app.clone(),
                    action: approval.action.clone(),
                    fields: beep::fields(approval.fields.clone()),
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
    /// Where the turn's latest reply is in the output.
    reply: Option<usize>,
    tools: Vec<beep::ToolStep>,
    replies: Vec<String>,
    approvals: Vec<Turn>,
}

impl OpenTurn {
    fn finish(&mut self, out: &mut Vec<Turn>) {
        let turn = mem::take(self);
        match turn.reply.and_then(|at| out.get_mut(at)) {
            Some(Turn::Assistant { tools, replies, .. }) => {
                *tools = turn.tools;
                *replies = turn.replies;
            }
            _ if turn.tools.is_empty() && turn.replies.is_empty() => {}
            _ => out.push(Turn::Assistant {
                html: String::new(),
                tools: turn.tools,
                replies: turn.replies,
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
    loop {
        if beep.link_state() != LinkState::Linked {
            tokio::time::sleep(UNLINKED_POLL).await;
            continue;
        }
        if let Err(e) = follow_once(&app, &beep).await {
            log::info!("following chat updates: {e}");
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn follow_once(app: &AppHandle, beep: &Beep) -> Result<()> {
    let mut newest = beep.stream(StreamUpdates {}).await?;
    while let Some(last) = newest.next().await? {
        if last != beep.chats().last() {
            catch_up(app, beep).await?;
        }
    }
    Ok(())
}

/// Asks for pages until the copy holds every update.
async fn catch_up(app: &AppHandle, beep: &Beep) -> Result<()> {
    loop {
        let after = beep.chats().last();
        let page = beep.call(GetUpdates { after }).await?;
        let more = page.more;
        let changed = beep.chats().apply(page);
        for id in changed.sessions {
            let _ = app.emit("sessions-changed", id.to_string());
        }
        for id in changed.groups {
            let _ = app.emit("group-changed", id.to_string());
        }
        if !more {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{GroupMember, TurnInfo};

    fn message(id: i64, body: MessageBody) -> ChatMessage {
        ChatMessage {
            id,
            turn_id: None,
            timestamp: id * 1000,
            body,
        }
    }

    fn user(id: i64, content: &str) -> ChatMessage {
        message(
            id,
            MessageBody::User {
                content: content.into(),
                images: Vec::new(),
                turn: TurnInfo {
                    model: String::new(),
                    auto: None,
                    reasoning: String::new(),
                    memory_score: None,
                },
            },
        )
    }

    fn tool(id: i64, name: &str, output: &str) -> ChatMessage {
        message(
            id,
            MessageBody::Tool {
                call_id: format!("c{id}"),
                name: name.into(),
                output: output.into(),
                success: true,
            },
        )
    }

    fn reply(id: i64, content: &str) -> ChatMessage {
        message(
            id,
            MessageBody::Assistant {
                content: content.into(),
            },
        )
    }

    fn info(id: Uuid, kind: ChatKind, title: &str) -> ChatInfo {
        ChatInfo {
            id,
            kind,
            title: title.into(),
            pinned: false,
            members: Vec::new(),
            apps_shared: true,
        }
    }

    fn page(last: i64, updates: Vec<Update>) -> UpdatePage {
        UpdatePage {
            updates,
            last,
            more: false,
            reset: false,
        }
    }

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("beep-sync-{name}-{}", Uuid::new_v4()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn tools_and_replies_sit_on_the_turns_last_reply() {
        let messages = [
            user(1, "Where is lunch?"),
            tool(2, "open_maps", "{}"),
            reply(3, "Here it is."),
            tool(4, beep::SUGGEST_REPLIES, r#"{"replies":["Thanks"]}"#),
            user(5, "Remind me"),
            tool(6, "cron_create", "failed"),
        ];
        let turns = turns(&messages);
        let [
            Turn::User { .. },
            Turn::Assistant { tools, replies, .. },
            Turn::User { .. },
            Turn::Assistant {
                html, tools: last, ..
            },
        ] = turns.as_slice()
        else {
            panic!("unexpected turns: {turns:?}");
        };
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "open_maps");
        assert_eq!(replies, &["Thanks"]);
        assert!(html.is_empty());
        assert_eq!(last[0].name, "cron_create");
    }

    #[test]
    fn the_copy_follows_the_log_and_survives_a_restart() {
        let dir = dir("follow");
        let (session, group) = (Uuid::new_v4(), Uuid::new_v4());
        let chats = Chats::load(dir.clone());
        assert!(chats.sessions().is_none());

        let changed = chats.apply(page(
            5,
            vec![
                Update::Chat {
                    update_id: 1,
                    chat: info(session, ChatKind::Session {}, "Lunch"),
                },
                Update::Message {
                    update_id: 2,
                    chat_id: session,
                    message: user(10, "hi"),
                },
                Update::Message {
                    update_id: 3,
                    chat_id: session,
                    message: reply(11, "hello"),
                },
                Update::Chat {
                    update_id: 4,
                    chat: ChatInfo {
                        members: vec![GroupMember {
                            name: "Ali".into(),
                            you: true,
                            creator: true,
                            joined_at: 0,
                        }],
                        ..info(group, ChatKind::Group {}, "Trip")
                    },
                },
                Update::Message {
                    update_id: 5,
                    chat_id: group,
                    message: message(
                        12,
                        MessageBody::Beep {
                            content: "**hey**".into(),
                        },
                    ),
                },
            ],
        ));
        assert_eq!(changed.sessions, HashSet::from([session]));
        assert_eq!(changed.groups, HashSet::from([group]));
        let sessions = chats.sessions().unwrap();
        assert_eq!(sessions[0].title, "Lunch");
        assert_eq!(sessions[0].updated_at, 11_000);
        assert_eq!(chats.session(session).unwrap().len(), 2);
        let view = chats.group(group).unwrap();
        assert_eq!(view.messages[0].seq, 12);
        assert!(view.messages[0].html.contains("<strong>hey</strong>"));
        assert!(chats.session(group).is_none());

        chats.apply(page(
            7,
            vec![
                Update::Deleted {
                    update_id: 6,
                    chat_id: session,
                    message_id: 11,
                },
                Update::ChatDeleted {
                    update_id: 7,
                    chat_id: group,
                },
            ],
        ));
        let reopened = Chats::load(dir.clone());
        assert_eq!(reopened.last(), 7);
        assert_eq!(reopened.session(session).unwrap().len(), 1);
        assert!(reopened.group(group).is_none());
        assert!(!chat_file(&dir, group).exists());

        let changed = reopened.apply(UpdatePage {
            reset: true,
            ..page(1, Vec::new())
        });
        assert_eq!(changed.sessions, HashSet::from([session]));
        assert!(reopened.sessions().unwrap().is_empty());
        reopened.clear();
        assert!(!dir.exists());
    }

    #[test]
    fn search_finds_the_latest_message_that_holds_the_words() {
        let dir = dir("search");
        let (lunch, trip) = (Uuid::new_v4(), Uuid::new_v4());
        let chats = Chats::load(dir.clone());
        assert!(chats.search("lunch").is_none());
        let mut updates = vec![
            Update::Chat {
                update_id: 1,
                chat: info(lunch, ChatKind::Session {}, "Lunch"),
            },
            Update::Chat {
                update_id: 2,
                chat: info(trip, ChatKind::Session {}, "Trip"),
            },
        ];
        let messages = [
            (lunch, user(10, "Where is Kimpton?")),
            (lunch, tool(11, "open_maps", "kimpton")),
            (lunch, reply(12, "The kimpton is on Main St.")),
            (trip, user(13, "Book the Kimpton")),
            (trip, tool(14, "book", "kimpton")),
        ];
        for (at, (chat_id, message)) in messages.into_iter().enumerate() {
            updates.push(Update::Message {
                update_id: 3 + at as i64,
                chat_id,
                message,
            });
        }
        chats.apply(page(7, updates));

        let found = chats.search(" KIMPTON ").unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].session.title, "Trip");
        assert!(found[0].from_user);
        assert_eq!(found[1].snippet, "The kimpton is on Main St.");
        assert!(!found[1].from_user);
        assert!(chats.search("open_maps").unwrap().is_empty());
        assert!(chats.search("k").unwrap().is_empty());
        chats.clear();
    }
}
