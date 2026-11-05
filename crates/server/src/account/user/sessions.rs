//! The user's sessions: listing, renaming, sharing, messages and turns.

use super::User;
use crate::{
    agent::images::Picture,
    app::UserNotice,
    protocol::{
        FilePart, ImageRef, RouteCard, Run, SentFiles, SentImages, Session, ToolLabel, TurnSources,
        UpdatePage, Upload, WebSource,
    },
    store::{
        AgentImageBytes, AgentImageOf, AlertRow, CopiedShare, DeletedTurn, FinishedTurn,
        FoundSession, Listing, NewAttachment, NewFile, NewPicture, PicturesOf, RunStatus,
        SavedTurn, Share, StoredContext, StoredMessage, TurnRecord, TurnsToDelete,
    },
};
use anyhow::Result;
use flashmind::core::Conversation;
use tokio::sync::watch;
use uuid::Uuid;

impl User {
    /// The user's sessions, pinned first, then newest first.
    pub async fn sessions(&self) -> Result<Vec<Session>> {
        self.store.sessions(&self.key).await
    }

    /// The user's sessions that are chats: alerts not replied to are left out.
    pub async fn chats(&self) -> Result<Vec<Session>> {
        self.store.chats(&self.key).await
    }

    /// Up to `limit` sessions with a message that holds `query`, ignoring case.
    pub async fn search_sessions(&self, query: &str, limit: i64) -> Result<Vec<FoundSession>> {
        self.store
            .search_sessions(&self.key, query, limit, Listing::All)
            .await
    }

    /// As `search_sessions`, leaving out alerts not replied to.
    pub async fn search_chats(&self, query: &str, limit: i64) -> Result<Vec<FoundSession>> {
        self.store
            .search_sessions(&self.key, query, limit, Listing::Chats)
            .await
    }

    /// The user's alerts, newest first.
    pub async fn alerts(&self) -> Result<Vec<AlertRow>> {
        self.store.alerts(&self.key).await
    }

    /// Makes session `chat` an alert that says `text`, and tells the apps.
    pub(crate) async fn add_alert(&self, chat: Uuid, source: &str, text: &str) -> Result<()> {
        self.store.add_alert(&self.key, chat, source, text).await?;
        self.changed("");
        Ok(())
    }

    pub async fn read_alerts(&self, ids: &[Uuid]) -> Result<()> {
        self.store.read_alerts(&self.key, ids).await
    }

    pub async fn dismiss_alerts(&self, ids: &[Uuid]) -> Result<()> {
        self.store.dismiss_alerts(&self.key, ids).await?;
        self.changed("");
        Ok(())
    }

    /// Renames a session; `false` when the user has no such session.
    pub async fn rename_session(&self, session_id: Uuid, title: &str) -> Result<bool> {
        let renamed = self
            .store
            .rename_session(&self.key, session_id, title)
            .await?;
        self.changed("");
        Ok(renamed)
    }

    /// Deletes a session with its runs; `false` when the user has no such session.
    pub async fn delete_session(&self, session_id: Uuid) -> Result<bool> {
        let deleted = self.store.delete_session(&self.key, session_id).await?;
        self.changed("");
        Ok(deleted)
    }

    /// Deletes a session only while it is empty and idle; `false` when it was kept.
    pub(crate) async fn delete_empty_session(&self, session_id: Uuid) -> Result<bool> {
        let deleted = self
            .store
            .delete_empty_session(&self.key, session_id)
            .await?;
        if deleted {
            self.changed("");
        }
        Ok(deleted)
    }

    /// Deletes sessions with their runs and returns how many existed.
    pub async fn delete_sessions(&self, ids: &[Uuid]) -> Result<u64> {
        let deleted = self.store.delete_sessions(&self.key, ids).await?;
        self.changed("");
        Ok(deleted)
    }

