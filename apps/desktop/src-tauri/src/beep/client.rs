//! `Beep`: the server's client, made once and reached over HTTPS when a call
//! needs it, its events stream while linked, and the state saved between
//! launches.

use crate::{
    agents::work_copy::Copies,
    beep::identity::{Identity, secret_key},
    beep::state::{LinkState, NoticeVoice, Saved, StateFile, Zoom},
    beep::sync::Chats,
    error::{DesktopError, Result},
    projects::known,
};
use beep_proto::{
    AppEvent, Call, Duplex, GetFile, ListChats, ListSessions, Notice, Platform, RemoveDevice,
    SearchChats, SearchSessions, Session, SessionMatch, Stream, TRANSCRIBE_RATE, Transcribe,
    Transcription,
    client::{Client, Replies, Uplink, refusal},
    follow::{Follow, OnEvent},
    http::Refusal,
};
use iroh::{Endpoint, EndpointId, SecretKey};
use serde_bytes::ByteBuf;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    time::timeout,
};
use uuid::Uuid;

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

/// The notices the server sends, for the app to show.
pub type Notices = mpsc::UnboundedReceiver<Notice>;

const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// How long reaching a server may take. Without a limit an unreachable
/// server held every call behind it for good.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Chats remembered as live ones, for the mic in the chat list.
const MAX_LIVE_CHATS: usize = 200;

/// Where the server answers: `BEEP_SERVER_URL` at build time, else a
/// server on this Mac.
fn server_url() -> &'static str {
    option_env!("BEEP_SERVER_URL").unwrap_or("http://127.0.0.1:47122")
}

pub struct Beep {
    /// The one iroh endpoint of the app, which the mesh shares.
    endpoint: Endpoint,
    /// The server's client, if this app has a server.
    server: Option<Arc<Client>>,
    pub(super) identity: Identity,
    /// The events stream, followed from the first call while linked until
    /// a sign-out.
    follow: Mutex<Option<Follow>>,
    notices: mpsc::UnboundedSender<Notice>,
    /// The newest update id the events stream named, None while none is
    /// followed.
    updates: watch::Sender<Option<i64>>,
    /// The link as `link_state` gives it, for what waits for a sign-in.
    link: watch::Sender<LinkState>,
    saved: Mutex<Saved>,
    file: StateFile,
    /// The group the bar shows, so its messages do not also notify.
    group: Mutex<Option<String>>,
    chats: Chats,
    copies: Copies,
}

impl Beep {
    /// The app on `endpoint`, with the server at its address or none.
    /// Reaches nothing: a call reaches the server when it needs it.
    pub fn start(
        endpoint: Endpoint,
        identity: Identity,
        server: bool,
        file: StateFile,
        chats: Chats,
    ) -> (Arc<Self>, Notices) {
        let (tx, rx) = mpsc::unbounded_channel();
        let server = server
            .then(|| new_client(&identity))
            .and_then(|made| match made {
                Ok(client) => Some(Arc::new(client)),
                Err(e) => {
                    log::warn!("the server's address is not valid: {e:#}");
                    None
                }
            });
        let saved = file.load();
        let link = if server.is_some() {
            saved.effective_link()
        } else {
            LinkState::Unlinked
        };
        (
            Arc::new(Self {
                endpoint,
                server,
                identity,
                follow: Mutex::new(None),
                notices: tx,
                updates: watch::Sender::new(None),
                link: watch::Sender::new(link),
                saved: Mutex::new(saved),
                file,
                group: Mutex::new(None),
                chats,
                copies: Copies::default(),
            }),
            rx,
        )
    }

