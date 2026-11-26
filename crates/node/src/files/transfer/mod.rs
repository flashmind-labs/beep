//! `transfer/1` (see `beep_proto::node::transfer`): sends files and folders
//! to other nodes over iroh-blobs. `Transfers` owns the blob store and serves
//! it only to the peers an open offer names.

use crate::lock;
use crate::{
    files::Folders,
    files::sync::{self, PeerHandler, Syncs},
    files::syncthing::Syncthing,
    membership::Membership,
    protocol::{Protocol, Session},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use beep_proto::node::{
    LinkRx, LinkTx,
    transfer::{Content, ContentKind, PROTOCOL, Reply, Request, Stage},
};
use beep_proto::watched;
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router, RouterBuilder},
};
use iroh_blobs::{
    BlobsProtocol, Hash,
    store::{
        GcConfig, ProtectCb, ProtectOutcome,
        fs::{FsStore, options::Options},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore, watch},
    task::spawn_blocking,
    time::{Instant, timeout},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

mod fetch;
pub(crate) mod offer;

use crate::files::{
    grants::{self, Grants},
    place::{self, Folder, Journal, Room},
};

/// The node's transfers, set once its services are up and read by whoever
/// holds a copy of the offers.
#[derive(Clone)]
pub(crate) struct Sharing(Arc<watch::Sender<Option<Transfers>>>);

impl Default for Sharing {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(None)))
    }
}

impl Sharing {
    pub(crate) fn get(&self) -> Option<Transfers> {
        watched::read_sent(&self.0, Clone::clone)
    }

    pub(crate) fn set(&self, transfers: Transfers) {
        self.0.send_replace(Some(transfers));
    }

    /// Changes when the transfers are set.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<Transfers>> {
        self.0.subscribe()
    }
}

/// The most transfers, offers and fetches together, running at once.
const MAX_RUNNING: usize = 4;
/// How long the collector, which drops the blobs no transfer holds any
/// more, waits for a transfer to end before it runs anyway.
const GC_IDLE: Duration = Duration::from_secs(5 * 60);
/// How often progress goes out while bytes move.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);
/// How long a device may fetch a file it asked to have shared.
const SHARE_FOR: Duration = Duration::from_secs(10 * 60);

/// Where a transfer's progress goes: the stream of the one who asked, or
/// nowhere when the node moves files for its own agent.
pub(super) enum Out<'a> {
    Stream(&'a mut LinkTx),
    Silent,
}

impl Out<'_> {
    async fn send(&mut self, reply: &Reply) -> Result<()> {
        match self {
            Out::Stream(tx) => tx.send(reply).await,
            Out::Silent => Ok(()),
        }
    }
}

/// Sends progress for one stage at most every `PROGRESS_EVERY`, so the
/// server's idle timeouts only fire on a transfer that stopped moving.
#[derive(Debug)]
struct Ticker {
    stage: Stage,
    total: u64,
    done: u64,
    last: Option<Instant>,
}

impl Ticker {
    fn new(stage: Stage, total: u64) -> Self {
        Self {
            stage,
            total,
            done: 0,
            last: None,
        }
    }

    fn done(&self) -> u64 {
        self.done
    }

    /// Whether a report is due at `now`; the first one always is.
    fn due(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|last| now.saturating_duration_since(last) >= PROGRESS_EVERY)
    }

    /// Records `done` and reports it if one is due.
    async fn tick(&mut self, tx: &mut Out<'_>, done: u64) -> Result<()> {
        self.done = done;

        if self.due(Instant::now()) {
            self.send(tx).await?;
        }

        Ok(())
    }

    /// Reports `done` now, as the stage's last report.
    async fn finish(&mut self, tx: &mut Out<'_>, done: u64) -> Result<()> {
        self.done = done;
        self.send(tx).await
    }

    async fn send(&mut self, tx: &mut Out<'_>) -> Result<()> {
        self.last = Some(Instant::now());
        tx.send(&Reply::Progress {
            done: self.done.min(self.total),
            total: self.total,
            stage: self.stage,
        })
        .await
    }
}