    /// Removes the turn whose user message was sent at `timestamp` (ms), or it and those after.
    pub async fn delete_turn(
        &self,
        session_id: Uuid,
        timestamp: i64,
        which: TurnsToDelete,
    ) -> Result<DeletedTurn> {
        let deleted = self
            .store
            .delete_turn(&self.key, session_id, timestamp, which)
            .await?;
        if deleted == DeletedTurn::Deleted {
            self.changed(&session_id.to_string());
        }
        Ok(deleted)
    }

    /// The ones of `ids` the user still has.
    pub async fn existing_sessions(&self, ids: &[Uuid]) -> Result<Vec<Uuid>> {
        self.store.existing_sessions(&self.key, ids).await
    }

    /// Pins or unpins a session; `false` when the user has no such session.
    pub async fn pin_session(&self, session_id: Uuid, pinned: bool) -> Result<bool> {
        let pinned = self
            .store
            .pin_session(&self.key, session_id, pinned)
            .await?;
        self.changed("");
        Ok(pinned)
    }

    /// Keeps a file Beep sends in `session_id` and returns its id.
    pub(crate) async fn save_file(
        &self,
        chat: Uuid,
        name: &str,
        mime: &str,
        data: &[u8],
    ) -> Result<String> {
        let file = NewFile {
            chat,
            name,
            mime,
            data,
        };
        self.store.save_file(&self.key, file).await
    }

    /// Keeps a copy of a picture a coding agent session showed, unless one is kept.
    pub(crate) async fn save_agent_image(
        &self,
        of: AgentImageOf<'_>,
        image: &AgentImageBytes,
    ) -> Result<()> {
        self.store.save_agent_image(&self.key, of, image).await
    }

    /// The kept copy of the user's agent session picture `key`.
    pub(crate) async fn agent_image(&self, key: &str) -> Result<Option<AgentImageBytes>> {
        self.store.agent_image(&self.key, key).await
    }

    /// Keeps the pictures the user sent with a message.
    pub(crate) async fn save_pictures(
        &self,
        of: PicturesOf,
        pictures: &[Picture],
    ) -> Result<Vec<ImageRef>> {
        let pictures: Vec<NewPicture> = pictures
            .iter()
            .map(|picture| NewPicture {
                mime: &picture.mime,
                data: &picture.data,
                width: picture.width,
                height: picture.height,
            })
            .collect();
        self.store.save_pictures(&self.key, of, &pictures).await
    }

    /// Keeps the files other than pictures the user sent with a message.
    pub(crate) async fn save_attachments(&self, of: PicturesOf, files: &[Upload]) -> Result<()> {
        let files: Vec<NewAttachment> = files
            .iter()
            .map(|file| NewAttachment {
                name: &file.name,
                mime: &file.mime,
                data: &file.data,
            })
            .collect();
        self.store.save_attachments(&self.key, of, &files).await
    }

    pub(crate) async fn mark_spoken(&self, of: PicturesOf) -> Result<()> {
        self.store.mark_spoken(&self.key, of).await
    }

    /// When each spoken prompt of a session was sent.
    pub(crate) async fn spoken_turns(&self, session_id: Uuid) -> Result<Vec<i64>> {
        self.store.spoken_turns(&self.key, session_id).await
    }

    /// The files other than pictures the user sent in a session, by turn.
    pub(crate) async fn session_files(&self, session_id: Uuid) -> Result<Vec<SentFiles>> {
        self.store.session_files(&self.key, session_id).await
    }

    /// The pictures the user sent in a session, by turn.
    pub(crate) async fn session_pictures(&self, session_id: Uuid) -> Result<Vec<SentImages>> {
        self.store.session_pictures(&self.key, session_id).await
    }

    pub(crate) async fn save_route_card(
        &self,
        chat: Uuid,
        call_id: &str,
        card: &RouteCard,
    ) -> Result<()> {
        self.store
            .save_route_card(&self.key, chat, call_id, card)
            .await
    }

    pub(crate) async fn route_card(&self, call_id: &str) -> Result<Option<RouteCard>> {
        self.store.route_card(&self.key, call_id).await
    }

