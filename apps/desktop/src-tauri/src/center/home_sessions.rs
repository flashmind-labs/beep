//! The machines' agent sessions for Home. The server asks every machine for
//! them, so one slow machine would hold all of Home back: Home waits a
//! moment, then draws with the last list, and the answer that comes later
//! has it draw again.

use crate::{
    agents::sessions::machine_sessions,
    beep::{AsyncMutex, Beep},
    center::cache_view,
    center::view_cache::{View, ViewCache},
    error::{DesktopError, Result},
    fleet::Fleet,
    machines::list::Outside,
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
    /// Held while the server is asked, so only one ask runs at a time.
    asking: Arc<AsyncMutex<()>>,
}

#[derive(Clone)]
struct Listed {
    /// When the server listed them; `None` for a list read from disk.
    at: Option<Instant>,
    sessions: Vec<MachineSession>,
}

impl AgentSessions {
    /// Starts from the list saved in `cache`, if it holds one.
    pub fn new(cache: &ViewCache) -> Self {
        let last = match cache
            .get(View::AgentSessions)
            .map(serde_json::from_value::<Vec<MachineSession>>)
        {
            Some(Ok(sessions)) => Some(Listed { at: None, sessions }),
            Some(Err(e)) => {
                // Only a head start: the server's list replaces it.
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
        if let Some(listed) = self.last().as_mut() {
            listed.at = None;
        }
    }

    /// The sessions, as fresh as `WAIT` allows. Without an older list it
    /// waits for the server's answer.
    pub async fn list(
        self: &Arc<Self>,
        app: &AppHandle,
        beep: &Arc<Beep>,
        cache: &Arc<ViewCache>,
    ) -> Result<Vec<MachineSession>> {
        let mut sessions = self.list_cached(app, beep, cache).await?;
        // A presence event may have arrived while the cached read was in flight.
        app.state::<Arc<Fleet>>().apply_presence(&mut sessions);
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
        Ok(sessions)
    }

    async fn list_cached(
        self: &Arc<Self>,
        app: &AppHandle,
        beep: &Arc<Beep>,
        cache: &Arc<ViewCache>,
    ) -> Result<Vec<MachineSession>> {
        let last = self.last().clone();

        if let Some(listed) = &last
            && listed.at.is_some_and(|at| at.elapsed() < FRESH)
        {
            return Ok(listed.sessions.clone());
        }

        let Ok(asking) = self.asking.clone().try_lock_owned() else {
            // Another window is asking already; its answer redraws this one.
            return match last {
                Some(listed) => Ok(listed.sessions),
                None => Ok(machine_sessions(
                    app,
                    beep,
                    &app.state::<Arc<Fleet>>(),
                    &app.state::<Outside>(),
                )
                .await),
            };
        };
        let (tx, rx) = oneshot::channel();
        let me = self.clone();
        let app = app.clone();
        let beep = beep.clone();
        let fleet = app.state::<Arc<Fleet>>().inner().clone();
        let cache = cache.clone();
        tauri::async_runtime::spawn(async move {
            let _asking = asking;
            let listed: Result<Vec<MachineSession>> =
                Ok(machine_sessions(&app, &beep, &fleet, &app.state::<Outside>()).await);
            let answered = listed.is_ok();

            if let Ok(sessions) = &listed {
                cache_view(&cache, View::AgentSessions, sessions);
                *me.last() = Some(Listed {
                    at: Some(Instant::now()),
                    sessions: sessions.clone(),
                });
            }
            // Home stopped waiting and drew the older list: draw again.
            if tx.send(listed).is_err()
                && answered
                && let Err(e) = app.emit(HOME_CHANGED, ())
            {
                log::warn!("telling the windows Home changed: {e}");
            }
        });
        let Some(last) = last else {
            return rx.await.map_err(|_| stopped())?;
        };

        match timeout(WAIT, rx).await {
            Ok(Ok(listed)) => listed,
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
