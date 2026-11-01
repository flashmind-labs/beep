//! The user's machines as the project tools see them. The tools reach the
//! nodes only through [`Fleet`], so tests can stand in for them.

use super::{commands::Shell, model::MachinePath};
use crate::{
    app::App,
    leak::PublicText,
    nodes::{Online, SHELL},
    protocol::node::{
        Capability,
        files::{Reply, Request},
    },
    store::{CheckoutRow, ProjectRow},
    tools::machines::command_name,
    user::User,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use iroh::EndpointId;
use std::sync::Arc;

/// One of the user's machines.
#[derive(Debug, Clone)]
pub(crate) struct Machine {
    pub(crate) id: EndpointId,
    pub(crate) name: String,
    pub(crate) online: bool,
    pub(crate) capabilities: Vec<Capability>,
    /// Its home folder, as it last reported it.
    pub(crate) home: Option<String>,
    /// Its own host name, which is as private as its Beep name.
    pub(crate) hostname: Option<String>,
}

#[async_trait]
pub(crate) trait Fleet: Send + Sync {
    /// Every machine of the user's, online or not.
    async fn machines(&self) -> Result<Vec<Machine>>;

    /// A shell on `machine`, while it is online and offers one.
    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>>;

    /// Sends `request` to the files `machine` shares.
    async fn files(&self, machine: &EndpointId, request: &Request) -> Result<Reply>;
}

/// The nodes the user has connected to the server.
pub(crate) struct NodeFleet {
    pub(crate) app: Arc<App>,
    pub(crate) user: Arc<User>,
}

impl NodeFleet {
    /// The node's connection, only while it is signed in as this user.
    fn online(&self, machine: &EndpointId) -> Option<Online> {
        self.app
            .nodes
            .online(&machine.to_string())
            .filter(|online| online.user == self.user.id())
    }
}

#[async_trait]
impl Fleet for NodeFleet {
    async fn machines(&self) -> Result<Vec<Machine>> {
        let rows = self.user.nodes().await?;
        let mut machines = Vec::with_capacity(rows.len());
        for row in rows {
            let Ok(id) = row.endpoint.parse::<EndpointId>() else {
                log::warn!("projects: a stored node key does not parse; skipped");
                continue;
            };
            machines.push(Machine {
                online: self.online(&id).is_some(),
                id,
                name: row.name,
                capabilities: row
                    .env
                    .as_ref()
                    .map(|env| env.capabilities())
                    .unwrap_or_default(),
                home: row.env.as_ref().map(|env| env.home.clone()),
                hostname: row.env.map(|env| env.hostname),
            });
        }
        Ok(machines)
    }

    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>> {
        self.online(machine)
            .filter(|online| online.offers(SHELL))
            .map(|online| Arc::new(online) as Arc<dyn Shell>)
    }

    async fn files(&self, machine: &EndpointId, request: &Request) -> Result<Reply> {
        let online = self
            .online(machine)
            .ok_or_else(|| anyhow!("the machine is offline"))?;
        online.files(request).await
    }
}

/// The machine `name` names, by name, ignoring case and spaces against dashes.
pub(crate) fn find<'a>(machines: &'a [Machine], name: &str) -> Option<&'a Machine> {
    let wanted = command_name(name);
    machines
        .iter()
        .find(|machine| command_name(&machine.name) == wanted)
}

/// The name of `id` among `machines`, or a placeholder for one removed.
pub(crate) fn name_of(machines: &[Machine], id: &EndpointId) -> String {
    machines
        .iter()
        .find(|machine| &machine.id == id)
        .map_or_else(
            || "a removed machine".to_owned(),
            |machine| command_name(&machine.name),
        )
}

/// What must not reach an issue or a PR: the machines' names and host
/// names, and their home folders.
pub(crate) fn public_text(machines: &[Machine]) -> PublicText {
    let names: Vec<String> = machines
        .iter()
        .flat_map(|machine| {
            [Some(machine.name.clone()), machine.hostname.clone()]
                .into_iter()
                .flatten()
        })
        .filter(|name| !name.trim().is_empty())
        .collect();
    let homes: Vec<String> = machines
        .iter()
        .filter_map(|machine| machine.home.clone())
        .collect();
    PublicText::new(&names, &homes)
}

/// The home checkout and a shell on its machine.
pub(crate) struct HomeShell {
    pub(crate) checkout: CheckoutRow,
    pub(crate) shell: Arc<dyn Shell>,
}

/// A shell on the home checkout's machine; the error is for the model.
pub(crate) async fn home_shell(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<HomeShell, String> {
    let checkouts = user
        .checkouts(project.id)
        .await
        .map_err(|error| format!("Could not read the checkouts: {error:#}"))?;
    let Some(checkout) = checkouts.into_iter().find(|checkout| checkout.home) else {
        return Err(
            "The project has no home checkout yet. Add one with project_setup checkout.".into(),
        );
    };
    let Some(shell) = fleet.shell(&checkout.machine) else {
        return Err("The home checkout's machine is offline or offers no shell.".into());
    };
    Ok(HomeShell { checkout, shell })
}

/// Whether `path` is in one of the folders the machine shares.
pub(crate) async fn shared(
    fleet: &dyn Fleet,
    machine: &EndpointId,
    path: &MachinePath,
) -> Result<bool> {
    match fleet.files(machine, &Request::Roots {}).await? {
        Reply::Roots { roots } => Ok(roots.iter().any(|root| {
            let root = root.trim_end_matches('/');
            // A node sharing `/` shares everything.
            root.is_empty() || MachinePath::parse(root).is_ok_and(|root| root.contains(path))
        })),
        Reply::Error { message } => Err(anyhow!("the machine said: {message}")),
        _ => Err(anyhow!("the machine did not list its shared folders")),
    }
}
