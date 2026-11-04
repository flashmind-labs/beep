//! Delivers notices to every key of the account: over its connection, by dialing it, or via FCM or APNs.

use super::CLOSE_REVOKED;
use crate::{
    app::UserNotice,
    protocol::{
        DeviceKind, Notice,
        wire::{PUSH_ALPN, framed},
    },
    push::remote::Fallback,
    store::Store,
};
use anyhow::{Context, Result};
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::{Connection, VarInt},
};
use std::{
    collections::{HashMap, VecDeque},
    mem,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::{
    sync::broadcast::{self, error::RecvError},
    time::{sleep, timeout},
};

/// The key now signs into another account; connecting again finds it.
pub(super) const CLOSE_MOVED: u32 = 409;

pub(super) const DIAL_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Notices kept per user while their phone is unreachable; older ones go first.
pub(super) const MAX_QUEUED: usize = 50;

/// Waits between retries; the last repeats until `RETRY_FOR` has passed.
pub(super) const RETRY_WAITS: [Duration; 5] = [
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
    Duration::from_secs(30 * 60),
];

pub(super) const RETRY_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// A phone woken this recently is not woken again: it is on its way.
pub(super) const WAKE_GAP: Duration = Duration::from_secs(20);

/// A key a notice goes to, and what kind of device holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushKey {
    pub id: EndpointId,
    pub kind: DeviceKind,
}

/// The keys a notice for an account goes to.
#[async_trait::async_trait]
pub trait Directory: Send + Sync {
    async fn keys(&self, user: &str) -> Result<Vec<PushKey>>;
}

#[async_trait::async_trait]
impl Directory for Store {
    async fn keys(&self, user: &str) -> Result<Vec<PushKey>> {
        let keys = self.live_keys(user).await?;
        Ok(keys
            .into_iter()
            .filter_map(|key| {
                Some(PushKey {
                    id: key.endpoint_id.parse().ok()?,
                    kind: key.kind,
                })
            })
            .collect())
    }
}

/// Sends each account's notices to its own key only, for tests.
pub struct OwnKey;

#[async_trait::async_trait]
impl Directory for OwnKey {
    async fn keys(&self, user: &str) -> Result<Vec<PushKey>> {
        Ok(user
            .parse()
            .into_iter()
            .map(|id| PushKey {
                id,
                kind: DeviceKind::Phone {},
            })
            .collect())
    }
}

/// A user's undelivered notices, oldest first.
#[derive(Default)]
pub(super) struct Pending {
    pub(super) notices: VecDeque<Notice>,
    /// Set while a task is retrying them.
    pub(super) retrying: bool,
}

pub struct Pusher {
    pub(super) endpoint: Endpoint,
    pub(super) directory: Arc<dyn Directory>,
    /// The latest connection each user opened, while it lasts.
    pub(super) live: Mutex<HashMap<EndpointId, Connection>>,
    pub(super) queued: Mutex<HashMap<EndpointId, Pending>>,
    pub(super) retry_waits: Vec<Duration>,
    pub(super) fallback: Option<Arc<dyn Fallback>>,
    /// When each phone was last woken.
    pub(super) woken: Mutex<HashMap<EndpointId, Instant>>,
}

impl Pusher {
    pub fn new(
        endpoint: Endpoint,
        directory: Arc<dyn Directory>,
        fallback: Option<Arc<dyn Fallback>>,
    ) -> Arc<Self> {
        Self::with_retry_waits(endpoint, directory, fallback, RETRY_WAITS.to_vec())
    }

