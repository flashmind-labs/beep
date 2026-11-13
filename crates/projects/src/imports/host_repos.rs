//! The user's repositories on their git hosts, read with `gh` and `teacli` on their machines, for
//! cloning from the Add a project sheet.

use crate::{
    Shell,
    git_items::github::HOST as GITHUB,
    issues::{read_first, run_rendered},
    model::ForgejoUrl,
    quote,
};
use anyhow::{Result, anyhow};
use beep_proto::{GitHostKind, HostRepo, HostRepos};
use chrono::DateTime;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::Value;
use std::{cmp::Reverse, sync::Arc, time::SystemTime};

/// The user's GitHub repositories and their organizations', one `name<TAB>updated` line each,
/// archived ones left out. Single quotes only: on Windows it runs in Git's sh all the same.
const GH_REPOS: &str = "gh api --paginate \
     'user/repos?affiliation=owner,collaborator,organization_member&sort=updated&per_page=100' \
     --jq '.[] | select(.archived | not) | [.full_name, .updated_at] | @tsv'";
/// The most GitHub repositories listed, newest first.
const GH_MOST: usize = 200;

/// Reads GitHub, when a machine has `gh`, and every Forgejo server in `servers`, all at once.
pub async fn read_hosts(
    gh: &[Arc<dyn Shell>],
    tea: &[Arc<dyn Shell>],
    servers: &[ForgejoUrl],
) -> Vec<HostRepos> {
    let github = async {
        if gh.is_empty() {
            return None;
        }

        let runs: Vec<Run> = gh
            .iter()
            .map(|shell| Run {
                shell: shell.clone(),
                command: GH_REPOS.to_owned(),
            })
            .collect();
        let read = read_any(&runs, parse_gh).await;
        Some(listed(
            GITHUB.to_owned(),
            GitHostKind::GitHub {},
            "https://github.com",
            read,
        ))
    };
    let forgejo = async {
        // teacli takes a server by the name its config gives it, so each machine's config is
        // read once, for every server.
        let configs = join_all(tea.iter().map(|shell| tea_servers(shell.as_ref()))).await;
        let machines: Vec<TeaMachine> = tea
            .iter()
            .zip(configs)
            .map(|(shell, servers)| TeaMachine {
                shell: shell.clone(),
                servers,
            })
            .collect();
        join_all(servers.iter().map(|server| {
            let machines = &machines;
            async move {
                let read = read_forgejo(machines, server).await;
                listed(
                    server.host(),
                    GitHostKind::Forgejo {},
                    server.as_ref(),
                    read,
                )
            }
        }))
        .await
    };
    let (github, forgejo) = tokio::join!(github, forgejo);
    github.into_iter().chain(forgejo).collect()
}

/// Prints the servers teacli is set up for, as `[{"name", "url", ...}]`.
const TEA_SERVERS: &str = "teacli auth config list --format json";

/// A machine with teacli and the servers its config names.
struct TeaMachine {
    shell: Arc<dyn Shell>,
    servers: Vec<TeaServer>,
}

/// One server in a teacli config.
#[derive(Debug, Deserialize)]
struct TeaServer {
    name: String,
    url: String,
}

impl TeaServer {
    /// Whether this is `server`, by URL, with case and a trailing slash ignored.
    fn is(&self, server: &ForgejoUrl) -> bool {
        match self.url.parse::<ForgejoUrl>() {
            Ok(url) => url.as_ref() == server.as_ref(),
            Err(_) => self.url.trim().trim_end_matches('/') == server.as_ref(),
        }
    }
}

/// The servers `shell`'s teacli is set up for; none when it cannot say, which leaves the
/// machine out of every Forgejo list.
async fn tea_servers(shell: &dyn Shell) -> Vec<TeaServer> {
    let read = async {
        let out = run_rendered(shell, TEA_SERVERS).await?;
        Ok::<_, anyhow::Error>(serde_json::from_str(&out)?)
    };
    read.await.unwrap_or_else(|e| {
        log::info!("projects: teacli's servers not read on one machine: {e:#}");
        Vec::new()
    })
}

/// `server`'s repositories, read on the machines whose teacli has it, in turn.
async fn read_forgejo(machines: &[TeaMachine], server: &ForgejoUrl) -> Result<Vec<Listed>> {
    if machines.is_empty() {
        return Err(anyhow!("No online machine has teacli."));
    }

    let runs: Vec<Run> = machines
        .iter()
        .filter_map(|machine| {
            let named = machine.servers.iter().find(|known| known.is(server))?;
            Some(Run {
                shell: machine.shell.clone(),
                command: tea_list(&named.name),
            })
        })
        .collect();
    if runs.is_empty() {
        return Err(anyhow!(
            "No machine's teacli is signed in to {}.",
            server.host()
        ));
    }

    read_any(&runs, parse_teacli).await
}

/// Lists the repositories of the server teacli calls `name`. Whole repositories are long: it
/// keeps each one's top-level name, update and archived flag, so a long list fits the shell's
/// output.
fn tea_list(name: &str) -> String {
    format!(
        "out=$(teacli repo list --host {} --format json --all 2>&1) || {{ printf '%s\\n' \"$out\"; exit 1; }}; \
         printf '%s\\n' \"$out\" | grep -E {} || true",
        quote(name),
        quote(r#"^    "(full_name|updated_at|archived)":"#),
    )
}

/// A command and the machine to run it on.
struct Run {
    shell: Arc<dyn Shell>,
    command: String,
}

/// Runs each command in turn until one answers, so one broken CLI does not hide the list; when
/// none does, every error, in order.
async fn read_any(runs: &[Run], parse: fn(&str) -> Result<Vec<Listed>>) -> Result<Vec<Listed>> {
    match read_first(
        runs,
        |run| run_rendered(run.shell.as_ref(), &run.command),
        parse,
    )
    .await
    {
        Ok((_, repos)) => Ok(repos),
        Err(mut errors) => Err(match errors.len() {
            0 => anyhow!("no machine to read it on"),
            1 => errors.remove(0),
            _ => anyhow!(
                "{}",
                errors
                    .iter()
                    .map(|error| format!("{error:#}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }),
    }
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

/// What [`GH_REPOS`] prints, the newest [`GH_MOST`].
fn parse_gh(out: &str) -> Result<Vec<Listed>> {
    let mut repos = out
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, at) = line
                .split_once('\t')
                .ok_or_else(|| anyhow!("gh printed {line:?}"))?;
            Ok(Listed {
                name: name.to_owned(),
                updated_at: DateTime::parse_from_rfc3339(at.trim())?.into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    repos.sort_by_key(|repo| Reverse(repo.updated_at));
    repos.truncate(GH_MOST);
    Ok(repos)
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
#[path = "../tests/imports_host_repos.rs"]
mod tests;
