//! This computer's local node: coding agents, terminals, files and desktop
//! control offered to the other devices while it is lent to the mesh.

use crate::platform::logfile;
use crate::store::lock;
use crate::{
    beep::state::Prefs,
    machines::{ThisMacStatus, ThisMacView, desktop},
};
use beep_node::{
    agents::{Parts, detect, skills::Skills},
    link::{self, Bound, Offers, state::Search},
    machine::{
        provision::{self, Provision},
        service::logs::Logs,
    },
    tools::{self, Node, ProjectTools, Socket},
};
use beep_proto::node::desktop::Permission;
use iroh::{Endpoint, EndpointId};
use std::{
    future::pending,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::async_runtime::{self, JoinHandle};

/// This Mac as a machine of the mesh. Owned by the app.
pub struct Machine {
    /// Reopens what was open when the app last quit, then keeps the note of
    /// what is open; `quit` stops it before the last word.
    keeping: Mutex<Option<JoinHandle<()>>>,
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
    offers: Bound,
    /// The MCP server's socket, bound with the app and served by `start`.
    mcp: Mutex<Option<Socket>>,
}

impl Machine {
    /// The machine whose node, on `endpoint`, keeps its tools, teacli config
    /// and sockets in `dir`. Its offers are made with the node's own values
    /// here, so every copy, the MCP server's and the fleet's, shares them.
    pub fn new(
        endpoint: &Endpoint,
        dir: &Path,
        project_tools: Option<Arc<dyn ProjectTools>>,
    ) -> Self {
        let provision = Provision::new(dir);
        let name = crate::beep::identity::computer_name();
        // The socket's folder exists before the bind.
        let _ = std::fs::create_dir_all(dir);
        // Bound here so sessions start with the server; `start` serves it.
        let mcp = async_runtime::block_on(async { tools::bind(dir) });
        let parts = Parts {
            kinds: detect(&[], Some(dir)),
            launch: mcp.as_ref().map(Socket::launch),
            skills_dir: provision::skills_dir(),
            search: Some(dir.join("search.sqlite")),
            ..Parts::default()
        };
        let mut offers = Offers::local(parts, provision.clone()).bound(endpoint, dir, &name);
        // The assistant's project tools on this Mac reach this app.
        offers.project_tools = project_tools;
        #[cfg(target_os = "macos")]
        offers.extra.push(Arc::new(desktop::Desktop));
        // The user's coding agent skills reach the mesh's other machines.
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            offers.skills = Skills::under(home.into());
        }
        // The embedded node logs to the app's own file.
        if let Some(dir) = logfile::dir() {
            let logs = Logs::in_directory(dir);
            offers.extra.push(Arc::new(logs.clone()));
            offers.log = logs;
        }
        // Searches read the agents' logs ahead on tasks of their own.
        async_runtime::block_on(async { offers.offer_searches(dir, &Search::default()) });
        Self {
            keeping: Mutex::default(),
            name,
            home: Arc::new(NodeHome {
                dir: dir.to_path_buf(),
                provision,
                offers,
                mcp: Mutex::new(mcp),
            }),
        }
    }

    /// Installs gh, teacli and the agent skills, and serves the agents' Beep
    /// MCP server, for the run of the app: agents on this Mac get the
    /// machine tools. Sessions run this app's executable as the server's
    /// command (`mcp_command`), which pipes to the socket the node bound,
    /// named in the session's environment.
    pub fn start(&self) {
        if let Err(e) = std::fs::create_dir_all(&self.home.dir) {
            log::warn!("not serving the Beep MCP server or noting what is open: {e}");
        }

        self.reopen();
        let home = self.home.clone();
        async_runtime::spawn(async move {
            let _provision = home.provision.start();
            let _adapters = home
                .offers
                .agents
                .catalog
                .install_adapters(home.dir.clone(), Vec::new());
            let _upkeep = home.offers.agents.start_upkeep();
            let _events = home.offers.agents.events.forward_system();
            let _mcp = lock(&home.mcp)
                .take()
                .map(|socket| tools::serve(socket, Node::of(&home.offers)));
            // Terminal Claude Code sessions report their state here; the
            // MCP server stops when this task does, with the app. The
            // phones on this Mac's adb go to the desktops that follow them.
            tokio::join!(
                async {
                    if let Err(e) = home.offers.occupancy.listen_hooks(&home.dir).await {
                        log::warn!("not taking Claude Code hooks: {e:#}");
                    }
                },
                async {
                    if let Err(e) = home.offers.occupancy.listen_presence(&home.dir).await {
                        log::warn!("not taking session presence: {e:#}");
                    }
                },
                beep_node::track_phones(),
            );
            pending::<()>().await;
        });
    }

    /// Sessions and terminals open when the app last quit or crashed open
    /// again, and the note of what is open is kept from then on.
    fn reopen(&self) {
        let home = self.home.clone();
        *lock(&self.keeping) = Some(async_runtime::spawn(async move {
            link::reopen_and_keep(&home.dir, &home.offers.agents).await;
        }));
    }

    /// For the app quitting: stops keeping the note, then notes what is
    /// open for the next start. Sessions that end from here on ended with
    /// the app and stay in the note.
    pub fn quit(&self) -> impl Future<Output = ()> + Send + 'static {
        let keeping = lock(&self.keeping).take();
        let home = self.home.clone();
        async move {
            if let Some(keeping) = keeping {
                keeping.abort();
                // Its write in flight lands before this one.
                let _ = keeping.await;
            }

            if let Err(e) = link::note_on_quit(&home.dir, &home.offers.agents) {
                log::warn!("what is open will not open again: {e:#}");
            }
        }
    }

    /// What this node serves.
    pub fn offers(&self) -> Bound {
        self.home.offers.clone()
    }

    /// The name this Mac has in the mesh.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn view(&self, prefs: &Prefs, id: EndpointId) -> ThisMacView {
        let enabled = prefs.use_mac();
        ThisMacView {
            id: id.to_string(),
            name: self.name.clone(),
            enabled,
            status: if enabled {
                ThisMacStatus::Connected
            } else {
                ThisMacStatus::Off
            },
            permissions: desktop::permissions(),
        }
    }

    /// Lends this Mac to the other devices of the mesh, or not, to match the
    /// setting.
    pub fn sync(&self, prefs: &Prefs) {
        self.home.offers.lend_to_peers(prefs.use_mac());
    }
}

/// Asks macOS for `permission`, opening its page in System Settings.
pub fn allow(permission: Permission) {
    desktop::request(permission);
}

#[cfg(test)]
#[path = "../tests/machines_this_mac.rs"]
mod tests;
