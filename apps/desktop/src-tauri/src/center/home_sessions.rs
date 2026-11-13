//! The machines' agent sessions for Home. Home asks every machine for
//! them, so one slow machine would hold all of Home back: Home waits a
//! moment, then draws with the last list, and the answer that comes later
//! has it draw again.

use crate::{
    agents::sessions::machine_sessions,
    center::cache_view,
    center::view_cache::{View, ViewCache},
    error::{DesktopError, Result},
    fleet::{AsyncMutex, Fleet},
    machines::list::MachineLists,
};
use anyhow::anyhow;
use beep_proto::{
    AgentStatus, MachineSession,
    node::agent::{ExternalSession, Presence},
};
use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{sync::oneshot, time::timeout};

/// How long Home waits for the list when it has an older one to show.
const WAIT: Duration = Duration::from_millis(1500);
/// A list this new is shown as it is, without asking again. It covers the
/// redraw that a late answer asks for, and the many Home redraws of a turn:
/// a session starting or ending marks the list stale, and live reports
/// update it in place.
const FRESH: Duration = Duration::from_secs(60);
/// Tells the windows a late answer came, for Home to load again.
pub const HOME_CHANGED: &str = "home-changed";

pub struct AgentSessions {
    last: Mutex<Option<Listed>>,
    /// Held while the machines are asked, so only one ask runs at a time.
    asking: Arc<AsyncMutex<()>>,
}

#[derive(Clone)]
struct Listed {
    age: Age,
    sessions: Vec<MachineSession>,
}

/// How old the last list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Age {
    /// Read from disk at start: Home shows it at once, and the answer draws Home again.
    Saved,
    /// A session started or ended since: Home waits up to `WAIT` for the new list.
    Stale,
    /// Listed then.
    Listed(Instant),
}

impl AgentSessions {
    /// Starts from the list saved in `cache`, if it holds one.
    pub fn new(cache: &ViewCache) -> Self {
        let last = match cache
            .get(View::AgentSessions)
            .map(serde_json::from_value::<Vec<MachineSession>>)
        {
            Some(Ok(sessions)) => Some(Listed {
                age: Age::Saved,
                sessions,
            }),
            Some(Err(e)) => {
                // Only a head start: the machines' list replaces it.
                log::info!("the saved agent sessions do not parse: {e}");
                None
            }
            None => None,
        };
        Self {
            last: Mutex::new(last),
            asking: Arc::new(AsyncMutex::new(())),
        }
    }

    fn last(&self) -> MutexGuard<'_, Option<Listed>> {
        self.last.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the last list, for what this app knows before the next read.
    pub fn edit(&self, change: impl FnOnce(&mut [MachineSession])) {
        if let Some(listed) = self.last().as_mut() {
            change(listed.sessions.as_mut_slice());
        }
    }

    /// Takes one external session's live report into the last list, so a
    /// change to one session does not list every machine again.
    pub(crate) fn live(&self, machine_id: &str, report: &ExternalSession) {
        let status = match report.presence {
            Presence::Working => AgentStatus::Working {},
            Presence::Waiting => AgentStatus::Waiting {},
            Presence::Running | Presence::Recent => AgentStatus::Running {},
            Presence::Stopped => AgentStatus::Ended {},
            _ => return,
        };
        let at = UNIX_EPOCH + Duration::from_secs(report.last_activity);

        self.edit(|sessions| {
            let held = sessions.iter_mut().find(|session| {
                session.external
                    && session.machine_id == machine_id
                    && session.id == report.id
                    && session.agent == report.agent
            });

            if let Some(session) = held {
                session.status = status;
                session.last_activity = session.last_activity.max(at);

                if report.title.is_some() {
                    session.title.clone_from(&report.title);
                }
            }
        });
    }

    pub(crate) fn stale(&self) {
        // A list from disk is older still and keeps drawing at once.
        if let Some(listed) = self.last().as_mut()
            && matches!(listed.age, Age::Listed(_))
        {
            listed.age = Age::Stale;
        }
    }

    /// The sessions, as fresh as `WAIT` allows; the list kept on disk at once. Without an
    /// older list it waits for the answer.
    pub async fn list(
        self: &Arc<Self>,
        app: &AppHandle,
        cache: &Arc<ViewCache>,
    ) -> Result<Vec<MachineSession>> {
        let mut sessions = self.list_cached(app, cache).await?;
        // A presence event may have arrived while the cached read was in flight.
        app.state::<Arc<Fleet>>().apply_presence(&mut sessions);
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
        Ok(sessions)
    }

    async fn list_cached(
        self: &Arc<Self>,
        app: &AppHandle,
        cache: &Arc<ViewCache>,
    ) -> Result<Vec<MachineSession>> {
        let last = self.last().clone();

        if let Some(listed) = &last
            && matches!(listed.age, Age::Listed(at) if at.elapsed() < FRESH)
        {
            return Ok(listed.sessions.clone());
        }

        let Ok(asking) = self.asking.clone().try_lock_owned() else {
            // Another window is asking already; its answer redraws this one.
            if let Some(listed) = last {
                return Ok(listed.sessions);
            }

            // Nothing to show yet: take that answer rather than listing every machine again.
            drop(self.asking.lock().await);

            if let Some(listed) = self.last().clone() {
                return Ok(listed.sessions);
            }

            return Ok(machine_sessions(
                app,
                &app.state::<Arc<Fleet>>(),
                &app.state::<MachineLists>(),
            )
            .await);
        };
        let (tx, rx) = oneshot::channel();
        let me = self.clone();
        let app = app.clone();
        let fleet = app.state::<Arc<Fleet>>().inner().clone();
        let cache = cache.clone();
        tauri::async_runtime::spawn(async move {
            let _asking = asking;
            let sessions = machine_sessions(&app, &fleet, &app.state::<MachineLists>()).await;
            cache_view(&cache, View::AgentSessions, &sessions);
            *me.last() = Some(Listed {
                age: Age::Listed(Instant::now()),
                sessions: sessions.clone(),
            });

            // Home stopped waiting and drew the older list: draw again.
            if tx.send(sessions).is_err()
                && let Err(e) = app.emit(HOME_CHANGED, ())
            {
                log::warn!("telling the windows Home changed: {e}");
            }
        });
        let Some(last) = last else {
            return rx.await.map_err(|_| stopped());
        };

        // The list from the last run draws at once; the answer draws Home again.
        if last.age == Age::Saved {
            return Ok(last.sessions);
        }

        match timeout(WAIT, rx).await {
            Ok(Ok(listed)) => Ok(listed),
            Ok(Err(_)) => {
                log::warn!("{}", stopped());
                Ok(last.sessions)
            }
            // Slow to answer: the older list now, the new one when it comes.
            Err(_) => Ok(last.sessions),
        }
    }
}

fn stopped() -> DesktopError {
    DesktopError::Other(anyhow!(
        "listing the agent sessions stopped before an answer"
    ))
}

#[cfg(test)]
#[path = "../tests/center_home_sessions.rs"]
mod tests;