/// The node's transfers: its store, the grants of its open offers and, when
/// it has one, the router serving the blobs protocol, all dropped together.
#[derive(Clone)]
pub struct Transfers {
    inner: Arc<Inner>,
}

struct Inner {
    /// The endpoint, the store, its directory, the journal and the room,
    /// as sync holds them.
    parts: sync::Parts,
    /// Wakes the collector once a transfer ended and its tags are gone.
    collect: Arc<Notify>,
    folders: Folders,
    grants: Arc<Grants>,
    running: Arc<Semaphore>,
    /// The files devices asked to have shared, by device and blob, each
    /// held in the store and granted until its lease ends.
    shares: Mutex<HashMap<(EndpointId, Hash), Lease>>,
    syncs: Syncs,
    // Dropping the router stops serving blobs; the handle ends the task
    // that answers its events. `None` when the caller serves `Served`.
    router: Option<Router>,
    _events: AbortOnDropHandle<()>,
}

/// A file or folder made ready for a peer, held open while it lives.
pub(crate) struct Offer {
    /// Always `Reply::Offered`.
    pub(crate) reply: Reply,
    _offered: offer::Offered,
    grant: grants::GrantGuard,
}

impl Offer {
    /// The most bytes one fetch of it has sent so far.
    pub(crate) fn sent(&self) -> watch::Receiver<u64> {
        self.grant.sent()
    }
}

/// One shared file, fetchable by one device until `until`.
struct Lease {
    until: Instant,
    _offered: offer::Offered,
    _grant: grants::GrantGuard,
}

/// A file `share` made fetchable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shared {
    pub(crate) hash: [u8; 32],
    pub(crate) size: u64,
}

/// What a source offered and where to write it, as one node's agent hands
/// it to another node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Incoming {
    pub(crate) from: EndpointAddr,
    pub(crate) content: Content,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) executable: bool,
    /// A folder inside the shared folders; the first one when unset.
    #[serde(default)]
    pub(crate) dest: Option<String>,
}

/// Where `receive` put it.
pub(crate) struct Received {
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
}

/// What `Transfers` answers on the endpoint: the blobs and sync protocols,
/// for a router the caller runs.
#[derive(Clone)]
pub struct Served {
    blobs: BlobsProtocol,
    sync: PeerHandler,
}

#[derive(Clone)]
enum Slot {
    Waiting,
    Ready(Served),
    Failed,
}

/// The blobs and sync protocols for a router built before `Transfers` is
/// up: a connection on either waits for the services, and is closed if they
/// never come.
pub struct Late {
    slot: watch::Sender<Slot>,
    /// Connections waiting for the services; more are closed.
    waiting: Arc<Semaphore>,
}

/// How many connections may wait for the services at once.
const MAX_WAITING: usize = 16;
/// How long a connection waits for the services before it is closed.
const WAIT_LIMIT: Duration = Duration::from_secs(30);

impl Default for Late {
    fn default() -> Self {
        Self {
            slot: watch::channel(Slot::Waiting).0,
            waiting: Arc::new(Semaphore::new(MAX_WAITING)),
        }
    }
}

impl Late {
    pub fn add_to(&self, router: RouterBuilder) -> RouterBuilder {
        let waiting = |blobs| Waiting {
            slot: self.slot.subscribe(),
            waiting: self.waiting.clone(),
            blobs,
        };
        router
            .accept(iroh_blobs::ALPN, waiting(true))
            .accept(sync::ALPN, waiting(false))
    }

    /// The services are up.
    pub fn serve(&self, served: Served) {
        self.slot.send_replace(Slot::Ready(served));
    }

    /// The services will not start, unless they already have.
    pub fn fail(&self) {
        self.slot.send_if_modified(|slot| {
            let waiting = matches!(slot, Slot::Waiting);

            if waiting {
                *slot = Slot::Failed;
            }

            waiting
        });
    }
}

