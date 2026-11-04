//! `Beep`: one iroh node for the app's life, a connection to the server
//! opened on demand, and the state saved between launches.

use crate::{
    agents::work_copy::Copies,
    beep::identity::{Identity, secret_key},
    beep::state::{LinkState, NoticeVoice, Saved, StateFile},
    beep::sync::Chats,
    error::{DesktopError, Result},
};
use beep_proto::{
    Call, Duplex, GetFile, ListSessions, Notice, Platform, RemoveDevice, SearchSessions, Session,
    SessionMatch, Stream, TRANSCRIBE_RATE, Transcribe, Transcription,
    client::{Client, Node, OnNotice, Replies, Uplink},
};
use iroh::{Endpoint, EndpointId};
use serde_bytes::ByteBuf;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{sync::mpsc, time::timeout};
use uuid::Uuid;

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

/// The notices the server pushes, for the app to show.
pub type Notices = mpsc::UnboundedReceiver<Notice>;

const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// How long opening a connection may take, all of it: the dial and the
/// first stream. Without a limit an unreachable server held the client
/// lock, and every call behind it, for good.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Chats remembered as live ones, for the mic in the chat list.
const MAX_LIVE_CHATS: usize = 200;

pub struct Beep {
    node: Node,
    server: EndpointId,
    pub(super) identity: Identity,
    client: AsyncMutex<Option<Arc<Client>>>,
    saved: Mutex<Saved>,
    file: StateFile,
    /// The group the bar shows, so its messages do not also notify.
    group: Mutex<Option<String>>,
    chats: Chats,
    copies: Copies,
}

