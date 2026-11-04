//! Following external sessions: hook events from the socket, passed to
//! every `Watch` stream through a broadcast, and for sessions that send no
//! hooks, their transcripts read as they grow.

use super::{
    discover::Store,
    hooks::{HookEvent, HookMessage},
    live::Live,
    transcript::{Digest, read_from},
};
use crate::link::local_socket::{self as socket, Listener, Stream};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{Activity, ActivityKind, SessionsReply},
};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::broadcast::{self, error::RecvError},
    task::spawn_blocking,
    time::{MissedTickBehavior, interval, timeout},
};

/// The longest message an activity carries.
pub(super) const MAX_MESSAGE: usize = 1500;
/// The most sessions one stream follows.
const MAX_WATCHED: usize = 32;
/// How often transcripts are checked for growth.
const POLL: Duration = Duration::from_secs(3);
/// The most of a transcript's growth read in one check.
const MAX_GROWTH: u64 = 4 * 1024 * 1024;
/// Hook events waiting for slow streams.
const HUB_SIZE: usize = 64;
/// How long a hook has to send its line.
const HOOK_READ: Duration = Duration::from_secs(2);
const MAX_HOOK_LINE: u64 = 64 * 1024;

/// Hands hook events to every stream that follows sessions.
pub(crate) struct Hub(broadcast::Sender<Activity>);

impl Default for Hub {
    fn default() -> Self {
        Self(broadcast::channel(HUB_SIZE).0)
    }
}

impl Hub {
    fn send(&self, activity: Activity) {
        // Nobody follows anything now; the event is not needed.
        let _ = self.0.send(activity);
    }
}

/// Takes hook events on `socket` until the node stops, one connection at a
/// time, each a single line answered with what the hook prints.
pub(crate) async fn listen(socket: &Path, hub: &Hub, live: &Live) -> Result<()> {
    let mut listener = Listener::bind(socket)?;
    log::info!("taking Claude Code hooks on {}", socket::shown(socket));
    loop {
        let stream = listener.accept().await?;
        match timeout(HOOK_READ, read_hook(stream, live)).await {
            Ok(Ok(Some(activity))) => {
                log::debug!(
                    "hook: session {} {}",
                    activity.session,
                    activity.kind.as_str()
                );
                hub.send(activity);
            }
            Ok(Ok(None)) => {}
            Ok(Err(e)) => log::debug!("skipped a hook: {e:#}"),
            Err(_) => log::debug!("a hook sent nothing in time"),
        }
    }
}

async fn read_hook(mut stream: Stream, live: &Live) -> Result<Option<Activity>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut line = String::new();
    BufReader::new((&mut stream).take(MAX_HOOK_LINE))
        .read_line(&mut line)
        .await?;
    let message: HookMessage = serde_json::from_str(&line).context("not a hook message")?;
    let answer = live.hear(&message);
    if let Some(answer) = &answer {
        stream.write_all(answer.to_string().as_bytes()).await?;
        stream.shutdown().await?;
    }
    // A blocked stop goes on with the turn, so the turn did not end.
    if message.event == HookEvent::Stop && answer.is_some() {
        return Ok(None);
    }
    Ok(message.activity())
}

/// One followed transcript, read as it grows.
struct Tracker {
    session: String,
    path: PathBuf,
    /// Where the next read starts.
    offset: u64,
    /// Hooks report this session, so its transcript is not needed.
    hooked: bool,
    /// A turn end seen in the transcript, sent after one more check if no
    /// hook reported it meanwhile.
    pending: Option<Activity>,
    last_message: String,
}

impl Tracker {
    fn new(session: String, path: PathBuf) -> Self {
        let offset = fs::metadata(&path).map_or(0, |meta| meta.len());
        Self {
            session,
            path,
            offset,
            hooked: false,
            pending: None,
            last_message: String::new(),
        }
    }

    /// Checks the transcript; returns what is due to be sent.
    fn poll(&mut self) -> Vec<Activity> {
        if self.hooked {
            return Vec::new();
        }
        let due: Vec<Activity> = self.pending.take().into_iter().collect();
        if let Err(e) = self.read_growth() {
            log::debug!("could not read session {}: {e}", self.session);
        }
        due
    }

    fn read_growth(&mut self) -> std::io::Result<()> {
        let mut file = File::open(&self.path)?;
        let len = file.metadata()?.len();
        if len < self.offset {
            // Rewritten: start over from its end.
            self.offset = len;
            return Ok(());
        }
        if len == self.offset {
            return Ok(());
        }
        let start = self.offset.max(len.saturating_sub(MAX_GROWTH));
        let tail = read_from(&mut file, start, len)?;
        self.offset = tail.end.max(self.offset);
        let digest = Digest::read(&tail.text);
        if let Some(message) = digest.last_message {
            self.last_message = message;
        }
        if digest.turns_ended > 0 {
            self.pending = Some(Activity {
                session: self.session.clone(),
                kind: ActivityKind::TurnDone,
                message: self.last_message.clone(),
                from_hook: false,
            });
        }
        Ok(())
    }

