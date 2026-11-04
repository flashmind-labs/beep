//! What the shared import code reads from the server: the user's machines, credentials and
//! hosts, and what happens around an import.

use crate::{
    account::{
        credentials::{host_client, is_public},
        user::User,
    },
    app::App,
    nodes::{Online, SHELL},
    protocol::node::agent::{SESSIONS_PROTOCOL, SessionFolder},
};
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::{
    Shell,
    credentials::Opened,
    imports::{
        flow::{Effects, Env},
        hosts::Hosts,
        machine::{FoundFolders, Machine},
    },
    model::ForgejoUrl,
};
use beep_proto::{CheckoutFailure, ImportedProject};
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// The server's calls to git hosts: one client, bounded, refusing hosts on private networks.
struct AppHosts {
    app: Arc<App>,
}

#[async_trait]
impl Hosts for AppHosts {
    fn client(&self) -> Result<Client> {
        host_client()
    }

    fn calls(&self) -> &Arc<Semaphore> {
        self.app.git_items.calls()
    }

    async fn is_public(&self, url: &ForgejoUrl) -> bool {
        is_public(url).await
    }
}

/// A node's folders its coding agents worked in.
struct OnlineSessions(Online);

#[async_trait]
impl FoundFolders for OnlineSessions {
    async fn found(&self, limit: u32) -> Result<Vec<SessionFolder>> {
        self.0.found_folders(limit).await
    }
}

/// After an import: the other machines are looked through, and a new project's checkouts sync.
struct ServerEffects {
    app: Arc<App>,
    user: Arc<User>,
}

#[async_trait]
impl Effects for ServerEffects {
    async fn imported(&self, machine: &str) {
        let others: Vec<String> = match self.user.nodes().await {
            Ok(nodes) => nodes
                .into_iter()
                .map(|node| node.endpoint)
                .filter(|endpoint| endpoint != machine)
                .collect(),
            Err(e) => {
                log::warn!("imports: the machines are unknown, none looked through: {e:#}");
                return;
            }
        };
        self.app.checkouts.check_all(&self.app, &self.user, &others);
    }

    async fn join_sync(
        &self,
        imported: &[ImportedProject],
        failed: &mut Vec<CheckoutFailure>,
    ) -> Result<()> {
        super::sync::join_sync(&self.app, &self.user, imported, failed).await
    }
}

/// Everything an import reads, loaded once.
pub(super) struct Context {
    pub(super) credentials: Vec<Opened>,
    pub(super) machines: Vec<Machine>,
    hosts: AppHosts,
    effects: ServerEffects,
}

impl Context {
    pub(super) async fn load(app: &Arc<App>, user: &Arc<User>) -> Result<Context> {
        let credentials = user.git_credentials(&app.credential_key).await?;
        let mut machines = Vec::new();
        for row in user.nodes().await? {
            let online = app
                .nodes
                .online(&row.endpoint)
                .filter(|online| online.user == user.id());
            let shell = online
                .clone()
                .filter(|online| online.offers(SHELL))
                .map(|online| Arc::new(online) as Arc<dyn Shell>);
            let sessions = online
                .clone()
                .filter(|online| online.offers(SESSIONS_PROTOCOL))
                .map(|online| Arc::new(OnlineSessions(online)) as Arc<dyn FoundFolders>);
            let env = row.env.as_ref();
            machines.push(Machine {
                online: online.is_some(),
                os: env.map(|env| env.os.clone()),
                home: env.map(|env| env.home.clone()),
                workspace: online.and_then(|online| online.workspace),
                shell,
                sessions,
                id: row.endpoint,
                name: row.name,
            });
        }
        Ok(Context {
            credentials,
            machines,
            hosts: AppHosts { app: app.clone() },
            effects: ServerEffects {
                app: app.clone(),
                user: user.clone(),
            },
        })
    }

    pub(super) fn hosts(&self) -> &dyn Hosts {
        &self.hosts
    }

    pub(super) fn env<'a>(&'a self, user: &'a User) -> Env<'a> {
        Env {
            store: user,
            hosts: &self.hosts,
            effects: &self.effects,
            credentials: &self.credentials,
            machines: &self.machines,
        }
    }

    /// The Forgejo servers among the user's credentials.
    pub(super) fn servers(&self) -> Vec<ForgejoUrl> {
        beep_projects::imports::hosts::forgejo_of(&self.credentials)
    }

    /// The machines that are online and run commands.
    pub(super) fn usable(&self) -> Vec<Machine> {
        self.machines
            .iter()
            .filter(|machine| machine.shell.is_some())
            .cloned()
            .collect()
    }
}
