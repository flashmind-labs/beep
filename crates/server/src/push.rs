//! Delivers notices to phones. A user's id is their phone's endpoint id, so
//! a notice goes over the user's open connection if there is one, or else
//! the server dials the phone, which waits on its home relay with no
//! connection open. A notice that can't be delivered is retried from time
//! to time, and sent at once when the phone next connects. With a fallback,
//! a notice iroh could not deliver goes through Firebase instead, and one
//! Firebase could not take wakes the phone to come for it.

use crate::{
    app::UserNotice,
    fcm::Fallback,
    protocol::{
        Notice,
        wire::{PUSH_ALPN, framed},
    },
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

const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// Notices kept per user while their phone is unreachable; older ones go first.
const MAX_QUEUED: usize = 50;
/// Waits between attempts to reach a phone with queued notices. The last
/// one repeats until `RETRY_FOR` has passed; after that the notices wait for
/// the phone to connect.
const RETRY_WAITS: [Duration; 5] = [
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
    Duration::from_secs(30 * 60),
];
const RETRY_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// A phone woken this recently is not woken again: it is on its way.
const WAKE_GAP: Duration = Duration::from_secs(20);

/// A user's undelivered notices, oldest first.
#[derive(Default)]
struct Pending {
    notices: VecDeque<Notice>,
    /// Set while a task is retrying them.
    retrying: bool,
}

pub struct Pusher {
    endpoint: Endpoint,
    /// The latest connection each user opened, while it lasts.
    live: Mutex<HashMap<EndpointId, Connection>>,
    queued: Mutex<HashMap<EndpointId, Pending>>,
    retry_waits: Vec<Duration>,
    fallback: Option<Arc<dyn Fallback>>,
    /// When each phone was last woken.
    woken: Mutex<HashMap<EndpointId, Instant>>,
}

impl Pusher {
    pub fn new(endpoint: Endpoint, fallback: Option<Arc<dyn Fallback>>) -> Arc<Self> {
        Self::with_retry_waits(endpoint, fallback, RETRY_WAITS.to_vec())
    }

    /// Like `new` with other waits between retries; tests shorten them.
    pub fn with_retry_waits(
        endpoint: Endpoint,
        fallback: Option<Arc<dyn Fallback>>,
        retry_waits: Vec<Duration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
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
                    Ok(UserNotice { user, notice }) => {
                        let Ok(id) = user.parse::<EndpointId>() else {
                            log::warn!("push: user id is not an endpoint id; user={user}");
                            continue;
                        };
                        let pusher = pusher.clone();
                        tokio::spawn(async move { pusher.deliver(id, notice).await });
                    }
                    Err(RecvError::Lagged(missed)) => log::warn!("push: missed {missed} notices"),
                    Err(RecvError::Closed) => return,
                }
            }
        });
    }

    /// Remembers `connection` as its user's way in until it closes, and
    /// sends what was queued while the phone was unreachable.
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

    async fn deliver(self: Arc<Self>, id: EndpointId, notice: Notice) {
        self.deliver_all(id, VecDeque::from([notice])).await;
    }

    /// Sends `notices` in order, and queues what could not be sent and
    /// wakes the phone to come for it.
    async fn deliver_all(self: Arc<Self>, id: EndpointId, notices: VecDeque<Notice>) {
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
    async fn push_all(&self, id: EndpointId, notices: VecDeque<Notice>) -> VecDeque<Notice> {
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
    async fn wake(&self, id: EndpointId) {
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

    /// Tries the queued notices again from time to time until they are sent,
    /// the phone connects and takes them, or `RETRY_FOR` runs out.
    async fn retry(self: Arc<Self>, id: EndpointId) {
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

    fn take(&self, id: EndpointId) -> VecDeque<Notice> {
        let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
        queued
            .get_mut(&id)
            .map(|pending| mem::take(&mut pending.notices))
            .unwrap_or_default()
    }

    /// Queues `unsent` ahead of anything queued since. Returns true if no
    /// task is retrying the queue yet, and marks one as started.
    fn put_back(&self, id: EndpointId, mut unsent: VecDeque<Notice>) -> bool {
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

    /// Sends what it can of `notices` over the user's open connection, or
    /// else by dialing the phone, and returns the rest.
    async fn send_all(&self, id: EndpointId, mut notices: VecDeque<Notice>) -> VecDeque<Notice> {
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
    async fn dial(&self, id: EndpointId) -> Result<Connection> {
        let connect = self.endpoint.connect(EndpointAddr::new(id), PUSH_ALPN);
        Ok(timeout(DIAL_TIMEOUT, connect)
            .await
            .context("dial timed out")??)
    }
}

/// Sends `notices` from the front, stopping at the first the phone does not take.
async fn send_each(connection: &Connection, notices: &mut VecDeque<Notice>) {
    while let Some(notice) = notices.front() {
        if let Err(e) = send(connection, notice).await {
            log::debug!("push: notice not taken; error={e:#}");
            return;
        }
        notices.pop_front();
    }
}

/// Sends `notice` on its own stream and waits for the phone's ack.
async fn send(connection: &Connection, notice: &Notice) -> Result<()> {
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

    /// A pusher on an endpoint with no relay or address lookup, so nothing
    /// here reaches the network.
    async fn pusher(retry_waits: Vec<Duration>) -> Arc<Pusher> {
        falling_back_pusher(retry_waits, None).await
    }

    async fn falling_back_pusher(
        retry_waits: Vec<Duration>,
        fallback: Option<Arc<dyn Fallback>>,
    ) -> Arc<Pusher> {
        let endpoint = Endpoint::builder(presets::Minimal)
            .bind()
            .await
            .expect("a local endpoint binds");
        Pusher::with_retry_waits(endpoint, fallback, retry_waits)
    }

    /// Records pushes and wakes instead of sending them. Takes only the
    /// notices whose title starts with "push".
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

    fn phone(n: u8) -> EndpointId {
        SecretKey::from_bytes(&[n; 32]).public()
    }

    fn notice(title: &str) -> Notice {
        Notice {
            title: title.into(),
            body: String::new(),
            session_id: String::new(),
            app: None,
            approval: None,
            changed: false,
        }
    }

    fn notices(titles: &[&str]) -> VecDeque<Notice> {
        titles.iter().map(|title| notice(title)).collect()
    }

    fn titles(notices: &VecDeque<Notice>) -> Vec<&str> {
        notices.iter().map(|notice| notice.title.as_str()).collect()
    }

    #[tokio::test]
    async fn put_back_starts_one_retry_per_queue() {
        let pusher = pusher(RETRY_WAITS.to_vec()).await;
        assert!(!pusher.put_back(phone(1), VecDeque::new()));
        assert!(pusher.put_back(phone(1), notices(&["a"])));
        assert!(!pusher.put_back(phone(1), notices(&["b"])));
        assert!(pusher.put_back(phone(2), notices(&["c"])));
    }

    #[tokio::test]
    async fn put_back_keeps_unsent_notices_ahead_of_newer_ones() {
        let pusher = pusher(RETRY_WAITS.to_vec()).await;
        pusher.put_back(phone(1), notices(&["c"]));
        pusher.put_back(phone(1), notices(&["a", "b"]));
        assert_eq!(titles(&pusher.take(phone(1))), ["a", "b", "c"]);
        assert!(pusher.take(phone(1)).is_empty());
        assert!(pusher.take(phone(2)).is_empty());
    }

    #[tokio::test]
    async fn put_back_drops_the_oldest_past_the_limit() {
        let pusher = pusher(RETRY_WAITS.to_vec()).await;
        let names: Vec<String> = (0..MAX_QUEUED + 5).map(|i| i.to_string()).collect();
        let all: Vec<&str> = names.iter().map(String::as_str).collect();
        pusher.put_back(phone(1), notices(&all[5..]));
        pusher.put_back(phone(1), notices(&all[..5]));
        let queued = pusher.take(phone(1));
        assert_eq!(queued.len(), MAX_QUEUED);
        assert_eq!(titles(&queued), all[5..]);
    }

    #[tokio::test]
    async fn retry_ends_once_the_phone_took_the_queue() {
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
    async fn an_unreachable_phone_is_woken_once_and_its_notices_wait() {
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
    async fn a_pushed_notice_is_not_queued_and_wakes_nobody() {
        let recorder = Arc::new(Recorder::default());
        let pusher = falling_back_pusher(RETRY_WAITS.to_vec(), Some(recorder.clone())).await;
        pusher.clone().deliver(phone(1), notice("push a")).await;
        assert_eq!(recorder.pushed(), ["push a"]);
        assert!(recorder.woken().is_empty());
        assert!(pusher.take(phone(1)).is_empty());
    }
}