    /// Like `new` with other waits between retries; tests shorten them.
    pub fn with_retry_waits(
        endpoint: Endpoint,
        directory: Arc<dyn Directory>,
        fallback: Option<Arc<dyn Fallback>>,
        retry_waits: Vec<Duration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            directory,
            live: Mutex::default(),
            queued: Mutex::default(),
            retry_waits,
            fallback,
            woken: Mutex::default(),
        })
    }

    /// Delivers every notice sent on `notices` until the channel closes.
    pub fn start(self: &Arc<Self>, mut notices: broadcast::Receiver<UserNotice>) {
        let pusher = self.clone();
        tokio::spawn(async move {
            loop {
                match notices.recv().await {
                    Ok(notice) => {
                        let pusher = pusher.clone();
                        tokio::spawn(async move { pusher.fan_out(notice).await });
                    }
                    Err(RecvError::Lagged(missed)) => log::warn!("push: missed {missed} notices"),
                    Err(RecvError::Closed) => return,
                }
            }
        });
    }

    /// Remembers `connection` until it closes and sends what was queued.
    pub fn connected(self: &Arc<Self>, connection: Connection) {
        let id = connection.remote_id();
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, connection.clone());
        let pusher = self.clone();
        tokio::spawn(async move {
            connection.closed().await;
            let mut live = pusher.live.lock().unwrap_or_else(PoisonError::into_inner);
            if live
                .get(&id)
                .is_some_and(|c| c.stable_id() == connection.stable_id())
            {
                live.remove(&id);
            }
        });
        let notices = self.take(id);
        if notices.is_empty() {
            return;
        }
        let pusher = self.clone();
        tokio::spawn(async move { pusher.deliver_all(id, notices).await });
    }

    /// The server's own key, which link offers are signed for.
    pub fn server_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Drops a removed key's connection and the notices queued for it.
    pub fn forget(&self, id: EndpointId) {
        let live = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        if let Some(connection) = live {
            connection.close(VarInt::from_u32(CLOSE_REVOKED), b"removed");
        }
        self.queued
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        self.woken
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }

    /// Closes `id`'s connection so it signs in again; its queued notices stay.
    pub fn reconnect(&self, id: EndpointId) {
        let live = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        if let Some(connection) = live {
            connection.close(VarInt::from_u32(CLOSE_MOVED), b"moved");
        }
    }

    pub(super) async fn fan_out(self: Arc<Self>, notice: UserNotice) {
        let user = &notice.user;
        let keys = match self.directory.keys(user).await {
            Ok(keys) => keys,
            Err(e) => {
                log::warn!("push: keys lookup failed; user={user} error={e:#}");
                return;
            }
        };
        let sends = keys.into_iter().filter_map(|key| {
            let sent = notice.for_device(&key.id, &key.kind)?.clone();
            Some(self.clone().deliver(key.id, sent))
        });
        futures::future::join_all(sends).await;
    }

    pub(super) async fn deliver(self: Arc<Self>, id: EndpointId, notice: Notice) {
        self.deliver_all(id, VecDeque::from([notice])).await;
    }

    /// Sends `notices` in order, queueing the rest and waking the phone for it.
    pub(super) async fn deliver_all(self: Arc<Self>, id: EndpointId, notices: VecDeque<Notice>) {
        let unsent = self.send_all(id, notices).await;
        let unsent = self.push_all(id, unsent).await;
        if unsent.is_empty() {
            return;
        }
        let start_retry = self.put_back(id, unsent);
        self.wake(id).await;
        if start_retry {
            tokio::spawn(self.retry(id));
        }
    }

    /// Sends `notices` through the fallback and returns those it did not take.
    pub(super) async fn push_all(
        &self,
        id: EndpointId,
        notices: VecDeque<Notice>,
    ) -> VecDeque<Notice> {
        let Some(fallback) = &self.fallback else {
            return notices;
        };
        let mut unsent = VecDeque::new();
        for notice in notices {
            if !fallback.push(id, &notice).await {
                unsent.push_back(notice);
            }
        }
        unsent
    }

    /// Wakes the phone through the fallback, unless it was woken just now.
    pub(super) async fn wake(&self, id: EndpointId) {
        let Some(fallback) = &self.fallback else {
            return;
        };
        {
            let mut woken = self.woken.lock().unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            woken.retain(|_, at| now.duration_since(*at) < WAKE_GAP);
            if woken.contains_key(&id) {
                return;
            }
            woken.insert(id, now);
        }
        fallback.wake(id).await;
    }

    /// Retries the queued notices until sent, taken, or `RETRY_FOR` runs out.
    pub(super) async fn retry(self: Arc<Self>, id: EndpointId) {
        let started = Instant::now();
        let last = self.retry_waits.len().saturating_sub(1);
        for attempt in 0.. {
            let wait = self.retry_waits.get(attempt.min(last)).copied();
            sleep(wait.unwrap_or(RETRY_WAITS[0])).await;
            let notices = {
                let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(pending) = queued.get_mut(&id) else {
                    return;
                };
                if pending.notices.is_empty() || started.elapsed() > RETRY_FOR {
                    pending.retrying = false;
                    if pending.notices.is_empty() {
                        queued.remove(&id);
                    }
                    return;
                }
                mem::take(&mut pending.notices)
            };
            let unsent = self.send_all(id, notices).await;
            let unsent = self.push_all(id, unsent).await;
            let missed = !unsent.is_empty();
            self.put_back(id, unsent);
            if missed {
                self.wake(id).await;
            }
        }
    }

    pub(super) fn take(&self, id: EndpointId) -> VecDeque<Notice> {
        let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
        queued
            .get_mut(&id)
            .map(|pending| mem::take(&mut pending.notices))
            .unwrap_or_default()
    }

    /// Queues `unsent` first; true if no retry task runs yet, marking one started.
    pub(super) fn put_back(&self, id: EndpointId, mut unsent: VecDeque<Notice>) -> bool {
        if unsent.is_empty() {
            return false;
        }
        log::info!(
            "push: phone unreachable, {} queued; user={id}",
            unsent.len()
        );
        let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
        let pending = queued.entry(id).or_default();
        unsent.append(&mut pending.notices);
        while unsent.len() > MAX_QUEUED {
            unsent.pop_front();
        }
        pending.notices = unsent;
        !mem::replace(&mut pending.retrying, true)
    }

    /// Sends what it can over the open connection or by dialing, and returns the rest.
    pub(super) async fn send_all(
        &self,
        id: EndpointId,
        mut notices: VecDeque<Notice>,
    ) -> VecDeque<Notice> {
        let live = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned();
        if let Some(connection) = live {
            send_each(&connection, &mut notices).await;
            if notices.is_empty() {
                return notices;
            }
        }
        match self.dial(id).await {
            Ok(connection) => {
                send_each(&connection, &mut notices).await;
                connection.close(VarInt::from_u32(0), b"done");
            }
            Err(e) => log::debug!("push: dial failed; user={id} error={e:#}"),
        }
        notices
    }

    /// Dials the phone just for notices; the caller hangs up.
    pub(super) async fn dial(&self, id: EndpointId) -> Result<Connection> {
        let connect = self.endpoint.connect(EndpointAddr::new(id), PUSH_ALPN);
        Ok(timeout(DIAL_TIMEOUT, connect)
            .await
            .context("dial timed out")??)
    }
}

