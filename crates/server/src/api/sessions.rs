//! Chats: listing, searching, sharing and deleting them, and what they hold.

use super::{Reply, chat_id, found};
use crate::{
    agent::steps,
    api::router::Cx,
    protocol::{search::match_snippet, *},
    store::{CopiedShare, DeletedTurn, FoundSession, TurnsToDelete},
};
use chrono::DateTime;
use flashmind::core::{Conversation, ConversationEntry, EntryKind};
use std::collections::HashSet;
use uuid::Uuid;

pub(super) async fn list_sessions(cx: Cx, _: ListSessions) -> Reply<Vec<Session>> {
    Ok(cx.user.sessions().await?)
}

pub(super) async fn get_session(cx: Cx, GetSession { id }: GetSession) -> Reply<Vec<HistoryEntry>> {
    let messages = cx
        .user
        .session_messages(chat_id(&id)?)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(steps::history(&messages))
}

pub(super) async fn share_session(cx: Cx, ShareSession { id }: ShareSession) -> Reply<SharedLink> {
    let id = chat_id(&id)?;
    let messages = cx
        .user
        .session_messages(id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let entries = steps::history(&messages);
    let share = cx
        .user
        .share_session(id, &shared_conversation(&entries))
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(SharedLink {
        url: share_url(&share.token),
        token: share.token,
        title: share.title,
        shared_at: share.shared_at.timestamp_millis(),
    })
}

pub(super) async fn unshare_session(cx: Cx, UnshareSession { id }: UnshareSession) -> Reply<()> {
    cx.user.unshare_session(chat_id(&id)?).await?;
    Ok(())
}

pub(super) async fn get_share(cx: Cx, GetShare { token }: GetShare) -> Reply<SharedSession> {
    let share = cx
        .app
        .store
        .share(&token)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(SharedSession {
        entries: share_history(&share.conversation),
        title: share.title,
        shared_at: share.shared_at.timestamp_millis(),
    })
}

pub(super) async fn copy_share(
    cx: Cx,
    CopyShare { token, session_id }: CopyShare,
) -> Reply<Session> {
    let id = session_id.to_string();
    match cx.user.copy_share(&token, session_id).await? {
        CopiedShare::Copied => {}
        CopiedShare::NotFound => return Err(ApiError::NotFound {}),
        CopiedShare::Taken => {
            return Err(ApiError::Conflict {
                message: "session id is taken".into(),
            });
        }
    }
    cx.user
        .sessions()
        .await?
        .into_iter()
        .find(|session| session.id == id)
        .ok_or(ApiError::Internal {})
}

pub(super) async fn get_session_images(
    cx: Cx,
    GetSessionImages { session_id }: GetSessionImages,
) -> Reply<Vec<SentImages>> {
    Ok(cx.user.session_pictures(chat_id(&session_id)?).await?)
}

pub(super) async fn get_session_files(
    cx: Cx,
    GetSessionFiles { session_id }: GetSessionFiles,
) -> Reply<Vec<SentFiles>> {
    Ok(cx.user.session_files(chat_id(&session_id)?).await?)
}

pub(super) async fn get_session_agents(
    cx: Cx,
    GetSessionAgents { session_id }: GetSessionAgents,
) -> Reply<Vec<TurnAgent>> {
    // No turn runs as an agent now, so the apps show none.
    if !cx.user.owns_session(chat_id(&session_id)?).await? {
        return Err(ApiError::NotFound {});
    }
    Ok(Vec::new())
}

pub(super) async fn get_tool_labels(
    cx: Cx,
    GetToolLabels { session_id }: GetToolLabels,
) -> Reply<Vec<ToolLabel>> {
    cx.user
        .tool_labels(session_id)
        .await?
        .ok_or(ApiError::NotFound {})
}

pub(super) async fn get_session_sources(
    cx: Cx,
    GetSessionSources { session_id }: GetSessionSources,
) -> Reply<Vec<TurnSources>> {
    cx.user
        .session_sources(session_id)
        .await?
        .ok_or(ApiError::NotFound {})
}

pub(super) async fn get_route(cx: Cx, GetRoute { call_id }: GetRoute) -> Reply<RouteCardV1> {
    get_route_card(cx, GetRouteCard { call_id })
        .await
        .map(RouteCardV1::from)
}

pub(super) async fn get_route_card(
    cx: Cx,
    GetRouteCard { call_id }: GetRouteCard,
) -> Reply<RouteCard> {
    cx.user
        .route_card(&call_id)
        .await?
        .ok_or(ApiError::NotFound {})
}

pub(super) async fn get_file(cx: Cx, GetFile { id, offset }: GetFile) -> Reply<FilePart> {
    cx.user
        .file_part(&id, offset, FILE_PART)
        .await?
        .ok_or(ApiError::NotFound {})
}

/// Only the user and assistant message text of `entries`, as a share keeps it.
fn shared_conversation(entries: &[HistoryEntry]) -> Conversation {
    let at = |millis| DateTime::from_timestamp_millis(millis).unwrap_or_default();
    let mut conversation = Conversation::new();
    for entry in entries {
        let mut shared = match entry {
            HistoryEntry::User { content, .. } => ConversationEntry::user(content.clone()),
            HistoryEntry::Assistant { content, .. } if !content.is_empty() => {
                ConversationEntry::assistant(content.clone())
            }
            HistoryEntry::Assistant { .. }
            | HistoryEntry::Scheduled { .. }
            | HistoryEntry::Approval { .. } => continue,
        };
        if let HistoryEntry::User { timestamp, .. } | HistoryEntry::Assistant { timestamp, .. } =
            entry
        {
            shared.timestamp = at(*timestamp);
        }
        conversation.add(shared);
    }
    conversation
}

/// A share's messages as history: it keeps only user messages and replies.
fn share_history(conversation: &Conversation) -> Vec<HistoryEntry> {
    conversation
        .entries()
        .iter()
        .filter_map(|entry| {
            let timestamp = entry.timestamp.timestamp_millis();
            match &entry.kind {
                EntryKind::User { content, .. } => Some(HistoryEntry::User {
                    content: content.clone(),
                    timestamp,
                    auto: None,
                }),
                EntryKind::Assistant { content, .. } if !content.is_empty() => {
                    Some(HistoryEntry::Assistant {
                        content: content.clone(),
                        timestamp,
                        tools: Vec::new(),
                        memory_score: None,
                        reasoning: String::new(),
                        model: String::new(),
                    })
                }
                _ => None,
            }
        })
        .collect()
}

/// Sessions the database may hand over for a search, before tool-only matches are dropped.
const SEARCH_CANDIDATES: i64 = 100;
/// Sessions a search returns.
const SEARCH_RESULTS: usize = 30;
pub(super) async fn search_sessions(
    cx: Cx,
    SearchSessions { query }: SearchSessions,
) -> Reply<Vec<SessionMatch>> {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Ok(Vec::new());
    }
    if query.len() > 200 {
        return Err(ApiError::bad_request("search is at most 200 bytes"));
    }
    let found = cx.user.search_sessions(query, SEARCH_CANDIDATES).await?;
    Ok(found
        .into_iter()
        .filter_map(|found| session_match(found, query))
        .take(SEARCH_RESULTS)
        .collect())
}