/// One of `Late`'s two protocols.
struct Waiting {
    slot: watch::Receiver<Slot>,
    waiting: Arc<Semaphore>,
    blobs: bool,
}

impl std::fmt::Debug for Waiting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Waiting")
    }
}

impl ProtocolHandler for Waiting {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let mut slot = self.slot.clone();
        // The permit is held while waiting, and no longer.
        let ready = match self.waiting.clone().try_acquire_owned() {
            Ok(_permit) => {
                let waited = timeout(
                    WAIT_LIMIT,
                    watched::wait_for(&mut slot, |slot| !matches!(slot, Slot::Waiting)),
                )
                .await;

                match waited {
                    Ok(Ok(())) => Some(watched::read(&slot, Clone::clone)),
                    _ => None,
                }
            }
            Err(_) => None,
        };

        match ready {
            Some(Slot::Ready(Served { blobs, .. })) if self.blobs => blobs.accept(connection).await,
            Some(Slot::Ready(Served { sync, .. })) => sync.accept(connection).await,
            _ => {
                connection.close(1u32.into(), b"not serving");
                Ok(())
            }
        }
    }
}

impl Served {
    pub fn add_to(self, router: RouterBuilder) -> RouterBuilder {
        router
            .accept(iroh_blobs::ALPN, self.blobs)
            .accept(sync::ALPN, self.sync)
    }
}

impl Transfers {
    /// Opens the store in `dir`, removes partial transfers a stopped node
    /// left, starts the synced projects kept in `dir/sync`, and serves the
    /// blobs and sync protocols on `endpoint`, which must not serve other
    /// protocols through a router of its own. Folders `syncthing` keeps in
    /// step are not synced.
    pub async fn start_with(
        endpoint: &Endpoint,
        dir: &Path,
        folders: Folders,
        syncthing: Syncthing,
    ) -> Result<Self> {
        let membership = Membership::default();

        Ok(Self::build(endpoint, dir, folders, syncthing, membership, true)
            .await?
            .0)
    }

    /// `start`, but the blobs and sync protocols are left to the caller's
    /// router: add `Served` to it. Stop that router before `shutdown`, or
    /// with it through `stop_for_exit`.
    pub async fn start_shared(
        endpoint: &Endpoint,
        dir: &Path,
        folders: Folders,
        membership: Membership,
    ) -> Result<(Self, Served)> {
        Self::build(
            endpoint,
            dir,
            folders,
            Syncthing::local(),
            membership,
            false,
        )
        .await
    }

    async fn build(
        endpoint: &Endpoint,
        dir: &Path,
        folders: Folders,
        syncthing: Syncthing,
        membership: Membership,
        own_router: bool,
    ) -> Result<(Self, Served)> {
        let collect = Arc::new(Notify::new());
        let mut options = Options::new(dir);
        // Small files are referenced where they lie, as large ones are,
        // instead of copied into the database, which redb caches in memory.
        options.inline.max_data_inlined = 0;
        options.gc = Some(GcConfig {
            // The collector checks this often, but waits in `add_protected`
            // for a transfer to end, so it runs right after each one.
            interval: Duration::from_secs(1),
            add_protected: Some(wait_for(collect.clone())),
        });
        let store = FsStore::load_with_opts(dir.join("blobs.db"), options)
            .await
            .with_context(|| format!("opening the blob store in {}", dir.display()))?;
        let journal = Journal::open(dir.join("partial"))?;
        let sweeping = journal.clone();
        spawn_blocking(move || sweeping.sweep()).await??;
        let room: Arc<Room> = Arc::default();
        let parts = sync::Parts {
            endpoint: endpoint.clone(),
            store,
            store_dir: dir.to_owned(),
            journal,
            room,
        };
        let syncs = Syncs::start(
            parts.clone(),
            folders.clone(),
            dir.join("sync"),
            syncthing,
            membership,
        )
        .await?;
        let grants = Arc::new(Grants::new(syncs.access()));
        let (events, events_task) = grants::events(grants.clone());
        let served = Served {
            blobs: BlobsProtocol::new(&parts.store, Some(events)),
            sync: PeerHandler {
                syncs: syncs.clone(),
            },
        };
        let router = own_router.then(|| {
            served
                .clone()
                .add_to(Router::builder(endpoint.clone()))
                .spawn()
        });
        let transfers = Self {
            inner: Arc::new(Inner {
                parts,
                collect,
                folders,
                grants,
                running: Arc::new(Semaphore::new(MAX_RUNNING)),
                shares: Mutex::default(),
                syncs,
                router,
                _events: events_task,
            }),
        };
        Ok((transfers, served))
    }

