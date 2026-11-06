//! "Let Beep use this Mac": while on, the app is a node like `beep-node`
//! on a server, signed in with this Mac's own key, offering a shell,
//! desktop control and the coding agents installed here, with the sessions
//! they ran. On for a fresh install.

use crate::{beep::Beep, machines::desktop, platform::logfile};
use beep_node::{
    agents::{mcp, skills::Skills},
    link::{self, Offers, reconnect::Backoff},
    machine::{provision::Provision, service::logs::Logs},
};
use beep_proto::node::desktop::{Permission, Permissions};
use serde::Serialize;
use std::{
    future::pending,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tauri::async_runtime::{self, JoinHandle};
use tokio::time::sleep;

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
    home: Arc<NodeHome>,
}

/// The node's own folder and what it keeps there.
struct NodeHome {
    /// The node's folder; its MCP socket goes here, or in a short per-user
    /// folder when this path is too long for one.
    dir: PathBuf,
    /// gh, teacli and the skills, and the credentials the server sends
    /// while the link is up.
    provision: Arc<Provision>,
    /// What this node serves, to the server and to the mesh alike: the same
    /// as `beep-node` serves, with agents found on the login shell's PATH
    /// (the app adopts it at start), plus desktop control.
    offers: Offers,
}

impl Machine {
    /// The machine whose node keeps its tools, teacli config and sockets
    /// in `dir`. `has_server` says whether the app has a server, which holds
    /// the memory tools; without one agents are not told of them.
    pub fn new(dir: &Path, has_server: bool) -> Self {
        let provision = Provision::new(dir);
        let mut offers = Offers::local(&[], provision.clone(), has_server);
        offers.extra.push(Arc::new(desktop::Desktop));
        // The user's coding agent skills reach the mesh's other machines.
        if let Some(home) = std::env::var_os("HOME") {
            offers.skills = Skills::under(home.into());
        }
        // The embedded node logs to the app's own file.
        if let Some(log) = logfile::current() {
            offers.extra.push(Arc::new(Logs::file(log)));
        }
        Self {
            task: Mutex::default(),
            link: Arc::default(),
            name: crate::beep::identity::computer_name(),
            home: Arc::new(NodeHome {
                dir: dir.to_path_buf(),
                provision,
                offers,
            }),
        }
    }

    /// Installs gh, teacli and the agent skills, and serves the agents' Beep
    /// MCP server, for the run of the app: agents on this Mac get the
    /// machine tools with or without a server. Sessions run this app's
    /// executable as the server's command (`mcp_command`), which pipes to
    /// the socket the node bound, named in the session's environment.
    pub fn start(&self) {
        let home = self.home.clone();
        async_runtime::spawn(async move {
            let _provision = home.provision.start();
            if let Err(e) = std::fs::create_dir_all(&home.dir) {
                log::warn!("not serving the Beep MCP server: {e}");
            }
            let _mcp = mcp::start(&home.dir, &home.offers);
            // Both stop when this task does, with the app.
            pending::<()>().await;
        });
    }

    /// What this node serves.
    pub fn offers(&self) -> Offers {
        self.home.offers.clone()
    }

    /// The name this Mac has in the mesh and on the account.
    pub fn name(&self) -> &str {
        &self.name
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
        // Other devices of the mesh get a shell, tasks and the screen only while it is on.
        self.home.offers.lend_to_peers(beep.use_mac());
        let mut task = self.task.lock().unwrap_or_else(PoisonError::into_inner);
        let wanted = beep.use_mac() && beep.linked();
        let running = task
            .as_ref()
            .is_some_and(|task| !task.inner().is_finished());
        if wanted && !running {
            log::info!("joining as a machine: Let Beep use this Mac is on");
            *task = Some(async_runtime::spawn(stay_linked(
                beep.clone(),
                self.link.clone(),
                self.home.clone(),
                self.name.clone(),
            )));
        } else if !wanted {
            if let Some(task) = task.take() {
                task.abort();
                // Off: the Mac keeps no token for the agents.
                let provision = self.home.provision.clone();
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

/// Keeps the node link up, as `beep-node` does but never waiting long:
/// the user is at this Mac. Until aborted.
async fn stay_linked(beep: Arc<Beep>, shared: Arc<Mutex<Link>>, home: Arc<NodeHome>, name: String) {
    let set = |now: Status| *lock(&shared) = Link::new(now);
    let offers = &home.offers;
    let mut backoff = Backoff::eager();
    loop {
        // A Mac removed from the account has nothing to sign in with, and
        // one with no server has nowhere to.
        if !beep.linked() {
            set(Status::Unlinked);
            return;
        }
        // After a failure, it says so until a try works.
        if lock(&shared).status != Status::Failed {
            set(Status::Connecting);
        }
        log::info!("opening this Mac's node link as {name:?}");
        let opened = match beep.server_id().await {
            Ok(server) => link::open(beep.endpoint(), server.into(), &name, offers).await,
            Err(e) => Err(e.into()),
        };
        match opened {
            Ok(linked) => {
                log::info!("this Mac's node link is open");
                set(Status::Connected);
                backoff.reset();
                match linked.serve().await {
                    Ok(()) => log::info!("the server closed this Mac's node link"),
                    Err(e) => log::warn!("node link lost: {e:#}"),
                }
            }
            Err(e) => {
                let wait = backoff.pause();
                log::info!(
                    "this Mac's node link failed, trying again in {:.1}s: {e:#}",
                    wait.as_secs_f32()
                );
                *lock(&shared) = Link {
                    status: Status::Failed,
                    error: Some(format!("{e:#}")),
                };
                sleep(wait).await;
                continue;
            }
        }
        set(Status::Connecting);
        sleep(backoff.pause()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mac_with_no_server_offers_no_memory_tools() {
        let dir = std::env::temp_dir().join(format!("beep-this-mac-{}", std::process::id()));
        assert!(!Machine::new(&dir, false).offers().has_server());
        assert!(Machine::new(&dir, true).offers().has_server());
    }
}
