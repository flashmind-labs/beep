//! `transfer/1` (see `beep_proto::node::transfer`): sending a file or
//! folder to another node directly over iroh-blobs. The node keeps one blob
//! store in its folder and serves the blobs protocol on its own endpoint,
//! to the peers an open offer names only.

use crate::{
    files::Folders,
    link::{Protocol, Session},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    transfer::{ContentKind, PROTOCOL, Reply, Request},
};
use iroh::{Endpoint, protocol::Router};
use iroh_blobs::{
    BlobsProtocol, Hash, HashAndFormat,
    store::{
        GcConfig,
        fs::{FsStore, options::Options},
    },
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    task::spawn_blocking,
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

mod fetch;
mod grants;
mod offer;
mod place;

use grants::Grants;
use place::Journal;

/// The most transfers, offers and fetches together, running at once.
const MAX_RUNNING: usize = 4;
/// How often blobs no transfer holds any more are dropped from the store.
const GC_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// The node's transfers: its store, the grants of its open offers and the
/// router serving the blobs protocol, all dropped together.
#[derive(Clone)]
pub struct Transfers {
    inner: Arc<Inner>,
}

struct Inner {
    endpoint: Endpoint,
    store: FsStore,
    folders: Folders,
    grants: Arc<Grants>,
    journal: Journal,
    running: Arc<Semaphore>,
    // Dropping the router stops serving blobs; the handle ends the task
    // that answers its events.
    _router: Router,
    _events: AbortOnDropHandle<()>,
}

impl Transfers {
    /// Opens the store in `dir`, removes partial transfers a stopped node
    /// left, and serves the blobs protocol on `endpoint`, which must not
    /// serve other protocols through a router of its own.
    pub async fn start(endpoint: &Endpoint, dir: &Path, folders: Folders) -> Result<Self> {
        let mut options = Options::new(dir);
        options.gc = Some(GcConfig {
            interval: GC_INTERVAL,
            add_protected: None,
        });
        let store = FsStore::load_with_opts(dir.join("blobs.db"), options)
            .await
            .with_context(|| format!("opening the blob store in {}", dir.display()))?;
        let journal = Journal::open(dir.join("partial"))?;
        let sweeping = journal.clone();
        spawn_blocking(move || sweeping.sweep()).await??;
        let grants = Arc::new(Grants::default());
        let (events, events_task) = grants::events(grants.clone());
        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, BlobsProtocol::new(&store, Some(events)))
            .spawn();
        Ok(Self {
            inner: Arc::new(Inner {
                endpoint: endpoint.clone(),
                store,
                folders,
                grants,
                journal,
                running: Arc::new(Semaphore::new(MAX_RUNNING)),
                _router: router,
                _events: events_task,
            }),
        })
    }
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
    match request {
        Request::Offer { path, peer } => serve_offer(&inner, permit, &path, peer, tx, rx).await,
        Request::Fetch {
            from,
            content,
            name,
            size,
            folder,
        } => {
            let checked = check_fetch(&inner, &name, &folder).await;
            let folder = match checked {
                Ok(folder) => folder,
                Err(e) => {
                    let message = format!("{e:#}");
                    return tx.send(&Reply::Error { message }).await;
                }
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
                size,
                folder,
            };
            serve_fetch(&inner, permit, job, tx, rx).await
        }
        Request::Cancel {} => Ok(()),
        other => {
            let message = format!("this node cannot do {other:?} yet");
            tx.send(&Reply::Error { message }).await
        }
    }
}

/// The resolved target folder, once the name and folder pass the rules.
async fn check_fetch(inner: &Inner, name: &str, folder: &str) -> Result<std::path::PathBuf> {
    place::plain_name(name)?;
    let (folders, folder) = (inner.folders.clone(), folder.to_owned());
    spawn_blocking(move || place::target_folder(&folders, &folder)).await?
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
        Err(e) => {
            let message = format!("{e:#}");
            return tx.send(&Reply::Error { message }).await;
        }
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
        Err(e) => {
            let message = format!("{e:#}");
            return tx.send(&Reply::Error { message }).await;
        }
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
        journal: &inner.journal,
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