    /// The synced projects, served to the server as `sync/1`.
    pub fn syncs(&self) -> Syncs {
        self.inner.syncs.clone()
    }

    /// Makes `path` ready for `peer` to fetch, for this node's own agent
    /// sending files to another machine: the offer stays open, and `peer`
    /// may fetch it, until the returned `Offer` drops.
    pub(crate) async fn offer_to(&self, path: &str, peer: EndpointId) -> Result<Offer> {
        let inner = &self.inner;
        let resolved = {
            let (folders, path) = (inner.folders.clone(), path.to_owned());
            spawn_blocking(move || folders.everywhere().resolve(&path)).await??
        };
        let offered = offer::offer(
            &inner.parts.store,
            &resolved,
            inner.parts.endpoint.addr(),
            &mut Out::Silent,
        )
        .await?;
        let grant = inner.grants.grant(peer, offered.content);
        Ok(Offer {
            reply: offered.reply.clone(),
            _offered: offered,
            grant,
        })
    }

    /// Lets `peer` fetch the file at `path`, already checked and resolved,
    /// for `SHARE_FOR`, as a device asks to show a media file. Asking again
    /// renews the lease.
    pub(crate) async fn share(&self, path: &Path, peer: EndpointId) -> Result<Shared> {
        let inner = &self.inner;
        let _permit = inner.permit()?;
        let offered =
            offer::offer(&inner.parts.store, path, inner.parts.endpoint.addr(), &mut Out::Silent).await?;
        let Reply::Offered { content, size, .. } = offered.reply else {
            bail!("{} could not be shared", path.display());
        };
        ensure!(
            content.kind == ContentKind::File,
            "{} is not a file",
            path.display()
        );

        let hash = offered.content.hash;
        let grant = inner.grants.grant(peer, offered.content);
        let until = Instant::now() + SHARE_FOR;
        let lease = Lease {
            until,
            _offered: offered,
            _grant: grant,
        };
        // The lease it replaces drops outside the lock.
        let replaced = lock(&inner.shares).insert((peer, hash), lease);
        drop(replaced);

        let held = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            tokio::time::sleep_until(until).await;
            let Some(inner) = held.upgrade() else {
                return;
            };
            let ended = {
                let mut shares = lock(&inner.shares);
                let over = shares
                    .get(&(peer, hash))
                    .is_some_and(|lease| lease.until <= Instant::now());
                over.then(|| shares.remove(&(peer, hash)))
            };

            if ended.is_some() {
                drop(ended);
                inner.collect.notify_one();
            }
        });

        Ok(Shared {
            hash: content.hash,
            size,
        })
    }

    /// Fetches what a source offered with `offer_to` into `dest`, made if
    /// missing (the first shared folder, else the home folder, when none),
    /// writing under the first free name.
    pub(crate) async fn receive(&self, incoming: Incoming) -> Result<Received> {
        let inner = &self.inner;
        let _permit = inner.permit()?;
        place::plain_name(&incoming.name)?;
        let (folders, dest) = (inner.folders.clone(), incoming.dest.clone());
        let folder = spawn_blocking(move || {
            let everywhere = folders.everywhere();
            let start = everywhere.first().context("this machine has no folders")?;
            let dest = start.join(dest.unwrap_or_default());
            fs::create_dir_all(&dest).with_context(|| format!("making {}", dest.display()))?;
            place::open_target(&everywhere, &dest.to_string_lossy())
        })
        .await??;
        let folder = Arc::new(folder);
        let job = fetch::Job::new(
            incoming.from,
            &incoming.content,
            incoming.name,
            folder,
            incoming.executable,
        );
        let cancel = CancellationToken::new();
        let fetched = fetch::fetch(&inner.parts, &cancel, job, &mut Out::Silent).await;
        inner.collect.notify_one();
        let fetched = fetched?;
        Ok(Received {
            path: fetched.path,
            size: fetched.size,
        })
    }

    /// Stops the synced projects, saving their state, stops serving, and
    /// closes the store, so it can be opened again.
    pub async fn shutdown(&self) -> Result<()> {
        self.inner.syncs.shutdown().await;

        if let Some(router) = &self.inner.router {
            router
                .shutdown()
                .await
                .context("stopping the blobs protocol")?;
        }
        // The store stops without answering, so its reply channel closes.
        if let Err(e) = self.inner.parts.store.shutdown().await {
            log::debug!("the blob store stopped: {e}");
        }

        Ok(())
    }
}