    /// Up to `len` bytes of a sent file from `offset`; `None` when gone or not the user's.
    pub(crate) async fn file_part(
        &self,
        id: &str,
        offset: u64,
        len: u64,
    ) -> Result<Option<FilePart>> {
        self.store.file_part(&self.key, id, offset, len).await
    }

    /// Shares `session_id` as `conversation`; `None` when the user has no such session.
    pub async fn share_session(
        &self,
        session_id: Uuid,
        conversation: &Conversation,
    ) -> Result<Option<Share>> {
        self.store
            .share_session(&self.key, session_id, conversation)
            .await
    }

    /// Stops sharing `session_id`; `false` when it was not shared.
    pub async fn unshare_session(&self, session_id: Uuid) -> Result<bool> {
        self.store.unshare_session(&self.key, session_id).await
    }

    /// Copies the share with `token` into the user's chats as `session_id`.
    pub async fn copy_share(&self, token: &str, session_id: Uuid) -> Result<CopiedShare> {
        let copied = self.store.copy_share(&self.key, token, session_id).await?;
        if copied == CopiedShare::Copied {
            self.changed("");
        }
        Ok(copied)
    }

    /// Tells the user's phones `session_id` changed, or the chat list when it is empty.
    pub fn changed(&self, session_id: &str) {
        // Nobody listening just means no phone is online; it syncs on connect.
        let _ = self
            .notifications
            .send(UserNotice::changed(self.id(), session_id));
    }

    /// Whether the user has the chat `session`.
    pub async fn owns_session(&self, session: Uuid) -> Result<bool> {
        self.store.owns_session(&self.key, session).await
    }

    /// The messages of the user's session, oldest first; `None` for no such session.
    pub async fn session_messages(&self, session: Uuid) -> Result<Option<Vec<StoredMessage>>> {
        if !self.store.owns_session(&self.key, session).await? {
            return Ok(None);
        }
        Ok(Some(self.store.chat_messages(session).await?))
    }

    /// What each tool step of the user's session showed, oldest first; `None` for no such session.
    pub async fn tool_labels(&self, session: Uuid) -> Result<Option<Vec<ToolLabel>>> {
        if !self.store.owns_session(&self.key, session).await? {
            return Ok(None);
        }
        Ok(Some(self.store.tool_labels(session).await?))
    }

    /// Adds the web pages the reply to message `turn` used.
    pub(crate) async fn save_sources(
        &self,
        session: Uuid,
        turn: i64,
        sources: &[WebSource],
    ) -> Result<()> {
        self.store.save_sources(session, turn, sources).await
    }

    /// The web pages each turn of the user's session used; `None` for no such session.
    pub async fn session_sources(&self, session: Uuid) -> Result<Option<Vec<TurnSources>>> {
        if !self.store.owns_session(&self.key, session).await? {
            return Ok(None);
        }
        Ok(Some(self.store.session_sources(session).await?))
    }

    /// What the newest turn of a session ran with.
    pub async fn last_turn(&self, session: Uuid) -> Result<Option<TurnRecord>> {
        self.store.last_turn(session).await
    }

    /// What the model sees for a session.
    pub async fn context(&self, session: Uuid) -> Result<Option<StoredContext>> {
        self.store.context(&self.key, session).await
    }

    /// The user's updates after `after`, a page at a time.
    pub async fn updates_page(&self, after: i64) -> Result<UpdatePage> {
        self.store.updates_page(&self.key, after).await
    }

    /// The user's newest update id, now and as it moves.
    pub async fn watch_updates(&self) -> Result<watch::Receiver<i64>> {
        self.store.watch_updates(self.key.as_str()).await
    }

    /// Ends a run with `status`, saving its turn if any; `None` when none was saved.
    pub async fn finish(
        &self,
        run: &Run,
        turn: Option<FinishedTurn<'_>>,
        status: RunStatus,
    ) -> Result<Option<SavedTurn>> {
        let saved = self.store.finish(&self.key, run, turn, status).await?;
        if saved.is_some() {
            self.changed(&run.session_id);
        }
        Ok(saved)
    }
}