/// The latest message the user or Beep wrote in `found` that holds `query`.
fn session_match(found: FoundSession, query: &str) -> Option<SessionMatch> {
    Some(SessionMatch {
        snippet: match_snippet(&found.text, query)?,
        session: found.session,
        from_user: found.from_user,
    })
}

pub(super) async fn rename_session(
    cx: Cx,
    RenameSession { id, title }: RenameSession,
) -> Reply<()> {
    if title.is_empty() {
        return Err(ApiError::bad_request("title required"));
    }
    found(cx.user.rename_session(chat_id(&id)?, &title).await?)
}

pub(super) async fn delete_session(cx: Cx, DeleteSession { id }: DeleteSession) -> Reply<()> {
    found(cx.user.delete_session(chat_id(&id)?).await?)
}

pub(super) async fn delete_sessions(cx: Cx, DeleteSessions { ids }: DeleteSessions) -> Reply<u64> {
    if ids.len() > 200 {
        return Err(ApiError::bad_request("at most 200 sessions at once"));
    }
    let ids: Vec<Uuid> = ids.iter().filter_map(|id| chat_id(id).ok()).collect();
    Ok(cx.user.delete_sessions(&ids).await?)
}

pub(super) async fn existing_sessions(
    cx: Cx,
    ExistingSessions { ids }: ExistingSessions,
) -> Reply<Vec<String>> {
    if ids.len() > 1000 {
        return Err(ApiError::bad_request("at most 1000 sessions at once"));
    }
    let parsed: Vec<Uuid> = ids.iter().filter_map(|id| chat_id(id).ok()).collect();
    let existing: HashSet<Uuid> = (cx.user.existing_sessions(&parsed).await?)
        .into_iter()
        .collect();
    // Answered in the caller's own spelling of each id.
    Ok(ids
        .into_iter()
        .filter(|id| chat_id(id).is_ok_and(|id| existing.contains(&id)))
        .collect())
}

pub(super) async fn delete_turn(
    cx: Cx,
    DeleteTurn {
        session_id,
        timestamp,
    }: DeleteTurn,
) -> Reply<()> {
    deleted_turn(
        cx.user
            .delete_turn(chat_id(&session_id)?, timestamp, TurnsToDelete::One)
            .await?,
    )
}

pub(super) async fn delete_turns_from(
    cx: Cx,
    DeleteTurnsFrom {
        session_id,
        timestamp,
    }: DeleteTurnsFrom,
) -> Reply<()> {
    deleted_turn(
        cx.user
            .delete_turn(chat_id(&session_id)?, timestamp, TurnsToDelete::Onward)
            .await?,
    )
}

fn deleted_turn(deleted: DeletedTurn) -> Reply<()> {
    match deleted {
        DeletedTurn::Deleted => Ok(()),
        DeletedTurn::NotFound => Err(ApiError::NotFound {}),
        DeletedTurn::Busy => Err(ApiError::Conflict {
            message: "the chat has a run in progress".into(),
        }),
    }
}

pub(super) async fn pin_session(cx: Cx, PinSession { id, pinned }: PinSession) -> Reply<()> {
    found(cx.user.pin_session(chat_id(&id)?, pinned).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::types::ToolCall;
    use serde_json::json;

    #[test]
    fn a_share_keeps_only_the_message_text() {
        let mut chat = Conversation::new();
        chat.add(ConversationEntry::system_message("Location: home"));
        chat.add(ConversationEntry::user("check my mail"));
        chat.add(ConversationEntry::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "mail".into(),
                arguments: json!({}),
            }],
            Some("thinking".into()),
        ));
        chat.add(ConversationEntry::tool("c1", "secret inbox"));
        chat.add(ConversationEntry::assistant_with_reasoning(
            "Two new emails.",
            Some("more thinking".into()),
        ));
        let shared = shared_conversation(&share_history(&chat));
        let json = serde_json::to_string(&shared).unwrap();
        for private in ["home", "secret", "thinking", "c1"] {
            assert!(!json.contains(private), "{private} leaked into {json}");
        }
        let texts: Vec<_> = shared.entries().iter().map(|e| e.content()).collect();
        assert_eq!(texts, ["check my mail", "Two new emails."]);
    }
}
