//! The signal that Home must draw again for a change outside the mirror's rows: a start, a
//! delete or move under way, a machine's terminals. Whoever changes such state pokes it; the
//! view layer follows it. It depends on nothing, so any module may poke it.

use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

/// Counts the pokes.
pub(crate) struct HomeSignal(watch::Sender<u64>);

impl HomeSignal {
    pub(crate) fn new() -> Self {
        Self(watch::channel(0).0)
    }

    pub(crate) fn follow(&self) -> watch::Receiver<u64> {
        self.0.subscribe()
    }
}

/// Home must draw again.
pub(crate) fn poke(app: &AppHandle) {
    app.state::<Arc<HomeSignal>>()
        .0
        .send_modify(|pokes| *pokes = pokes.wrapping_add(1));
}