/// Stops `router` and the `transfers` it serves for a process about to
/// exit, such as the Mac app: the synced projects stop and save their state
/// while the router says goodbye to the peers, so a peer that does not
/// answer holds up no save. The caller bounds the wait; the process exits
/// after it, so a goodbye still going is simply cut.
pub async fn stop_for_exit(router: &Router, transfers: Option<&Transfers>) {
    let goodbye = async {
        if let Err(e) = router.shutdown().await {
            log::warn!("the router did not stop cleanly: {e:#}");
        }
    };
    let saving = async {
        if let Some(transfers) = transfers
            && let Err(e) = transfers.shutdown().await
        {
            log::warn!("transfers did not stop cleanly: {e:#}");
        }
    };
    tokio::join!(goodbye, saving);
}

/// A collector hook that protects nothing extra and returns once `wake`
/// is notified, or after `GC_IDLE`.
fn wait_for(wake: Arc<Notify>) -> ProtectCb {
    Arc::new(move |_live| {
        let wake = wake.clone();
        Box::pin(async move {
            // Both ways it is time to collect.
            let _ = timeout(GC_IDLE, wake.notified()).await;
            ProtectOutcome::Continue
        })
    })
}

impl Protocol for Transfers {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(self.inner.clone(), tx, rx))
    }
}

impl Inner {
    /// A place among the `MAX_RUNNING` transfers, or a busy error.
    fn permit(&self) -> Result<OwnedSemaphorePermit> {
        self.running
            .clone()
            .try_acquire_owned()
            .map_err(|_| anyhow!("busy: {MAX_RUNNING} transfers are running on this machine"))
    }
}

/// Serves one `transfer/1` stream: one offer or one fetch.
async fn serve(inner: Arc<Inner>, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };
    let permit = match inner.permit() {
        Ok(permit) => permit,
        Err(e) => return tx.send(&failed(&e)).await,
    };
    let served = serve_request(&inner, permit, request, tx, rx).await;
    // Its tags are gone: let the collector drop its blobs now.
    inner.collect.notify_one();
    served
}

async fn serve_request(
    inner: &Inner,
    permit: OwnedSemaphorePermit,
    request: Request,
    mut tx: LinkTx,
    rx: LinkRx,
) -> Result<()> {
    match request {
        Request::Offer { path, peer } => serve_offer(inner, permit, &path, peer, tx, rx).await,
        Request::Fetch {
            from,
            content,
            name,
            folder,
            executable,
            ..
        } => {
            let checked = check_fetch(inner, &name, &folder).await;
            let folder = match checked {
                Ok(folder) => folder,
                Err(e) => return tx.send(&failed(&e)).await,
            };
            let job = fetch::Job::new(from, &content, name, folder, executable);
            serve_fetch(inner, permit, job, tx, rx).await
        }
        Request::Cancel {} => Ok(()),
        other => {
            let message = format!("this node cannot do {other:?} yet");
            tx.send(&Reply::Error { message }).await
        }
    }
}

