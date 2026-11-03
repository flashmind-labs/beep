//! `webhook/1`: live updates for a project (see `beep_proto::node::webhook`).
//! The server opens a session per repository it wants live and keeps it
//! open while it does. The node forwards that repository's webhook into its
//! listener and passes each signed delivery's event on. When the session
//! ends, however it ends, the node stops the forward and deletes the
//! webhook it made, and no other.

mod hosts;
mod listener;
mod signature;

use crate::link::{Protocol, Session};
use anyhow::{Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    webhook::{PROTOCOL, PollReason, Watch, WatchHost, WatchReply},
};
use hosts::{Forgejo, Forward, GitHub, HostApi, Made, Refusal};
use listener::{Listeners, Reach};
use secrecy::SecretString;
use std::{
    future::{Future, pending},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::time::timeout;
use tokio_util::task::TaskTracker;

/// The most repositories one node watches at once.
const MAX_WATCHES: usize = 16;
/// How long removing a webhook may take once its session is gone.
const CLEANUP_LIMIT: Duration = Duration::from_secs(60);

/// The `webhook/1` protocol.
#[derive(Clone)]
pub struct Webhooks {
    inner: Arc<Inner>,
}

struct Inner {
    /// Where Forgejo and Gitea servers reach this node; `None` takes
    /// GitHub only.
    listen: Option<SocketAddr>,
    listeners: Listeners,
    active: AtomicUsize,
    /// Removals still running after their session ended.
    cleanups: TaskTracker,
}

impl Webhooks {
    /// Webhooks taken on `listen` for Forgejo and Gitea, besides GitHub's
    /// through `gh webhook forward`. Refuses an unspecified address such as
    /// `0.0.0.0`: the listener binds the private-network address only.
    pub fn new(listen: Option<SocketAddr>) -> Result<Self> {
        if let Some(addr) = listen
            && addr.ip().is_unspecified()
        {
            bail!(
                "webhook_listen must be this machine's private-network address, not {}",
                addr.ip()
            );
        }
        Ok(Self {
            inner: Arc::new(Inner {
                listen,
                listeners: Listeners::default(),
                active: AtomicUsize::new(0),
                cleanups: TaskTracker::new(),
            }),
        })
    }
}

impl Protocol for Webhooks {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, rx: LinkRx) -> Session {
        Box::pin(serve(self.inner.clone(), tx, rx))
    }
}

