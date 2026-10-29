//! "Let Beep use this Mac": while on, the app is a node like `beep-node`
//! on a server, signed in with this Mac's own key, offering a shell and
//! desktop control. Off by default.

use crate::{beep::Beep, desktop, state::LinkState};
use beep_node::link::{self, Offers};
use beep_proto::node::desktop::{Permission, Permissions};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::async_runtime::{self, JoinHandle};
use tokio::time::sleep;

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);
/// How long to wait after the server turned this Mac away before asking again.
const REFUSED_RETRY: Duration = Duration::from_secs(30 * 60);

/// Where the link stands, as Settings shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Off,
    Connecting,
    Connected,
    /// The server lets only admins use their computers this way for now.
    NotAllowed,
    /// This Mac is not linked to an account.
    Unlinked,
}

/// What Settings shows under This Mac.
#[derive(Debug, Serialize)]
pub struct View {
    enabled: bool,
    status: Status,
    permissions: Permissions,
}

/// The node link while it runs. Owned by the app; stopping aborts the task,
/// which closes the connection and every session with it.
#[derive(Default)]
pub struct Machine {
    task: Mutex<Option<JoinHandle<()>>>,
    status: Arc<Mutex<Status>>,
}

impl Machine {
    fn status(&self) -> MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn view(&self, beep: &Beep) -> View {
        View {
            enabled: beep.use_mac(),
            status: *self.status(),
            permissions: desktop::permissions(),
        }
    }

    /// Starts or stops the link to match the setting and the link state.
    pub fn sync(&self, beep: &Arc<Beep>) {
        let mut task = self.task.lock().unwrap_or_else(PoisonError::into_inner);
        let wanted = beep.use_mac() && beep.link_state() == LinkState::Linked;
        let running = task
            .as_ref()
            .is_some_and(|task| !task.inner().is_finished());
        if wanted && !running {
            *task = Some(async_runtime::spawn(stay_linked(
                beep.clone(),
                self.status.clone(),
            )));
        } else if !wanted {
            if let Some(task) = task.take() {
                task.abort();
            }
            *self.status() = if beep.use_mac() {
                Status::Unlinked
            } else {
                Status::Off
            };
        }
    }
}

/// Asks macOS for `permission`, opening its page in System Settings.
pub fn allow(permission: Permission) {
    desktop::request(permission);
}

/// Keeps the node link up, waiting longer after each failure, until aborted.
async fn stay_linked(beep: Arc<Beep>, status: Arc<Mutex<Status>>) {
    let set = |now: Status| *status.lock().unwrap_or_else(PoisonError::into_inner) = now;
    let offers = Offers {
        agents: Arc::default(),
        extra: vec![Arc::new(desktop::Desktop)],
    };
    let name = crate::computer_name();
    let mut wait = FIRST_RETRY;
    loop {
        // A Mac removed from the account has nothing to sign in with.
        if beep.link_state() != LinkState::Linked {
            set(Status::Unlinked);
            return;
        }
        set(Status::Connecting);
        match link::open(beep.endpoint(), beep.server().into(), &name, &offers).await {
            Ok(linked) => {
                set(Status::Connected);
                wait = FIRST_RETRY;
                match linked.serve().await {
                    Ok(()) => log::info!("the server closed this Mac's node link"),
                    Err(e) => log::warn!("node link lost: {e:#}"),
                }
            }
            Err(e) if format!("{e:#}").contains("admins") => {
                log::info!("the server does not let this account use its Mac: {e:#}");
                set(Status::NotAllowed);
                sleep(REFUSED_RETRY).await;
                continue;
            }
            Err(e) => log::warn!("node link failed: {e:#}"),
        }
        set(Status::Connecting);
        sleep(wait).await;
        wait = (wait * 2).min(LAST_RETRY);
    }
}
