//! Following external sessions' transcripts for `Watch` streams: hook events
//! from the occupancy tell a session its hooks speak for it, and for sessions
//! that send no hooks their transcripts are read when the agents' stores ring.

use crate::logs::claude::transcript::{Digest, read_from};
use crate::{events::Change, occupancy::Occupancy};
use anyhow::Result;
use beep_proto::SessionId;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{Activity, ActivityKind, SessionsReply},
};
use std::{
    fs::{self, File},
    path::PathBuf,
    time::Duration,
};
use tokio::{
    sync::broadcast::{self, error::RecvError},
    task::spawn_blocking,
    time::{Instant, sleep_until},
};

/// The most sessions one stream follows.
const MAX_WATCHED: usize = 32;
/// How long a turn end seen in a transcript waits for its hook.
const HOOK_GRACE: Duration = Duration::from_secs(3);
/// The most of a transcript's growth read in one check.
const MAX_GROWTH: u64 = 4 * 1024 * 1024;

/// One followed transcript, read as it grows.
struct Tracker {
    session: SessionId,
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
    fn new(session: SessionId, path: PathBuf) -> Self {
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
        self.grow();

        due
    }

    /// Reads what the transcript gained, leaving any turn end pending.
    fn grow(&mut self) {
        if self.hooked {
            return;
        }

        if let Err(e) = self.read_growth() {
            log::debug!("could not read session {}: {e}", self.session);
        }
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
        if let Some(message) = digest.said.last_message {
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
/// sends their activity until the caller closes the stream.
pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    occupancy: &Occupancy,
    sessions: Vec<SessionId>,
    mut rings: broadcast::Receiver<Change>,
) -> Result<()> {
    let mut events = occupancy.hook_events();
    let store = occupancy.claude().store();
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
    // When the turn ends seen so far are sent, if no hook spoke for them.
    let mut release: Option<Instant> = None;

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
            ring = rings.recv() => match ring {
                Ok(change) if change.topic.name() != "agent" => {}
                Ok(_) | Err(RecvError::Lagged(_)) => {
                    // Once hooks report for every session, no transcript is read.
                    if trackers.iter().all(|tracker| tracker.hooked) {
                        continue;
                    }

                    trackers = spawn_blocking(move || {
                        trackers.iter_mut().for_each(Tracker::grow);
                        trackers
                    })
                    .await?;

                    if release.is_none() && trackers.iter().any(|tracker| tracker.pending.is_some()) {
                        release = Some(Instant::now() + HOOK_GRACE);
                    }
                }
                Err(RecvError::Closed) => return Ok(()),
            },
            () = async { sleep_until(release.unwrap_or_else(Instant::now)).await }, if release.is_some() => {
                release = None;
                let (back, due) = spawn_blocking(move || {
                    let due: Vec<Activity> = trackers.iter_mut().flat_map(Tracker::poll).collect();
                    (trackers, due)
                })
                .await?;
                trackers = back;

                if trackers.iter().any(|tracker| tracker.pending.is_some()) {
                    release = Some(Instant::now() + HOOK_GRACE);
                }

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
#[path = "../../tests/agents_claude_watch.rs"]
mod tests;