/// Sends `notices` from the front, stopping at the first the phone does not take.
pub(super) async fn send_each(connection: &Connection, notices: &mut VecDeque<Notice>) {
    while let Some(notice) = notices.front() {
        if let Err(e) = send(connection, notice).await {
            log::debug!("push: notice not taken; error={e:#}");
            return;
        }
        notices.pop_front();
    }
}

/// Sends `notice` on its own stream and waits for the phone's ack.
pub(super) async fn send(connection: &Connection, notice: &Notice) -> Result<()> {
    let exchange = async {
        let (send, recv) = connection.open_bi().await?;
        let (mut tx, mut rx) = framed(send, recv);
        tx.ok(notice).await?;
        tx.finish()?;
        rx.reply::<()>()
            .await?
            .context("phone did not take the notice")
    };
    timeout(SEND_TIMEOUT, exchange)
        .await
        .context("notice timed out")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::{SecretKey, endpoint::presets};

    /// A pusher that never reaches the network.
    pub(super) async fn pusher(retry_waits: Vec<Duration>) -> Arc<Pusher> {
        falling_back_pusher(retry_waits, None).await
    }

    pub(super) async fn falling_back_pusher(
        retry_waits: Vec<Duration>,
        fallback: Option<Arc<dyn Fallback>>,
    ) -> Arc<Pusher> {
        let endpoint = Endpoint::builder(presets::Minimal)
            .bind()
            .await
            .expect("a local endpoint binds");
        Pusher::with_retry_waits(endpoint, Arc::new(OwnKey), fallback, retry_waits)
    }

    /// Records pushes and wakes; takes only notices titled "push...".
    #[derive(Default)]
    struct Recorder {
        pushed: Mutex<Vec<String>>,
        woken: Mutex<Vec<EndpointId>>,
    }

    #[async_trait::async_trait]
    impl Fallback for Recorder {
        async fn push(&self, _phone: EndpointId, notice: &Notice) -> bool {
            let taken = notice.title.starts_with("push");
            if taken {
                self.pushed
                    .lock()
                    .expect("no test thread panics holding it")
                    .push(notice.title.clone());
            }
            taken
        }

        async fn wake(&self, phone: EndpointId) {
            self.woken
                .lock()
                .expect("no test thread panics holding it")
                .push(phone);
        }
    }

    impl Recorder {
        fn pushed(&self) -> Vec<String> {
            self.pushed
                .lock()
                .expect("no test thread panics holding it")
                .clone()
        }

        fn woken(&self) -> Vec<EndpointId> {
            self.woken
                .lock()
                .expect("no test thread panics holding it")
                .clone()
        }
    }

    pub(super) fn phone(n: u8) -> EndpointId {
        SecretKey::from_bytes(&[n; 32]).public()
    }

    pub(super) fn notice(title: &str) -> Notice {
        Notice {
            title: title.into(),
            body: String::new(),
            session_id: String::new(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        }
    }

    pub(super) fn notices(titles: &[&str]) -> VecDeque<Notice> {
        titles.iter().map(|title| notice(title)).collect()
    }

    pub(super) fn titles(notices: &VecDeque<Notice>) -> Vec<&str> {
        notices.iter().map(|notice| notice.title.as_str()).collect()
    }

    #[tokio::test]
    pub(super) async fn put_back_starts_one_retry_per_queue() {
        let pusher = pusher(RETRY_WAITS.to_vec()).await;
        assert!(!pusher.put_back(phone(1), VecDeque::new()));
        assert!(pusher.put_back(phone(1), notices(&["a"])));
        assert!(!pusher.put_back(phone(1), notices(&["b"])));
        assert!(pusher.put_back(phone(2), notices(&["c"])));
    }

    #[tokio::test]
    pub(super) async fn put_back_keeps_unsent_notices_ahead_of_newer_ones() {
        let pusher = pusher(RETRY_WAITS.to_vec()).await;
        pusher.put_back(phone(1), notices(&["c"]));
        pusher.put_back(phone(1), notices(&["a", "b"]));
        assert_eq!(titles(&pusher.take(phone(1))), ["a", "b", "c"]);
        assert!(pusher.take(phone(1)).is_empty());
        assert!(pusher.take(phone(2)).is_empty());
    }

    #[tokio::test]
    pub(super) async fn retry_ends_once_the_phone_took_the_queue() {
        let pusher = pusher(vec![Duration::from_millis(10)]).await;
        assert!(pusher.put_back(phone(1), notices(&["a"])));
        assert_eq!(titles(&pusher.take(phone(1))), ["a"]);
        timeout(Duration::from_secs(5), pusher.clone().retry(phone(1)))
            .await
            .expect("retry stops when nothing is queued");
        assert!(
            !pusher
                .queued
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains_key(&phone(1))
        );
        // With the queue gone, the next failure starts a retry again.
        assert!(pusher.put_back(phone(1), notices(&["b"])));
    }

    #[tokio::test]
    pub(super) async fn an_unreachable_phone_is_woken_once_and_its_notices_wait() {
        let recorder = Arc::new(Recorder::default());
        let pusher = falling_back_pusher(RETRY_WAITS.to_vec(), Some(recorder.clone())).await;
        // Nothing can reach this phone, so the dial fails.
        pusher.clone().deliver(phone(1), notice("a")).await;
        pusher.clone().deliver(phone(1), notice("b")).await;
        assert_eq!(recorder.woken(), [phone(1)]);
        let queued = pusher.take(phone(1));
        let mut waiting = titles(&queued);
        waiting.sort();
        assert_eq!(waiting, ["a", "b"]);
    }

    #[tokio::test]
    pub(super) async fn a_pushed_notice_is_not_queued_and_wakes_nobody() {
        let recorder = Arc::new(Recorder::default());
        let pusher = falling_back_pusher(RETRY_WAITS.to_vec(), Some(recorder.clone())).await;
        pusher.clone().deliver(phone(1), notice("push a")).await;
        assert_eq!(recorder.pushed(), ["push a"]);
        assert!(recorder.woken().is_empty());
        assert!(pusher.take(phone(1)).is_empty());
    }
}