impl Beep {
    pub async fn start(
        identity: Identity,
        server: EndpointId,
        file: StateFile,
        chats: Chats,
    ) -> anyhow::Result<(Arc<Self>, Notices)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let on_notice: OnNotice = Arc::new(move |notice| tx.send(notice).is_ok());
        let node = Node::bind(secret_key(&identity), server, on_notice)
            .await?
            .with_app(Platform::Macos {}, env!("CARGO_PKG_VERSION"));
        let saved = file.load();
        Ok((
            Arc::new(Self {
                node,
                server,
                identity,
                client: AsyncMutex::new(None),
                saved: Mutex::new(saved),
                file,
                group: Mutex::new(None),
                chats,
                copies: Copies::default(),
            }),
            rx,
        ))
    }

    /// The copies of projects sync keeps on this Mac.
    pub(crate) fn copies(&self) -> &Copies {
        &self.copies
    }

    pub fn id(&self) -> EndpointId {
        self.node.id()
    }

    /// This Mac's iroh endpoint, which the node link shares.
    pub fn endpoint(&self) -> &Endpoint {
        self.node.endpoint()
    }

    pub fn server(&self) -> EndpointId {
        self.server
    }

    pub fn use_mac(&self) -> bool {
        self.saved().uses_mac()
    }

    pub fn set_use_mac(&self, on: bool) {
        self.update_saved(|s| s.set_use_mac(on));
    }

    pub fn chats(&self) -> &Chats {
        &self.chats
    }

    fn saved(&self) -> MutexGuard<'_, Saved> {
        self.saved.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the saved state and writes it to disk.
    fn update_saved(&self, f: impl FnOnce(&mut Saved)) {
        let mut saved = self.saved();
        f(&mut saved);
        self.file.save(&saved);
    }

    pub fn link_state(&self) -> LinkState {
        self.saved().link
    }

    pub fn set_link_state(&self, link: LinkState) {
        self.update_saved(|s| s.link = link);
    }

    pub fn session(&self) -> Option<String> {
        self.saved().session.clone()
    }

    pub fn set_session(&self, session: Option<String>) {
        self.update_saved(|s| s.session = session);
    }

    /// The chats started here as researches.
    pub fn researches(&self) -> Vec<Uuid> {
        self.saved().researches.clone()
    }

    /// Remembers that `session` is a research.
    pub fn add_research(&self, session: Uuid) {
        self.add_researches(&[session]);
    }

    /// Remembers chats as researches, saving once.
    pub fn add_researches(&self, sessions: &[Uuid]) {
        let missing = {
            let saved = self.saved();
            sessions.iter().any(|id| !saved.researches.contains(id))
        };
        if !missing {
            return;
        }
        self.update_saved(|s| {
            for session in sessions {
                if !s.researches.contains(session) {
                    s.researches.push(*session);
                }
            }
        });
    }

    /// What a new notice does while the user is here.
    pub fn notice_voice(&self) -> NoticeVoice {
        self.saved().notice_voice()
    }

    pub fn set_notice_voice(&self, choice: NoticeVoice) {
        self.update_saved(|s| s.set_notice_voice(choice));
    }

    /// Whether a pause sends in live mode, rather than only "send".
    pub fn send_on_pause(&self) -> bool {
        self.saved().send_on_pause
    }

    pub fn set_send_on_pause(&self, on: bool) {
        self.update_saved(|s| s.send_on_pause = on);
    }

    /// Whether coding agents notify when they ask or finish a turn.
    pub fn agent_notices(&self) -> bool {
        self.saved().agent_notices
    }

    pub fn set_agent_notices(&self, on: bool) {
        self.update_saved(|s| s.agent_notices = on);
    }

    /// Chats live mode talked in, newest last.
    pub fn live_chats(&self) -> Vec<Uuid> {
        self.saved().live_chats.clone()
    }

    /// Remembers that live mode talked in `session`.
    pub fn add_live_chat(&self, session: Uuid) {
        if self.saved().live_chats.last() == Some(&session) {
            return;
        }
        self.update_saved(|s| {
            s.live_chats.retain(|id| *id != session);
            s.live_chats.push(session);
            let over = s.live_chats.len().saturating_sub(MAX_LIVE_CHATS);
            s.live_chats.drain(..over);
        });
    }

    pub fn group(&self) -> Option<String> {
        self.group
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_group(&self, group: Option<String>) {
        *self.group.lock().unwrap_or_else(PoisonError::into_inner) = group;
    }

    /// The open connection, or a new one. Never connects while unlinked, so
    /// the server does not make this key an account of its own.
    async fn client(&self) -> Result<Arc<Client>> {
        match self.link_state() {
            LinkState::Unlinked => return Err(DesktopError::Unlinked),
            LinkState::Revoked => return Err(DesktopError::Revoked),
            LinkState::Linked => {}
        }
        let mut cached = self.client.lock().await;
        if let Some(client) = cached.as_ref() {
            if client.is_revoked() {
                self.set_link_state(LinkState::Revoked);
                return Err(DesktopError::Revoked);
            }
            if !client.is_closed() {
                return Ok(client.clone());
            }
        }
        // A closed or revoked client is not kept while a new one is made,
        // so a connect that fails leaves nothing behind.
        *cached = None;
        let client = Arc::new(connect_in_time(self.node.connect(), CONNECT_TIMEOUT).await?);
        *cached = Some(client.clone());
        Ok(client)
    }

    /// Maps a failed request, marking the key revoked when the server said so.
    pub(super) async fn failed(&self, e: anyhow::Error) -> DesktopError {
        let revoked = self
            .client
            .lock()
            .await
            .as_ref()
            .is_some_and(|c| c.is_revoked());
        if revoked {
            self.set_link_state(LinkState::Revoked);
            return DesktopError::Revoked;
        }
        e.into()
    }

    pub async fn call<R: Call>(&self, request: R) -> Result<R::Reply> {
        self.call_within(request, CALL_TIMEOUT).await
    }

    /// `call` for a request that may take longer, such as a clone.
    pub async fn call_within<R: Call>(&self, request: R, limit: Duration) -> Result<R::Reply> {
        let client = self.client().await?;
        match timeout(limit, client.call(&request)).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(e)) => Err(self.failed(e).await),
            Err(_) => Err(DesktopError::BadRequest("the server took too long".into())),
        }
    }

    pub async fn stream<R: Stream>(&self, request: R) -> Result<Replies<R::Reply>> {
        let client = self.client().await?;
        match client.stream(&request).await {
            Ok(replies) => Ok(replies),
            Err(e) => Err(self.failed(e).await),
        }
    }

    /// Sends a request that keeps sending: what follows goes up the
    /// returned `Uplink`, and the replies come as they arrive.
    pub async fn duplex<R: Duplex>(
        &self,
        request: R,
    ) -> Result<(Uplink<R::Up>, Replies<R::Reply>)> {
        let client = self.client().await?;
        match client.duplex(&request).await {
            Ok(opened) => Ok(opened),
            Err(e) => Err(self.failed(e).await),
        }
    }

    /// Opens a live transcription: audio goes up `Uplink`, words come
    /// back.
    pub async fn transcribe(&self) -> Result<(Uplink<ByteBuf>, Replies<Transcription>)> {
        self.duplex(Transcribe {
            sample_rate: TRANSCRIBE_RATE,
        })
        .await
    }

    /// Takes this computer off the account.
    pub async fn unlink(&self) -> Result<()> {
        let id = self.id().to_string();
        self.call(RemoveDevice { id }).await?;
        self.set_link_state(LinkState::Revoked);
        Ok(())
    }

    pub async fn sessions(&self) -> Result<Vec<Session>> {
        match self.chats.sessions() {
            Some(sessions) => Ok(sessions),
            None => self.call(ListSessions {}).await,
        }
    }

    /// Searches the local copy, or the server before the copy is ready.
    pub async fn search(&self, query: String) -> Result<Vec<SessionMatch>> {
        if let Some(found) = self.chats.search(&query) {
            return Ok(found);
        }
        self.call(SearchSessions { query }).await
    }

    /// Downloads a sent file to `dest`, part by part. It lands under a
    /// `.part` name first, so a cut-off download is never opened.
    pub async fn download(&self, id: &str, dest: &Path) -> Result<()> {
        let mut partial = dest.as_os_str().to_owned();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        let mut result = self.download_to(id, &partial).await;
        if result.is_ok() {
            result = fs::rename(&partial, dest).map_err(|e| anyhow::Error::from(e).into());
        }
        if result.is_err() {
            // Best effort: a leftover `.part` file is never opened.
            let _ = fs::remove_file(&partial);
        }
        result
    }

    /// Writes every part of file `id` to `path`.
    async fn download_to(&self, id: &str, path: &Path) -> Result<()> {
        let mut out = File::create(path).map_err(anyhow::Error::from)?;
        let mut offset = 0;
        loop {
            let part = self
                .call(GetFile {
                    id: id.to_owned(),
                    offset,
                })
                .await?;
            if part.data.is_empty() && offset < part.size {
                return Err(DesktopError::Other(anyhow::anyhow!(
                    "the download stopped at {offset} of {} bytes",
                    part.size
                )));
            }
            out.write_all(&part.data).map_err(anyhow::Error::from)?;
            offset += part.data.len() as u64;
            if offset >= part.size {
                return Ok(());
            }
        }
    }
}

