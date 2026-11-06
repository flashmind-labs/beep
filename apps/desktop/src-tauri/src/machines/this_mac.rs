//! "Let Beep use this Mac": while on, the other devices of the mesh may use
//! this Mac as they use a `beep-node`: a shell, desktop control and the
//! coding agents installed here, with the sessions they ran. The node in this
//! process serves the mesh either way; the setting only lends it. On for a
//! fresh install.

use crate::{beep::Beep, machines::desktop, platform::logfile};
use beep_node::{
    agents::{mcp, skills::Skills},
    link::{Offers, listen_hooks, state::Search},
    machine::{provision::Provision, service::logs::Logs},
};
use beep_proto::node::desktop::{Permission, Permissions};
use serde::Serialize;
use std::{
    future::pending,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::async_runtime;

/// Whether this Mac is lent to the mesh, as Settings shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Off,
    Connected,
}

/// What Settings shows under This Mac, and what the command center needs
/// to tell this Mac apart in the machine list.
#[derive(Debug, Serialize)]
pub struct View {
    /// This Mac's key, its id in the mesh.
    id: String,
    /// The name this Mac has in the mesh.
    name: String,
    enabled: bool,
    status: Status,
    permissions: Permissions,
}

/// This Mac as a machine of the mesh. Owned by the app.
pub struct Machine {
    /// The computer name, read once.
    name: String,
    home: Arc<NodeHome>,
}

/// The node's own folder and what it keeps there.
struct NodeHome {
    /// The node's folder; its MCP socket goes here, or in a short per-user
    /// folder when this path is too long for one.
    dir: PathBuf,
    /// gh, teacli and the skills, and the credentials the mesh holds.
    provision: Arc<Provision>,
    /// What this node serves to the mesh: the same as `beep-node` serves,
    /// with agents found on the login shell's PATH (the app adopts it at
    /// start), plus desktop control.
    offers: Offers,
}

impl Machine {
    /// The machine whose node keeps its tools, teacli config and sockets
    /// in `dir`.
    pub fn new(dir: &Path) -> Self {
        let provision = Provision::new(dir);
        let mut offers = Offers::local(&[], provision.clone());
        offers.extra.push(Arc::new(desktop::Desktop));
        // The user's coding agent skills reach the mesh's other machines.
        if let Some(home) = std::env::var_os("HOME") {
            offers.skills = Skills::under(home.into());
        }
        // The embedded node logs to the app's own file.
        if let Some(log) = logfile::current() {
            offers.extra.push(Arc::new(Logs::file(log)));
        }
        offers.log = Logs::files(logfile::kept());
        // Searches read the agents' logs ahead on tasks of their own.
        async_runtime::block_on(async { offers.offer_searches(dir, &Search::default()) });
        Self {
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
    /// machine tools. Sessions run this app's executable as the server's
    /// command (`mcp_command`), which pipes to the socket the node bound,
    /// named in the session's environment.
    pub fn start(&self) {
        let home = self.home.clone();
        async_runtime::spawn(async move {
            let _provision = home.provision.start();
            if let Err(e) = std::fs::create_dir_all(&home.dir) {
                log::warn!("not serving the Beep MCP server: {e}");
            }
            let _mcp = mcp::start(&home.dir, &home.offers);
            // Terminal Claude Code sessions report their state here; the
            // MCP server stops when this task does, with the app.
            listen_hooks(&home.offers.agents, &home.dir).await;
            pending::<()>().await;
        });
    }

    /// What this node serves.
    pub fn offers(&self) -> Offers {
        self.home.offers.clone()
    }

    /// The name this Mac has in the mesh.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn view(&self, beep: &Beep) -> View {
        let enabled = beep.use_mac();
        View {
            id: beep.id().to_string(),
            name: self.name.clone(),
            enabled,
            status: if enabled {
                Status::Connected
            } else {
                Status::Off
            },
            permissions: desktop::permissions(),
        }
    }

    /// Lends this Mac to the other devices of the mesh, or not, to match the
    /// setting.
    pub fn sync(&self, beep: &Beep) {
        self.home.offers.lend_to_peers(beep.use_mac());
    }
}

/// Asks macOS for `permission`, opening its page in System Settings.
pub fn allow(permission: Permission) {
    desktop::request(permission);
}