fn failed(e: &anyhow::Error) -> Reply {
    Reply::Error {
        message: format!("{e:#}"),
    }
}

/// The target folder, open, once the name and folder pass the rules.
async fn check_fetch(inner: &Inner, name: &str, folder: &str) -> Result<Arc<Folder>> {
    place::plain_name(name)?;
    let (folders, folder) = (inner.folders.clone(), folder.to_owned());
    let folder = spawn_blocking(move || place::open_target(&folders, &folder)).await??;
    Ok(Arc::new(folder))
}

async fn serve_offer(
    inner: &Inner,
    _permit: OwnedSemaphorePermit,
    path: &str,
    peer: iroh::EndpointId,
    mut tx: LinkTx,
    mut rx: LinkRx,
) -> Result<()> {
    let resolved = {
        let (folders, path) = (inner.folders.clone(), path.to_owned());
        spawn_blocking(move || folders.resolve(&path)).await?
    };
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(e) => return tx.send(&failed(&e)).await,
    };
    let from = inner.parts.endpoint.addr();
    // Dropping the import when the server goes away leaves only blobs no
    // tag holds, which the store's collector drops.
    let mut out = Out::Stream(&mut tx);
    let offered = tokio::select! {
        offered = offer::offer(&inner.parts.store, &resolved, from, &mut out) => offered,
        _ = rx.recv::<Request>() => return Ok(()),
    };
    let offered = match offered {
        Ok(offered) => offered,
        Err(e) => return tx.send(&failed(&e)).await,
    };
    let _grant = inner.grants.grant(peer, offered.content);
    tx.send(&offered.reply).await?;
    // Open until the caller closes the stream or cancels; then the grant
    // and the tags go.
    loop {
        match rx.recv::<Request>().await {
            Ok(Some(Request::Cancel {})) | Ok(None) | Err(_) => return Ok(()),
            Ok(Some(other)) => log::debug!("an open offer ignored {other:?}"),
        }
    }
}

async fn serve_fetch(
    inner: &Inner,
    _permit: OwnedSemaphorePermit,
    job: fetch::Job,
    mut tx: LinkTx,
    mut rx: LinkRx,
) -> Result<()> {
    let cancel = CancellationToken::new();
    let result = {
        let mut out = Out::Stream(&mut tx);
        let work = fetch::fetch(&inner.parts, &cancel, job, &mut out);
        let mut work = std::pin::pin!(work);

        loop {
            tokio::select! {
                result = &mut work => break result,
                // The work stops at its next check and cleans up; it is
                // never dropped halfway through writing.
                message = rx.recv::<Request>(), if !cancel.is_cancelled() => match message {
                    Ok(Some(Request::Cancel {})) | Ok(None) | Err(_) => cancel.cancel(),
                    Ok(Some(other)) => log::debug!("a fetch ignored {other:?}"),
                },
            }
        }
    };
    let reply = match result {
        Ok(fetched) => Reply::Done {
            path: fetched.path.to_string_lossy().into_owned(),
            size: fetched.size,
            files: fetched.files,
        },
        Err(_) if cancel.is_cancelled() => Reply::Cancelled {},
        Err(e) => Reply::Error {
            message: format!("{e:#}"),
        },
    };

    if let Err(e) = tx.send(&reply).await {
        // A server that went away cannot hear it; the result stands.
        log::debug!("could not report a fetch: {e:#}");

        if matches!(reply, Reply::Done { .. }) {
            return Ok(());
        }

        bail!(e);
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/files_transfer.rs"]
mod tests;
