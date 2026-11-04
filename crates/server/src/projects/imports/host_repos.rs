//! The user's repositories on their git hosts, read with `gh` and `teacli` on one of their machines,
//! for cloning from the Add a project sheet (`ListHostRepos`).

use super::checks::forgejo_servers;
use crate::{
    account::user::User,
    app::App,
    projects::fleet::{Fleet, NodeFleet},
    protocol::HostRepos,
    store::NodeRow,
};
use anyhow::Result;
use beep_projects::{Shell, imports::host_repos::read_hosts};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long a list is kept when every host answered.
const FRESH: Duration = Duration::from_secs(300);
/// How long it is kept when a host failed, to try that one again sooner.
const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);
/// The most repositories one host lists.
const MAX_REPOS: usize = 100;

/// What one user was last given, before leaving out what they imported since.
struct Cached {
    /// The Forgejo servers it was read for, so a new one rereads.
    servers: Vec<String>,
    at: Instant,
    hosts: Vec<HostRepos>,
}

/// Each user's last read, by user id.
#[derive(Default)]
pub struct HostRepoCache(Mutex<HashMap<String, Cached>>);

/// The user's repositories per host, newest update first, leaving out archived and imported ones.
pub(crate) async fn host_repos(app: &Arc<App>, user: &Arc<User>) -> Result<Vec<HostRepos>> {
    let servers = forgejo_servers(app, user).await?;
    let keys: Vec<String> = servers.iter().map(|url| url.as_str().to_owned()).collect();
    let cached = {
        let cache = app
            .host_repos
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        cache.get(user.id()).and_then(|last| {
            let ttl = if last.hosts.iter().any(|host| host.error.is_some()) {
                FRESH_AFTER_ERROR
            } else {
                FRESH
            };
            (last.servers == keys && last.at.elapsed() < ttl).then(|| last.hosts.clone())
        })
    };
    let mut hosts = match cached {
        Some(hosts) => hosts,
        None => {
            let fleet = NodeFleet::new(app, user);
            let nodes = user.nodes().await?;
            let hosts = read_hosts(
                &shells(&fleet, &nodes, "gh"),
                &shells(&fleet, &nodes, "teacli"),
                &servers,
            )
            .await;
            app.host_repos
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(
                    user.id().to_owned(),
                    Cached {
                        servers: keys,
                        at: Instant::now(),
                        hosts: hosts.clone(),
                    },
                );
            hosts
        }
    };
    let imported: Vec<String> = user
        .imports()
        .await?
        .into_iter()
        .map(|row| row.project.to_ascii_lowercase())
        .collect();
    for host in &mut hosts {
        host.repos
            .retain(|repo| !imported.contains(&repo.name.to_ascii_lowercase()));
        host.repos.truncate(MAX_REPOS);
    }
    Ok(hosts)
}

/// The online non-Windows machines with `tool` and a shell, those signed in to it first.
fn shells(fleet: &dyn Fleet, nodes: &[NodeRow], tool: &str) -> Vec<Arc<dyn Shell>> {
    let mut found: Vec<(bool, Arc<dyn Shell>)> = nodes
        .iter()
        .filter_map(|node| {
            let env = node.env.as_ref()?;
            if env.os == "windows" || !env.tools.iter().any(|known| known.name == tool) {
                return None;
            }
            let id: EndpointId = node.endpoint.parse().ok()?;
            let signed_in = env
                .logins
                .iter()
                .any(|login| login.tool == tool && login.signed_in);
            Some((signed_in, fleet.shell(&id)?))
        })
        .collect();
    found.sort_by_key(|(signed_in, _)| !*signed_in);
    found.into_iter().map(|(_, shell)| shell).collect()
}
