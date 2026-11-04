//! The user's repositories on their git hosts, read with `gh` and `teacli` on one of their machines,
//! for cloning from the Add a project sheet (`ListHostRepos`).

use super::checks::forgejo_servers;
use crate::{
    account::credentials::ForgejoUrl,
    account::user::User,
    app::App,
    projects::{
        Shell,
        fleet::{Fleet, NodeFleet},
        issues::run_rendered,
        quote,
    },
    protocol::{GitHostKind, HostRepo, HostRepos},
    store::NodeRow,
};
use anyhow::{Result, anyhow};
use chrono::{DateTime, FixedOffset};
use futures::future::join_all;
use iroh::EndpointId;
use serde::Deserialize;
use serde_json::Value;
use std::{
    cmp::Reverse,
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant, SystemTime},
};

/// How long a list is kept when every host answered.
const FRESH: Duration = Duration::from_secs(300);
/// How long it is kept when a host failed, to try that one again sooner.
const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);
/// The most repositories one host lists.
const MAX_REPOS: usize = 100;
const GITHUB: &str = "github.com";

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
            let hosts =
                read_hosts(&NodeFleet::new(app, user), &user.nodes().await?, &servers).await;
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

/// Reads GitHub, when a machine has `gh`, and every Forgejo server in `servers`, all at once.
async fn read_hosts(
    fleet: &dyn Fleet,
    nodes: &[NodeRow],
    servers: &[ForgejoUrl],
) -> Vec<HostRepos> {
    let gh = shells(fleet, nodes, "gh");
    let tea = shells(fleet, nodes, "teacli");
    let github = async {
        if gh.is_empty() {
            return None;
        }
        let command = "gh repo list --json nameWithOwner,updatedAt,isArchived --limit 100";
        let read = read_any(&gh, command, parse_gh).await;
        Some(listed(
            GITHUB.to_owned(),
            GitHostKind::GitHub {},
            "https://github.com",
            read,
        ))
    };
    let forgejo = servers.iter().map(|server| {
        let tea = &tea;
        async move {
            // Whole repositories are long: keep each one's top-level name, update and
            // archived flag, so a long list fits the shell's output.
            let command = format!(
                "out=$(teacli repo list --host {} --format json --all 2>&1) || {{ printf '%s\\n' \"$out\"; exit 1; }}; \
                 printf '%s\\n' \"$out\" | grep -E {} || true",
                quote(server.as_str()),
                quote(r#"^    "(full_name|updated_at|archived)":"#),
            );
            let read = if tea.is_empty() {
                Err(anyhow!("No online machine has teacli."))
            } else {
                read_any(tea, &command, parse_teacli).await
            };
            listed(server.host(), GitHostKind::Forgejo {}, server.as_str(), read)
        }
    });
    let (github, forgejo) = tokio::join!(github, join_all(forgejo));
    github.into_iter().chain(forgejo).collect()
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

/// Runs `command` on each shell in turn until one answers, so one broken CLI does not hide the list.
async fn read_any(
    shells: &[Arc<dyn Shell>],
    command: &str,
    parse: fn(&str) -> Result<Vec<Listed>>,
) -> Result<Vec<Listed>> {
    let mut last = anyhow!("no machine to read it on");
    for shell in shells {
        match run_rendered(shell.as_ref(), command)
            .await
            .and_then(|out| parse(&out))
        {
            Ok(repos) => return Ok(repos),
            Err(e) => {
                log::info!("host repos: reading failed on one machine: {e:#}");
                last = e;
            }
        }
    }
    Err(last)
}

/// One host's answer, newest first, with web pages under `base`.
fn listed(host: String, kind: GitHostKind, base: &str, read: Result<Vec<Listed>>) -> HostRepos {
    match read {
        Ok(mut repos) => {
            repos.sort_by_key(|repo| Reverse(repo.updated_at));
            HostRepos {
                host,
                kind,
                repos: repos
                    .into_iter()
                    .map(|repo| HostRepo {
                        url: format!("{base}/{}", repo.name),
                        name: repo.name,
                        updated_at: repo.updated_at,
                    })
                    .collect(),
                error: None,
            }
        }
        Err(e) => HostRepos {
            host,
            kind,
            repos: Vec::new(),
            error: Some(format!("{e:#}")),
        },
    }
}

/// A repository as a CLI listed it, archived ones already left out.
#[derive(Debug, PartialEq)]
struct Listed {
    name: String,
    updated_at: SystemTime,
}

/// What `gh repo list --json nameWithOwner,updatedAt,isArchived` prints.
fn parse_gh(out: &str) -> Result<Vec<Listed>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GhRepo {
        name_with_owner: String,
        updated_at: DateTime<FixedOffset>,
        is_archived: bool,
    }
    let repos: Vec<GhRepo> = serde_json::from_str(out)?;
    Ok(repos
        .into_iter()
        .filter(|repo| !repo.is_archived)
        .map(|repo| Listed {
            name: repo.name_with_owner,
            updated_at: repo.updated_at.into(),
        })
        .collect())
}

/// The `"full_name"`, `"archived"` and `"updated_at"` lines `teacli repo list --format json` printed
/// for each repository, in that order; a repository missing one is left out.
fn parse_teacli(out: &str) -> Result<Vec<Listed>> {
    struct Partial {
        name: String,
        archived: bool,
        updated_at: Option<SystemTime>,
    }
    let mut all: Vec<Partial> = Vec::new();
    for line in out.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value: Value = serde_json::from_str(value.trim().trim_end_matches(','))?;
        match (key.trim_matches('"'), all.last_mut()) {
            ("full_name", _) => all.push(Partial {
                name: value.as_str().unwrap_or_default().to_owned(),
                archived: false,
                updated_at: None,
            }),
            ("archived", Some(repo)) => repo.archived = value.as_bool().unwrap_or_default(),
            ("updated_at", Some(repo)) => {
                repo.updated_at = value
                    .as_str()
                    .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                    .map(SystemTime::from);
            }
            _ => {}
        }
    }
    Ok(all
        .into_iter()
        .filter(|repo| !repo.archived && !repo.name.is_empty())
        .filter_map(|repo| {
            Some(Listed {
                updated_at: repo.updated_at?,
                name: repo.name,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn gh_and_teacli_lists_parse_without_archived_repos() {
        let gh = r#"[{"isArchived":false,"nameWithOwner":"acme/app","updatedAt":"2026-01-01T00:00:10Z"},
            {"isArchived":true,"nameWithOwner":"acme/old","updatedAt":"2026-01-01T00:00:00Z"}]"#;
        assert_eq!(
            parse_gh(gh).expect("gh output parses"),
            vec![Listed {
                name: "acme/app".into(),
                updated_at: at(1_767_225_610)
            }]
        );
        let tea = r#"    "full_name": "team/infra",
    "archived": false,
    "updated_at": "2026-01-01T01:00:10+01:00",
    "full_name": "team/old",
    "archived": true,
    "updated_at": "2026-01-01T00:00:00Z","#;
        assert_eq!(
            parse_teacli(tea).expect("teacli output parses"),
            vec![Listed {
                name: "team/infra".into(),
                updated_at: at(1_767_225_610)
            }]
        );
        assert!(parse_teacli("").expect("nothing parses").is_empty());
    }
}