    /// The copies of projects sync keeps on this Mac.
    pub(crate) fn copies(&self) -> &Copies {
        &self.copies
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// This Mac's device key, which signs what it sends.
    pub(crate) fn device_key(&self) -> SecretKey {
        secret_key(&self.identity)
    }

    /// Whether this app talks to a server now. Not while signed out, so
    /// what mirrors changes to the server leaves it alone.
    pub fn has_server(&self) -> bool {
        self.server.is_some() && !self.signed_out()
    }

    /// The server's key, as the server at this app's address says.
    pub(crate) async fn server_id(&self) -> Result<EndpointId> {
        let client = self.reach()?;
        connect_in_time(client.server_id(), CONNECT_TIMEOUT).await
    }

    /// Whether this app is linked to a server: it has one and the link code
    /// was used. A server alone is not a link.
    pub fn linked(&self) -> bool {
        self.server.is_some() && self.link_state() == LinkState::Linked
    }

    /// Notes that the server still has to drop `machine`.
    pub fn owe_unlink(&self, machine: &str) {
        self.update_saved(|s| s.owe_unlink(machine));
    }

    /// The server dropped `machine`, or never had it.
    pub fn settle_unlink(&self, machine: &str) {
        self.update_saved(|s| s.settle_unlink(machine));
    }

    /// Machines the server has yet to drop.
    pub fn owed_unlinks(&self) -> Vec<String> {
        self.saved().unlinks.clone()
    }

    /// The server's client, or `NoServer`. A Mac signed out gets
    /// `Unlinked`, the answer of one with no account, so every path that
    /// works without a server works the same.
    fn reach(&self) -> Result<Arc<Client>> {
        let client = self.server.as_ref().ok_or(DesktopError::NoServer)?;

        if self.signed_out() {
            return Err(DesktopError::Unlinked);
        }

        Ok(client.clone())
    }

    #[cfg(desktop)]
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

        if self.server.is_some() {
            self.link.send_if_modified(|link| {
                let was = *link;
                *link = saved.effective_link();
                was != *link
            });
        }
    }

    /// Waits until this app is linked to the server. Never returns with no
    /// server to link to.
    pub(crate) async fn until_linked(&self) {
        let mut link = self.link.subscribe();

        if link
            .wait_for(|link| *link == LinkState::Linked)
            .await
            .is_err()
        {
            std::future::pending::<()>().await;
        }
    }

    /// Where the link stands. A Mac signed out reads as unlinked, and so does one with no server
    /// to talk to, whatever the saved link says: the saved link stays for the day a server is
    /// back.
    pub fn link_state(&self) -> LinkState {
        if self.server.is_none() {
            return LinkState::Unlinked;
        }

        self.saved().effective_link()
    }

    /// Sets the link, which ends a sign-out.
    pub fn set_link_state(&self, link: LinkState) {
        self.update_saved(|s| {
            s.link = link;
            s.signed_out = false;
        });
    }

    /// Whether the user signed this Mac out of the server in Settings.
    pub fn signed_out(&self) -> bool {
        self.saved().signed_out
    }

    /// Notes a sign-out or a sign-in, saved so a launch keeps it.
    pub(super) fn set_signed_out(&self, signed_out: bool) {
        self.update_saved(|s| s.signed_out = signed_out);
    }

