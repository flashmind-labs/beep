//! `transfer/1` (see `beep_proto::node::transfer`): sends files and folders
//! to other nodes over iroh-blobs. `Transfers` owns the blob store and serves
//! it only to the peers an open offer names.

use crate::{
    files::Folders,
    files::sync::{self, PeerHandler, Syncs},
    files::syncthing::Syncthing,
    link::{Protocol, Session},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    transfer::{ContentKind, PROTOCOL, Reply, Request, Stage},
};
use iroh::{
    Endpoint,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router, RouterBuilder},
};
use iroh_blobs::{
    BlobsProtocol, Hash, HashAndFormat,
    store::{
        GcConfig, ProtectCb, ProtectOutcome,
        fs::{FsStore, options::Options},
    },
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore, watch},
    task::spawn_blocking,
    time::{Instant, timeout},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

mod fetch;
pub(crate) mod grants;
pub(crate) mod offer;
pub(crate) mod place;

use grants::Grants;
use place::{Folder, Journal, Room};

/// The most transfers, offers and fetches together, running at once.
const MAX_RUNNING: usize = 4;
/// How long the collector, which drops the blobs no transfer holds any
/// more, waits for a transfer to end before it runs anyway.
const GC_IDLE: Duration = Duration::from_secs(5 * 60);
/// How often progress goes out while bytes move.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);

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
    async fn tick(&mut self, tx: &mut LinkTx, done: u64) -> Result<()> {
        self.done = done;
        if self.due(Instant::now()) {
            self.send(tx).await?;
        }
        Ok(())
    }

    /// Reports `done` now, as the stage's last report.
    async fn finish(&mut self, tx: &mut LinkTx, done: u64) -> Result<()> {
        self.done = done;
        self.send(tx).await
    }

    async fn send(&mut self, tx: &mut LinkTx) -> Result<()> {
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
    endpoint: Endpoint,
    store: FsStore,
    /// Where the store keeps its data.
    store_dir: PathBuf,
    /// Wakes the collector once a transfer ended and its tags are gone.
    collect: Arc<Notify>,
    folders: Folders,
    grants: Arc<Grants>,
    journal: Journal,
    /// The free space running fetches hold.
    room: Arc<Room>,
    running: Arc<Semaphore>,
    syncs: Syncs,
    // Dropping the router stops serving blobs; the handle ends the task
    // that answers its events. `None` when the caller serves `Served`.
    router: Option<Router>,
    _events: AbortOnDropHandle<()>,
}

/// What `Transfers` answers on the endpoint: the blobs and sync protocols,
/// for a router the caller runs.
pub struct Served {
    blobs: BlobsProtocol,
    sync: PeerHandler,
}

#[derive(Clone)]
enum Slot {
    Waiting,
    Ready {
        blobs: BlobsProtocol,
        sync: PeerHandler,
    },
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
        self.slot.send_replace(Slot::Ready {
            blobs: served.blobs,
            sync: served.sync,
        });
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
            Ok(_permit) => timeout(
                WAIT_LIMIT,
                slot.wait_for(|slot| !matches!(slot, Slot::Waiting)),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .map(|ready| ready.clone()),
            Err(_) => None,
        };
        match ready {
            Some(Slot::Ready { blobs, .. }) if self.blobs => blobs.accept(connection).await,
            Some(Slot::Ready { sync, .. }) => sync.accept(connection).await,
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
    /// protocols through a router of its own. Folders the machine's
    /// Syncthing keeps in step are not synced.
    pub async fn start(endpoint: &Endpoint, dir: &Path, folders: Folders) -> Result<Self> {
        Self::start_with(endpoint, dir, folders, Syncthing::local()).await
    }

    /// `start`, asking `syncthing` which folders it keeps in step.
    pub async fn start_with(
        endpoint: &Endpoint,
        dir: &Path,
        folders: Folders,
        syncthing: Syncthing,
    ) -> Result<Self> {
        Ok(Self::build(endpoint, dir, folders, syncthing, true)
            .await?
            .0)
    }

    /// `start`, but the blobs and sync protocols are left to the caller's
    /// router: add `Served` to it. Stop that router before `shutdown`.
    pub async fn start_shared(
        endpoint: &Endpoint,
        dir: &Path,
        folders: Folders,
    ) -> Result<(Self, Served)> {
        Self::build(endpoint, dir, folders, Syncthing::local(), false).await
    }

    async fn build(
        endpoint: &Endpoint,
        dir: &Path,
        folders: Folders,
        syncthing: Syncthing,
        own_router: bool,
    ) -> Result<(Self, Served)> {
        let collect = Arc::new(Notify::new());
        let mut options = Options::new(dir);
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
        let syncs = Syncs::start(
            sync::Parts {
                endpoint: endpoint.clone(),
                store: store.clone(),
                store_dir: dir.to_owned(),
                journal: journal.clone(),
                room: room.clone(),
            },
            folders.clone(),
            dir.join("sync"),
            syncthing,
        )
        .await?;
        let grants = Arc::new(Grants::default());
        grants.set_access(syncs.access());
        let (events, events_task) = grants::events(grants.clone());
        let served = Served {
            blobs: BlobsProtocol::new(&store, Some(events)),
            sync: PeerHandler {
                syncs: syncs.clone(),
            },
        };
        let router = own_router.then(|| {
            let (blobs, sync) = (served.blobs.clone(), served.sync.clone());
            Served { blobs, sync }
                .add_to(Router::builder(endpoint.clone()))
                .spawn()
        });
        let transfers = Self {
            inner: Arc::new(Inner {
                endpoint: endpoint.clone(),
                store,
                store_dir: dir.to_owned(),
                collect,
                folders,
                grants,
                journal,
                room,
                running: Arc::new(Semaphore::new(MAX_RUNNING)),
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
        if let Err(e) = self.inner.store.shutdown().await {
            log::debug!("the blob store stopped: {e}");
        }
        Ok(())
    }
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

/// Serves one `transfer/1` stream: one offer or one fetch.
async fn serve(inner: Arc<Inner>, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };
    let Ok(permit) = inner.running.clone().try_acquire_owned() else {
        let message = format!("busy: {MAX_RUNNING} transfers are running on this machine");
        return tx.send(&Reply::Error { message }).await;
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
            let hash = Hash::from_bytes(content.hash);
            let job = fetch::Job {
                from,
                content: match content.kind {
                    ContentKind::File => HashAndFormat::raw(hash),
                    ContentKind::Folder => HashAndFormat::hash_seq(hash),
                },
                kind: content.kind,
                name,
                folder,
                executable,
            };
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
    let from = inner.endpoint.addr();
    // Dropping the import when the server goes away leaves only blobs no
    // tag holds, which the store's collector drops.
    let offered = tokio::select! {
        offered = offer::offer(&inner.store, &resolved, from, &mut tx) => offered,
        _ = rx.recv::<Request>() => return Ok(()),
    };
    let offered = match offered {
        Ok(offered) => offered,
        Err(e) => return tx.send(&failed(&e)).await,
    };
    let _grant = inner.grants.grant(peer, offered.content);
    tx.send(&offered.reply).await?;
    // Open until the server closes the stream or cancels; then the grant
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
    let cx = fetch::Fetcher {
        endpoint: &inner.endpoint,
        store: &inner.store,
        store_dir: &inner.store_dir,
        journal: &inner.journal,
        room: &inner.room,
        cancel: &cancel,
    };
    let result = {
        let work = fetch::fetch(cx, job, &mut tx);
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