    /// A hook spoke for this session: it reports from now on.
    fn hook(&mut self) {
        self.hooked = true;
        self.pending = None;
    }
}

/// Serves one `Watch` stream: says which sessions it could not find, then
/// sends their activity until the server closes the stream.
pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    store: Option<Store>,
    hub: &Hub,
    sessions: Vec<String>,
) -> Result<()> {
    let mut events = hub.0.subscribe();
    let (mut trackers, unknown) = spawn_blocking(move || {
        let mut trackers = Vec::new();
        let mut unknown = Vec::new();
        for session in sessions.into_iter().take(MAX_WATCHED) {
            match store.as_ref().and_then(|store| store.find(&session)) {
                Some(path) => trackers.push(Tracker::new(session, path)),
                None => unknown.push(session),
            }
        }
        (trackers, unknown)
    })
    .await?;
    tx.send(&SessionsReply::Watching { unknown }).await?;
    let mut tick = interval(POLL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(activity) => {
                    let Some(tracker) = trackers.iter_mut().find(|t| t.session == activity.session) else {
                        continue;
                    };
                    tracker.hook();
                    tx.send(&SessionsReply::Activity { activity }).await?;
                }
                Err(RecvError::Lagged(missed)) => log::debug!("watch missed {missed} hook events"),
                Err(RecvError::Closed) => return Ok(()),
            },
            _ = tick.tick() => {
                let (back, due) = spawn_blocking(move || {
                    let due: Vec<Activity> = trackers.iter_mut().flat_map(Tracker::poll).collect();
                    (trackers, due)
                })
                .await?;
                trackers = back;
                for activity in due {
                    tx.send(&SessionsReply::Activity { activity }).await?;
                }
            }
            // The server ends the stream to stop following.
            message = rx.recv::<serde_json::Value>() => match message {
                Ok(None) | Err(_) => return Ok(()),
                Ok(Some(_)) => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::claude::hooks::{HookEvent, send};
    use std::io::Write;

    const ID: &str = "0f3c2a9e-1111-4222-8333-944455556666";

    fn append(path: &Path, text: &str) {
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn a_turn_end_in_the_transcript_is_sent_one_check_later() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let path = dir.join(format!("{ID}.jsonl"));
        fs::write(&path, "{\"type\":\"permission-mode\"}\n").unwrap();
        let mut tracker = Tracker::new(ID.into(), path.clone());
        assert!(tracker.poll().is_empty());
        append(
            &path,
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}\n\
             {\"type\":\"system\",\"subtype\":\"turn_duration\"}\n{\"type\":\"assis",
        );
        assert!(tracker.poll().is_empty(), "held for one check");
        let due = tracker.poll();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].kind, ActivityKind::TurnDone);
        assert_eq!(due[0].message, "Done.");
        assert!(!due[0].from_hook);
        // The half-written line is read once it ends; no turn end in it.
        append(&path, "tant\",\"message\":{\"content\":[]}}\n");
        assert!(tracker.poll().is_empty());
        assert!(tracker.poll().is_empty());

        // Once a hook spoke, the transcript is left alone.
        append(
            &path,
            "{\"type\":\"system\",\"subtype\":\"turn_duration\"}\n",
        );
        tracker.poll();
        tracker.hook();
        assert!(tracker.poll().is_empty());
    }

    #[tokio::test]
    async fn a_hook_reaches_the_hub_over_the_socket() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let socket = dir.join("hooks.sock");
        let hub = Hub::default();
        let mut events = hub.0.subscribe();
        let listening = {
            let socket = socket.clone();
            async move { listen(&socket, &hub, &Live::default()).await }
        };
        let sent = async {
            // The listener binds before its first await, so a failed
            // connect only means it has not run yet.
            for _ in 0..50 {
                let message = HookMessage {
                    event: HookEvent::Notification,
                    session_id: ID.into(),
                    message: Some("Claude needs your permission to use Bash".into()),
                    notification_type: Some("permission_prompt".into()),
                    tool: None,
                    detail: None,
                    file: None,
                    todos: None,
                    error: None,
                };
                if send(&socket, &message).await.is_ok() {
                    return events.recv().await.unwrap();
                }
                tokio::task::yield_now().await;
            }
            panic!("the socket never opened");
        };
        let activity = tokio::select! {
            activity = sent => activity,
            result = listening => panic!("the listener stopped: {result:?}"),
        };
        assert_eq!(activity.session, ID);
        assert_eq!(activity.kind, ActivityKind::NeedsInput);
        assert!(activity.from_hook);
    }
}
