//! What keeps the tray current: the waiting list as the mirror's rows change, and the menu when an
//! update has been downloaded.

use super::{
    refresh,
    waiting::{WaitingList, entries, no_longer_waiting},
};
#[cfg(desktop)]
use crate::platform::update;
use crate::{
    agents::overlay::overlaid,
    fleet::{Fleet, home_signal::HomeSignal, session_mirror::SessionMirror},
};
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Manager};
use tokio::{select, time::sleep};

/// The least time between two reads of the mirror, so a burst of changes makes one.
const GAP: Duration = Duration::from_secs(2);

/// Keeps the list current while Beep runs: it follows the mirror's rows and the signal Home
/// follows, and reads the rows as they stand. Alerts and pushes reach the rows, so nothing asks
/// the machines.
pub(crate) async fn follow_waiting(app: AppHandle) {
    let list = app.state::<WaitingList>();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let mirror = app.state::<Arc<SessionMirror>>();
    let mut rows = mirror.watch();
    let mut pokes = app.state::<Arc<HomeSignal>>().follow();

    loop {
        let sessions = overlaid(&app, &fleet, &mirror.current());
        let entries = entries(crate::home::needs_you(sessions.iter()));
        let gone: Vec<String> = list
            .entries()
            .into_iter()
            .map(|e| e.id)
            .filter(|id| !entries.iter().any(|e| &e.id == id))
            .collect();

        if list.set(entries) {
            refresh(&app);
            no_longer_waiting(&app, &gone);
        }

        sleep(GAP).await;
        select! {
            changed = rows.changed() => if changed.is_err() { return },
            changed = pokes.changed() => if changed.is_err() { return },
        }
    }
}

#[cfg(desktop)]
/// Draws the menu again when an update has been downloaded.
pub(crate) async fn follow_update(app: AppHandle) {
    let mut ready = update::changes(&app);

    while ready.changed().await.is_ok() {
        refresh(&app);
    }
}
