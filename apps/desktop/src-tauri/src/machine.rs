//! "Let Beep use this Mac": while on, the app is a node like `beep-node`
//! on a server, signed in with this Mac's own key, offering a shell and
//! desktop control. On for a fresh install.

use crate::{beep::Beep, desktop, state::LinkState};
use beep_node::{
    link::{self, Offers},
    provision::Provision,
};
use beep_proto::node::desktop::{Permission, Permissions};
use serde::Serialize;
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::async_runtime::{self, JoinHandle};
use tokio::time::sleep;

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(60);

/// Where the link stands, as Settings shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Off,
    Connecting,
    Connected,
    /// The last try failed; `View::error` says why.
    /// The link tries again on its own.
    Failed,
    /// This Mac is not linked to an account.
    Unlinked,
}

/// Where the link stands and, after a failed try, why it failed.
#[derive(Debug, Clone, Default)]
struct Link {
    status: Status,
    error: Option<String>,
}

impl Link {
    fn new(status: Status) -> Self {
        Self {
            status,
            error: None,
        }
    }
}

/// What Settings shows under This Mac, and what the command center needs
/// to tell this Mac apart in the machine list.
#[derive(Debug, Serialize)]
pub struct View {
    /// This Mac's key, the `Machine::id` the server lists it under.
    id: String,
    /// The name this Mac links with.
    name: String,
    enabled: bool,
    status: Status,
    /// Why the last try failed, while `status` is `Failed`.
    error: Option<String>,
    permissions: Permissions,
}

/// The node link while it runs. Owned by the app; stopping aborts the task,
/// which closes the connection and every session with it.
pub struct Machine {
    task: Mutex<Option<JoinHandle<()>>>,
    link: Arc<Mutex<Link>>,
    /// The computer name, read once.
    name: String,
    /// gh, teacli and the skills, in the node's own folder, and the
    /// credentials the server sends while the link is up.
    provision: Arc<Provision>,
}

impl Machine {
    /// The machine whose node keeps its tools and teacli config in `dir`.
    pub fn new(dir: &Path) -> Self {
        Self {
            task: Mutex::default(),
            link: Arc::default(),
            name: crate::computer_name(),
            provision: Provision::new(dir),
        }
    }

    fn link(&self) -> MutexGuard<'_, Link> {
        lock(&self.link)
    }

    pub fn view(&self, beep: &Beep) -> View {
        let link = self.link().clone();
        View {
            id: beep.id().to_string(),
            name: self.name.clone(),
            enabled: beep.use_mac(),
            status: link.status,
            error: link.error,
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
            log::info!("joining as a machine: Let Beep use this Mac is on");
            *task = Some(async_runtime::spawn(stay_linked(
                beep.clone(),
                self.link.clone(),
                self.provision.clone(),
                self.name.clone(),
            )));
        } else if !wanted {
            if let Some(task) = task.take() {
                task.abort();
                // Off: the Mac keeps no token for the agents.
                let provision = self.provision.clone();
                async_runtime::spawn(async move { provision.forget().await });
            }
            let now = if beep.use_mac() {
                Status::Unlinked
            } else {
                Status::Off
            };
            let mut link = self.link();
            if link.status != now {
                match now {
                    Status::Off => log::info!("not a machine: Let Beep use this Mac is off"),
                    _ => log::info!("not a machine: this Mac is not linked to an account"),
                }
            }
            *link = Link::new(now);
        }
    }
}

/// Asks macOS for `permission`, opening its page in System Settings.
pub fn allow(permission: Permission) {
    desktop::request(permission);
}

fn lock(link: &Mutex<Link>) -> MutexGuard<'_, Link> {
    link.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Keeps the node link up, waiting longer after each failure, until aborted.
async fn stay_linked(
    beep: Arc<Beep>,
    shared: Arc<Mutex<Link>>,
    provision: Arc<Provision>,
    name: String,
) {
    let set = |now: Status| *lock(&shared) = Link::new(now);
    // Installs gh, teacli and the agent skills, and takes the credentials
    // the server sends; the work stops with this task.
    let _provision = provision.start();
    let offers = Offers {
        agents: Arc::default(),
        extra: vec![Arc::new(desktop::Desktop)],
        provision: Some(provision),
    };
    let mut wait = FIRST_RETRY;
    loop {
        // A Mac removed from the account has nothing to sign in with.
        if beep.link_state() != LinkState::Linked {
            set(Status::Unlinked);
            return;
        }
        // After a failure, it says so until a try works.
        if lock(&shared).status != Status::Failed {
            set(Status::Connecting);
        }
        log::info!("opening this Mac's node link as {name:?}");
        match link::open(beep.endpoint(), beep.server().into(), &name, &offers).await {
            Ok(linked) => {
                log::info!("this Mac's node link is open");
                set(Status::Connected);
                wait = FIRST_RETRY;
                match linked.serve().await {
                    Ok(()) => log::info!("the server closed this Mac's node link"),
                    Err(e) => log::warn!("node link lost: {e:#}"),
                }
            }
            Err(e) => {
                log::info!(
                    "this Mac's node link failed, trying again in {}s: {e:#}",
                    wait.as_secs()
                );
                *lock(&shared) = Link {
                    status: Status::Failed,
                    error: Some(format!("{e:#}")),
                };
                sleep(wait).await;
                wait = (wait * 2).min(LAST_RETRY);
                continue;
            }
        }
        set(Status::Connecting);
        sleep(wait).await;
        wait = (wait * 2).min(LAST_RETRY);
    }
}
