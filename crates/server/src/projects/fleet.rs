//! The user's machines as the project tools see them, behind [`Fleet`] so tests can fake them.

use super::{
    Projects,
    model::{StartRequest, WorkerAgent},
    worker::{self, StartError, Started},
};
use crate::{
    account::user::User,
    app::App,
    protocol::node::{
        Capability, SyncthingFolder,
        agent::SessionInfo,
        files::{Reply, Request},
    },
    store::{ProjectRow, TaskRow, WorkerRow},
    tools::machines::command_name,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use beep_projects::{Shell, model::MachinePath};
use beep_projects::{
    issues::{MachineName, Machines},
    leak::PublicText,
};
use iroh::EndpointId;
use std::sync::Arc;
use uuid::Uuid;

/// Every machine's name, for messages; empty when they are unknown, since names only make a
/// message clearer.
pub(crate) async fn names_of(fleet: &(impl Fleet + ?Sized)) -> Vec<MachineName> {
    match fleet.machines().await {
        Ok(machines) => machines
            .into_iter()
            .map(|machine| MachineName {
                id: machine.id,
                name: machine.name,
            })
            .collect(),
        Err(error) => {
            log::info!("projects: the machines' names are unknown: {error:#}");
            Vec::new()
        }
    }
}

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
    /// The folders its Syncthing keeps in step, as it last reported them.
    pub(crate) syncthing: Vec<SyncthingFolder>,
}

#[async_trait]
pub(crate) trait Fleet: Machines {
    /// Every machine of the user's, online or not.
    async fn machines(&self) -> Result<Vec<Machine>>;

    /// Sends `request` to the files `machine` shares.
    async fn files(&self, machine: &EndpointId, request: &Request) -> Result<Reply>;

    /// The server's project state.
    fn projects(&self) -> &Projects;

    /// Cancels and closes `agent`'s session `session` on `machine`.
    async fn close_session(
        &self,
        machine: &EndpointId,
        agent: WorkerAgent,
        session: &str,
    ) -> Result<()>;

    /// Starts a turn of the project chat `chat` on `text`, labelled `label`.
    fn post(&self, chat: Uuid, label: &str, text: String);

    /// Starts `task` as `request` asks.
    async fn start_task(
        &self,
        project: &ProjectRow,
        task: &TaskRow,
        request: &StartRequest,
    ) -> Result<Started, StartError>;

    /// Where the worker's session stands now, following it if nothing does.
    async fn session_info(&self, worker: &WorkerRow) -> Result<SessionInfo>;
}

/// What a project tool hears when it needs a machine: the server reaches none.
pub(crate) const UNREACHABLE: &str =
    "The server reaches no machine: machines are reached through a desktop now.";

/// The machines the account recorded. The server reaches none of them, so each is offline and
/// every call that needs one fails with [`UNREACHABLE`].
pub(crate) struct RecordedFleet {
    pub(crate) app: Arc<App>,
    pub(crate) user: Arc<User>,
}

impl RecordedFleet {
    pub(crate) fn new(app: &Arc<App>, user: &Arc<User>) -> Self {
        Self {
            app: app.clone(),
            user: user.clone(),
        }
    }
}

#[async_trait]
impl Machines for RecordedFleet {
    fn shell(&self, _: &EndpointId) -> Option<Arc<dyn Shell>> {
        None
    }

    async fn names(&self) -> Vec<MachineName> {
        names_of(self).await
    }
}

#[async_trait]
impl Fleet for RecordedFleet {
    async fn machines(&self) -> Result<Vec<Machine>> {
        let rows = self.user.nodes().await?;
        let mut machines = Vec::with_capacity(rows.len());

        for row in rows {
            let Ok(id) = row.endpoint.parse::<EndpointId>() else {
                log::warn!("projects: a stored node key does not parse; skipped");
                continue;
            };
            machines.push(Machine {
                online: false,
                id,
                name: row.name,
                capabilities: row
                    .env
                    .as_ref()
                    .map(|env| env.capabilities())
                    .unwrap_or_default(),
                home: row.env.as_ref().map(|env| env.home.clone()),
                syncthing: row
                    .env
                    .as_ref()
                    .map(|env| env.syncthing.clone())
                    .unwrap_or_default(),
                hostname: row.env.map(|env| env.hostname),
            });
        }

        Ok(machines)
    }

    async fn files(&self, _: &EndpointId, _: &Request) -> Result<Reply> {
        Err(anyhow!(UNREACHABLE))
    }

    fn projects(&self) -> &Projects {
        &self.app.projects
    }

    async fn close_session(&self, _: &EndpointId, _: WorkerAgent, _: &str) -> Result<()> {
        Err(anyhow!(UNREACHABLE))
    }

    fn post(&self, chat: Uuid, label: &str, text: String) {
        self.app
            .projects
            .post(&self.app, self.user.clone(), chat, label, text);
    }

    async fn start_task(
        &self,
        project: &ProjectRow,
        task: &TaskRow,
        request: &StartRequest,
    ) -> Result<Started, StartError> {
        worker::start_task(&self.app, &self.user, project, task, request).await
    }

    async fn session_info(&self, _: &WorkerRow) -> Result<SessionInfo> {
        Err(anyhow!(UNREACHABLE))
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

/// What must not reach an issue or PR: machine names, host names and home folders.
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

/// [`public_text`] plus one project's checkout paths and its machines' shared folders.
pub(crate) async fn project_public_text(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    machines: &[Machine],
) -> Result<PublicText> {
    let mut paths: Vec<String> = user
        .checkouts(project.id)
        .await?
        .into_iter()
        .map(|checkout| checkout.path.to_string())
        .collect();
    for machine in machines.iter().filter(|machine| machine.online) {
        // A machine that cannot say what it shares adds nothing.
        if let Ok(Reply::Roots { roots }) = fleet.files(&machine.id, &Request::Roots {}).await {
            paths.extend(roots);
        }
    }

    Ok(public_text(machines).with_paths(&paths))
}

/// A shell on the home checkout's machine; the error is for the model.
pub(crate) async fn home_shell(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Arc<dyn Shell>, String> {
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
    Ok(shell)
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
            root.is_empty()
                || (root)
                    .parse::<MachinePath>()
                    .is_ok_and(|root| root.contains(path))
        })),
        Reply::Error { message } => Err(anyhow!("the machine said: {message}")),
        _ => Err(anyhow!("the machine did not list its shared folders")),
    }
}