    /// Signs out: no call is made from now on, and the events stream ends.
    /// The key and the link are kept, so signing in returns to the same
    /// account.
    pub(super) async fn sign_out(&self) {
        self.set_signed_out(true);
        self.stop_events();
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

    /// The page zoom of every window.
    pub fn zoom(&self) -> Zoom {
        self.saved().zoom
    }

    pub fn set_zoom(&self, zoom: Zoom) {
        self.update_saved(|s| s.zoom = zoom);
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

    /// The server's client, following its events. Never calls while
    /// unlinked, so the server does not make this key an account of its own.
    async fn client(&self) -> Result<Arc<Client>> {
        let client = self.reach()?;

        match self.link_state() {
            LinkState::Unlinked => return Err(DesktopError::Unlinked),
            LinkState::Revoked => return Err(DesktopError::Revoked),
            LinkState::Linked => {}
        }

        self.follow_events(&client);
        Ok(client)
    }

    /// Opens the events stream if none is followed.
    fn follow_events(&self, client: &Arc<Client>) {
        let mut follow = self.follow.lock().unwrap_or_else(PoisonError::into_inner);

        if follow.is_some() {
            return;
        }

        let notices = self.notices.clone();
        let updates = self.updates.clone();
        let on_event: OnEvent = Arc::new(move |event| match event {
            AppEvent::Notice { notice, .. } => notices.send(*notice).is_ok(),
            AppEvent::Updated { last } => {
                updates.send_replace(Some(last));
                false
            }
            AppEvent::Heartbeat {} | AppEvent::Resume { .. } => false,
        });
        *follow = Some(Follow::start(client.clone(), on_event));
    }

    /// Ends the events stream; the next call opens it again.
    fn stop_events(&self) {
        let stopped = self
            .follow
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(stopped);
        self.updates.send_replace(None);
    }

    /// Whether the events stream is followed.
    #[cfg(test)]
    fn following(&self) -> bool {
        self.follow
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// The newest update id, each time the events stream names one. It
    /// reads None once the stream is no longer followed.
    pub(super) async fn watch_updates(&self) -> Result<watch::Receiver<Option<i64>>> {
        self.client().await?;
        let mut updates = self.updates.subscribe();
        updates.mark_changed();
        Ok(updates)
    }

    /// Whether the server has hosted sign-in. Makes no account on one without.
    pub(super) async fn has_login(&self) -> bool {
        match self.reach() {
            Ok(client) => client.has_login().await,
            Err(_) => false,
        }
    }

    /// `call` for a key that may have no account yet, which only a server
    /// that took it as a guest answers.
    pub(super) async fn guest_call<R: Call>(&self, request: R) -> Result<R::Reply> {
        let client = self.reach()?;

        match timeout(CALL_TIMEOUT, client.call(&request)).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(e)) => Err(self.failed(e)),
            Err(_) => Err(DesktopError::BadRequest("the server took too long".into())),
        }
    }

    /// Opens the events stream again on the next call, as the account the
    /// key is on now.
    pub(super) fn restart_events(&self) {
        self.stop_events();
    }

    /// A client for another server, to try it before choosing it. Only the
    /// server at this app's address can be chosen: a link code names a
    /// server, not where it answers.
    pub(super) async fn connect_to(&self, server: EndpointId) -> Result<Client> {
        let client = new_client(&self.identity)?;
        let id = connect_in_time(client.server_id(), CONNECT_TIMEOUT).await?;

        if id != server {
            return Err(DesktopError::BadRequest(
                "this Mac does not know where that server answers".into(),
            ));
        }

        Ok(client)
    }

    /// Maps a failed request, marking the key revoked when the server said so.
    pub(super) fn failed(&self, e: anyhow::Error) -> DesktopError {
        if refusal(&e) == Some(Refusal::Revoked) {
            self.set_link_state(LinkState::Revoked);
            self.stop_events();
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
            Ok(Err(e)) => Err(self.failed(e)),
            Err(_) => Err(DesktopError::BadRequest("the server took too long".into())),
        }
    }

    pub async fn stream<R: Stream>(&self, request: R) -> Result<Replies<R::Reply>> {
        let client = self.client().await?;

        match client.stream(&request).await {
            Ok(replies) => Ok(replies),
            Err(e) => Err(self.failed(e)),
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
            Err(e) => Err(self.failed(e)),
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
        self.stop_events();
        Ok(())
    }

    pub async fn sessions(&self) -> Result<Vec<Session>> {
        if !self.has_server() {
            return Ok(self.chats.sessions().unwrap_or_default());
        }

        match self.chats.sessions() {
            Some(sessions) => Ok(sessions),
            // Without the alerts; a server from before them lists all.
            None => match known(self.call(ListChats {}).await)? {
                Some(chats) => Ok(chats),
                None => self.call(ListSessions {}).await,
            },
        }
    }

    /// Searches the local copy, off the async workers since it walks every
    /// message, or the server before the copy is ready.
    pub async fn search(self: &Arc<Self>, query: String) -> Result<Vec<SessionMatch>> {
        let (me, words) = (Arc::clone(self), query.clone());
        let copy = tauri::async_runtime::spawn_blocking(move || me.chats.search(&words))
            .await
            .map_err(anyhow::Error::from)?;
        match copy {
            Some(found) => Ok(found),
            None if !self.has_server() => Ok(Vec::new()),
            None => match known(
                self.call(SearchChats {
                    query: query.clone(),
                })
                .await,
            )? {
                Some(found) => Ok(found),
                None => self.call(SearchSessions { query }).await,
            },
        }
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

/// The server's client, signing as `identity`.
fn new_client(identity: &Identity) -> Result<Client> {
    Ok(Client::new(server_url(), secret_key(identity))?
        .with_app(Platform::Macos {}, env!("CARGO_PKG_VERSION")))
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
#[path = "../tests/beep_client.rs"]
mod tests;