/// A watch counted against `MAX_WATCHES` until dropped.
struct Slot(Arc<Inner>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Inner {
    fn slot(self: &Arc<Self>) -> Option<Slot> {
        let taken = self.active.fetch_add(1, Ordering::Relaxed);
        let slot = Slot(self.clone());
        (taken < MAX_WATCHES).then_some(slot)
    }
}

/// The server's end of a session, as a watch uses it.
trait Server: Send {
    fn reply(&mut self, reply: &WatchReply) -> impl Future<Output = Result<()>> + Send;
    /// Finishes once the server closed the session.
    fn closed(&mut self) -> impl Future<Output = ()> + Send;
}

/// A session stream of the node link.
struct Link {
    tx: LinkTx,
    rx: LinkRx,
}

impl Server for Link {
    async fn reply(&mut self, reply: &WatchReply) -> Result<()> {
        self.tx.send(reply).await
    }

    async fn closed(&mut self) {
        // Nothing more is asked of a watch: anything else sent is skipped.
        while let Ok(Some(_)) = self.rx.recv::<Watch>().await {}
    }
}

async fn serve(inner: Arc<Inner>, tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(watch) = rx.recv::<Watch>().await? else {
        return Ok(());
    };
    let mut server = Link { tx, rx };
    let Some(_slot) = inner.slot() else {
        return polling(&mut server, Refusal::new(PollReason::Busy, "")).await;
    };
    let secret = SecretString::from(watch.secret.expose());
    match &watch.host {
        WatchHost::GitHub { token_var } => {
            let host = GitHub::new(&watch.repo, token_var.as_deref());
            run(&inner, host, Reach::Loopback, &watch, secret, &mut server).await
        }
        WatchHost::Forgejo { host } => match inner.listen {
            Some(addr) => {
                let host = Forgejo {
                    repo: watch.repo.clone(),
                    host: host.clone(),
                };
                run(
                    &inner,
                    host,
                    Reach::Private(addr),
                    &watch,
                    secret,
                    &mut server,
                )
                .await
            }
            None => polling(&mut server, Refusal::new(PollReason::NoAddress, "")).await,
        },
        other => {
            let detail = format!("unknown host {other:?}");
            polling(&mut server, Refusal::new(PollReason::Failed, detail)).await
        }
    }
}

async fn polling(server: &mut impl Server, refusal: Refusal) -> Result<()> {
    if !refusal.detail.is_empty() {
        log::info!("no live updates ({}): {}", refusal.why, refusal.detail);
    }
    server
        .reply(&WatchReply::Polling {
            why: refusal.why,
            detail: refusal.detail,
        })
        .await
}

/// Watches one repository on `host` until the server closes the session
/// or the forward stops.
async fn run<H: HostApi>(
    inner: &Arc<Inner>,
    host: H,
    reach: Reach,
    watch: &Watch,
    secret: SecretString,
    server: &mut impl Server,
) -> Result<()> {
    match host.is_admin().await {
        Ok(true) => {}
        Ok(false) => return polling(server, Refusal::new(PollReason::NotAdmin, "")).await,
        Err(refusal) => return polling(server, refusal).await,
    }
    let host = Arc::new(host);
    // Only the ids the server kept of webhooks Beep made.
    for &stale in &watch.stale {
        match host.delete(stale).await {
            Ok(()) => server.reply(&WatchReply::Removed { hook: stale }).await?,
            Err(refusal) => log::info!("an old webhook was not removed: {}", refusal.detail),
        }
    }
    let mut route = match inner.listeners.route(reach, secret.clone()).await {
        Ok(route) => route,
        Err(e) => {
            let refusal = Refusal::new(PollReason::NoAddress, format!("{e:#}"));
            return polling(server, refusal).await;
        }
    };
    let made = match host.make(&route.url, &secret).await {
        Ok(made) => made,
        Err(refusal) => return polling(server, refusal).await,
    };
    let Made { hook, mut forward } = made;
    let mut cleanup = Cleanup {
        host: Some(host),
        hook,
        tracker: inner.cleanups.clone(),
    };
    server.reply(&WatchReply::Live { hook }).await?;
    loop {
        tokio::select! {
            event = route.events.recv() => match event {
                Some(event) => server.reply(&WatchReply::Event { event }).await?,
                None => break,
            },
            () = server.closed() => break,
            news = forward_news(&mut forward) => match news {
                News::Line(line) => log::debug!("gh webhook forward: {line}"),
                News::Exited(how) => {
                    log::info!("gh webhook forward stopped: {how}");
                    break;
                }
            },
        }
    }
    // Stopped first, so nothing posts to a webhook being deleted.
    drop(forward);
    drop(route);
    if let Some(removed) = cleanup.now().await {
        // The server may have closed already; it learns of the removal
        // at the next watch then.
        if let Err(e) = server.reply(&WatchReply::Removed { hook: removed }).await {
            log::debug!("the removal was not reported: {e:#}");
        }
    }
    Ok(())
}

/// What the forward did.
enum News {
    /// A line on its stderr, read so the pipe never fills.
    Line(String),
    /// It exited, saying how.
    Exited(String),
}

/// The forward's next line, or its exit once its stderr ended; never
/// without one.
async fn forward_news(forward: &mut Option<Forward>) -> News {
    let Some(forward) = forward else {
        return pending().await;
    };
    if let Ok(Some(line)) = forward.stderr.next_line().await {
        return News::Line(line);
    }
    News::Exited(match forward.child.wait().await {
        Ok(status) => status.to_string(),
        Err(e) => e.to_string(),
    })
}

/// Deletes the webhook a watch made: now, or once the watch is dropped,
/// as when the connection to the server ends.
struct Cleanup<H: HostApi> {
    host: Option<Arc<H>>,
    hook: Option<u64>,
    tracker: TaskTracker,
}

impl<H: HostApi> Cleanup<H> {
    /// Deletes the webhook; its id once it is gone.
    async fn now(&mut self) -> Option<u64> {
        let host = self.host.take()?;
        let hook = self.hook.take()?;
        remove(host.as_ref(), hook).await.then_some(hook)
    }
}

impl<H: HostApi> Drop for Cleanup<H> {
    fn drop(&mut self) {
        let (Some(host), Some(hook)) = (self.host.take(), self.hook.take()) else {
            return;
        };
        // Bounded by `MAX_WATCHES` and by `CLEANUP_LIMIT`.
        self.tracker.spawn(async move {
            remove(host.as_ref(), hook).await;
        });
    }
}

async fn remove<H: HostApi>(host: &H, hook: u64) -> bool {
    match timeout(CLEANUP_LIMIT, host.delete(hook)).await {
        Ok(Ok(())) => true,
        Ok(Err(refusal)) => {
            log::warn!("the webhook Beep made was not removed: {}", refusal.detail);
            false
        }
        Err(_) => {
            log::warn!("removing the webhook Beep made took too long");
            false
        }
    }
}

#[cfg(test)]
mod tests;
