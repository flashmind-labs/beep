//! `Beep`: one iroh node for the app's life, a connection to the server
//! opened on demand, and the state saved between launches.

use crate::{
    agents::work_copy::Copies,
    beep::identity::Identity,
    beep::state::{LinkState, NoticeVoice, Saved, StateFile},
    beep::sync::Chats,
    error::{DesktopError, Result},
};
use beep_proto::{
    ApiError, Call, Duplex, GetFile, ListSessions, Notice, Platform, RemoveDevice, SearchSessions,
    Session, SessionMatch, Stream, TRANSCRIBE_RATE, Transcribe, Transcription,
    client::{Client, Node, OnNotice, PushHandler, Replies, Uplink},
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
    /// The one iroh endpoint of the app, which the mesh, the node link and
    /// a server's notices share.
    endpoint: Endpoint,
    /// The link to the server, if this app has one.
    server: Option<Server>,
    pub(super) identity: Identity,
    client: AsyncMutex<Option<Arc<Client>>>,
    saved: Mutex<Saved>,
    file: StateFile,
    /// The group the bar shows, so its messages do not also notify.
    group: Mutex<Option<String>>,
    chats: Chats,
    copies: Copies,
}

/// A server this app talks to.
struct Server {
    node: Node,
    id: EndpointId,
    /// What takes its notices on the endpoint.
    push: PushHandler,
}