/// What `connect` makes, or an error once `limit` passes without it.
async fn connect_in_time<T>(
    connect: impl Future<Output = anyhow::Result<T>>,
    limit: Duration,
) -> Result<T> {
    match timeout(limit, connect).await {
        Ok(connected) => Ok(connected?),
        Err(_) => Err(DesktopError::Other(anyhow::anyhow!(
            "the server did not answer in {} seconds",
            limit.as_secs()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use tokio::time::{Instant, sleep};

    // The clock is paused, so tokio moves it straight to the next timer
    // instead of the test sleeping.
    #[tokio::test(start_paused = true)]
    async fn a_connect_that_never_answers_gives_up_in_time() {
        let started = Instant::now();
        let connect = pending::<anyhow::Result<()>>();
        let error = connect_in_time(connect, CONNECT_TIMEOUT)
            .await
            .expect_err("a server that never answers is an error");
        assert_eq!(started.elapsed(), CONNECT_TIMEOUT);
        assert_eq!(error.to_string(), "the server did not answer in 15 seconds");
    }

    #[tokio::test(start_paused = true)]
    async fn a_connect_inside_the_limit_is_kept() {
        let connect = async {
            sleep(CONNECT_TIMEOUT - Duration::from_secs(1)).await;
            Ok(7)
        };
        let connected = connect_in_time(connect, CONNECT_TIMEOUT).await;
        assert_eq!(connected.expect("it answered in time"), 7);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_connect_keeps_its_error() {
        let connect = async { Err::<(), _>(anyhow::anyhow!("no route to the server")) };
        let error = connect_in_time(connect, CONNECT_TIMEOUT).await.unwrap_err();
        assert_eq!(error.to_string(), "no route to the server");
    }
}