impl Beep {
    /// The app on `endpoint`, with a server or none. Binds nothing and
    /// reaches nothing: a connection opens when a call needs one.
    pub fn start(
        endpoint: Endpoint,
        identity: Identity,
        server: Option<EndpointId>,
        file: StateFile,
        chats: Chats,
    ) -> (Arc<Self>, Notices) {
        let (tx, rx) = mpsc::unbounded_channel();
        let on_notice: OnNotice = Arc::new(move |notice| tx.send(notice).is_ok());
        let server = server.map(|id| {
            let (node, push) = Node::on_endpoint(endpoint.clone(), id, on_notice);
            Server {
                node: node.with_app(Platform::Macos {}, env!("CARGO_PKG_VERSION")),
                id,
                push,
            }
        });
        let saved = file.load();
        (
            Arc::new(Self {
                endpoint,
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
        )
    }

    /// The copies of projects sync keeps on this Mac.
    pub(crate) fn copies(&self) -> &Copies {
        &self.copies
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// This Mac's iroh endpoint, which the node link shares.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The server this app talks to now, if it has one. None while signed
    /// out, so what mirrors changes to the server leaves it alone.
    pub fn server(&self) -> Option<EndpointId> {
        self.server
            .as_ref()
            .filter(|_| !self.signed_out())
            .map(|server| server.id)
    }

    /// Whether this app is linked to a server: one is set and the link code
    /// was used. The built-in server id alone is not a link.
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

    /// What takes the server's notices on the endpoint, for the router.
    pub fn push(&self) -> Option<PushHandler> {
        self.server.as_ref().map(|server| server.push.clone())
    }

    /// The server's side of the app, or `NoServer`. A Mac signed out gets
    /// `Unlinked`, the answer of one with no account, so every path that
    /// works without a server works the same.
    fn node(&self) -> Result<&Node> {
        let server = self.server.as_ref().ok_or(DesktopError::NoServer)?;
        if self.signed_out() {
            return Err(DesktopError::Unlinked);
        }
        Ok(&server.node)
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

    /// Where the link stands. A Mac signed out reads as unlinked.
    pub fn link_state(&self) -> LinkState {
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

    /// Signs out: no call is made from now on, and the connection open
    /// closes, ending the streams on it. The key and the link are kept, so
    /// signing in returns to the same account.
    pub(super) async fn sign_out(&self) {
        self.set_signed_out(true);
        if let Some(client) = self.client.lock().await.take() {
            client.close();
        }
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
        self.node()?;
        match self.link_state() {
            LinkState::Unlinked => return Err(DesktopError::Unlinked),
            LinkState::Revoked => return Err(DesktopError::Revoked),
            LinkState::Linked => {}
        }
        self.connection().await
    }

    /// The open connection, or a new one, whatever the link state.
    async fn connection(&self) -> Result<Arc<Client>> {
        let node = self.node()?;
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
        let client = Arc::new(connect_in_time(node.connect(), CONNECT_TIMEOUT).await?);
        *cached = Some(client.clone());
        Ok(client)
    }

    /// Whether the server has hosted sign-in. Makes no account on one without.
    pub(super) async fn has_login(&self) -> bool {
        match self.node() {
            Ok(node) => node.has_login().await,
            Err(_) => false,
        }
    }

    /// `call` for a key that may have no account yet, which only a server
    /// that took it as a guest answers. A connection that went away is
    /// replaced once, with the same key.
    pub(super) async fn guest_call<R: Call>(&self, request: R) -> Result<R::Reply> {
        let mut client = self.connection().await?;
        let mut reply = timeout(CALL_TIMEOUT, client.call(&request)).await;
        if matches!(&reply, Ok(Err(e)) if e.downcast_ref::<ApiError>().is_none()) {
            self.forget_connection().await;
            client = self.connection().await?;
            reply = timeout(CALL_TIMEOUT, client.call(&request)).await;
        }
        match reply {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(e)) => Err(self.failed(e).await),
            Err(_) => Err(DesktopError::BadRequest("the server took too long".into())),
        }
    }

    /// Drops the connection, so the next call opens a new one.
    pub(super) async fn forget_connection(&self) {
        *self.client.lock().await = None;
    }

    /// Connects to another server, to try it before choosing it.
    pub(super) async fn connect_to(&self, server: EndpointId) -> Result<Client> {
        connect_in_time(self.node()?.connect_to(server), CONNECT_TIMEOUT).await
    }

    /// Makes `server` the one this app connects to from the next launch.
    pub(super) fn choose_server(&self, server: EndpointId, link: LinkState) {
        self.update_saved(|s| {
            s.server = Some(server);
            s.link = link;
            s.signed_out = false;
        });
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

    /// Searches the local copy, off the async workers since it walks every
    /// message, or the server before the copy is ready.
    pub async fn search(self: &Arc<Self>, query: String) -> Result<Vec<SessionMatch>> {
        let (me, words) = (Arc::clone(self), query.clone());
        let copy = tauri::async_runtime::spawn_blocking(move || me.chats.search(&words))
            .await
            .map_err(anyhow::Error::from)?;
        match copy {
            Some(found) => Ok(found),
            None => self.call(SearchSessions { query }).await,
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
    use crate::beep::identity::load_or_create;
    use beep_mesh::transport_config;
    use iroh::{SecretKey, endpoint::presets};
    use std::future::pending;
    use tempfile::TempDir;
    use tokio::time::{Instant, sleep};

    /// The app on a loopback endpoint with no relay, so nothing reaches a
    /// network, and no server.
    async fn without_server(dir: &TempDir) -> Arc<Beep> {
        app(dir, None).await
    }

    async fn app(dir: &TempDir, server: Option<EndpointId>) -> Arc<Beep> {
        let identity = load_or_create(&dir.path().join("identity.key")).expect("a key is made");
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .transport_config(transport_config())
            .bind()
            .await
            .expect("the endpoint binds");
        let file = StateFile::new(dir.path().join("state.json"));
        let (beep, _notices) = Beep::start(
            endpoint,
            identity,
            server,
            file,
            Chats::load(dir.path().join("sync")),
        );
        beep
    }

    #[tokio::test]
    async fn a_request_only_a_server_answers_says_there_is_none() {
        let dir = TempDir::new().expect("a temp dir");
        let beep = without_server(&dir).await;
        assert_eq!(beep.server(), None);
        assert!(beep.push().is_none());
        // Linked or not, there is nothing to connect to.
        for link in [LinkState::Unlinked, LinkState::Linked, LinkState::Revoked] {
            beep.set_link_state(link);
            let error = beep.call(ListSessions {}).await.expect_err("no server");
            assert!(matches!(error, DesktopError::NoServer), "{error:?}");
        }
        assert!(matches!(
            beep.link_offer("Studio"),
            Err(DesktopError::NoServer)
        ));
        assert!(!beep.has_login().await);
    }

    #[tokio::test]
    async fn a_signed_out_mac_connects_to_nothing_and_signs_back_in_with_its_key() {
        let dir = TempDir::new().expect("a temp dir");
        let server = SecretKey::generate().public();
        // A file as an earlier run left it: linked, then signed out.
        fs::write(
            dir.path().join("state.json"),
            r#"{"link":"linked","signed_out":true}"#,
        )
        .expect("the state is written");
        let beep = app(&dir, Some(server)).await;
        assert!(beep.signed_out());
        assert_eq!(beep.link_state(), LinkState::Unlinked);
        assert!(!beep.linked());
        assert_eq!(beep.server(), None);
        let error = beep.call(ListSessions {}).await.expect_err("signed out");
        assert!(matches!(error, DesktopError::Unlinked), "{error:?}");
        let error = beep
            .guest_call(ListSessions {})
            .await
            .expect_err("signed out");
        assert!(matches!(error, DesktopError::Unlinked), "{error:?}");
        assert!(!beep.has_login().await);
        assert!(
            beep.client.lock().await.is_none(),
            "no connection was opened"
        );
        // Signing in again is the one change: the same link, the same key.
        beep.set_signed_out(false);
        assert!(!beep.signed_out());
        assert!(beep.linked());
        assert_eq!(beep.server(), Some(server));
        // The link set while signed out ends the sign-out.
        beep.sign_out().await;
        assert!(beep.signed_out());
        beep.set_link_state(LinkState::Linked);
        assert!(!beep.signed_out());
    }

    #[tokio::test]
    async fn a_server_id_is_a_link_only_once_the_app_is_linked() {
        let dir = TempDir::new().expect("a temp dir");
        let none = without_server(&dir).await;
        none.set_link_state(LinkState::Linked);
        assert!(!none.linked(), "a link with no server is no link");
        let dir = TempDir::new().expect("a temp dir");
        let built_in = SecretKey::generate().public();
        let beep = app(&dir, Some(built_in)).await;
        assert!(!beep.linked(), "a built-in id alone is no link");
        beep.set_link_state(LinkState::Linked);
        assert!(beep.linked());
        beep.set_link_state(LinkState::Revoked);
        assert!(!beep.linked());
    }

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
