//! The user's repositories on their git hosts, read with `gh` and `teacli` on their machines, for
//! cloning from the Add a project sheet.

use crate::{Shell, issues::read_first, model::ForgejoUrl, quote};
use anyhow::{Result, anyhow};
use beep_proto::{GitHostKind, HostRepo, HostRepos};
use chrono::{DateTime, FixedOffset};
use futures::future::join_all;
use serde::Deserialize;
use serde_json::Value;
use std::{cmp::Reverse, sync::Arc, time::SystemTime};

const GITHUB: &str = "github.com";

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
        let command = "gh repo list --json nameWithOwner,updatedAt,isArchived --limit 100";
        let read = read_any(gh, command, parse_gh).await;
        Some(listed(
            GITHUB.to_owned(),
            GitHostKind::GitHub {},
            "https://github.com",
            read,
        ))
    };
    let forgejo = servers.iter().map(|server| {
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

/// Runs `command` on each shell in turn until one answers, so one broken CLI does not hide the
/// list; when none does, the last error.
async fn read_any(
    shells: &[Arc<dyn Shell>],
    command: &str,
    parse: fn(&str) -> Result<Vec<Listed>>,
) -> Result<Vec<Listed>> {
    match read_first(
        shells,
        |shell| shell.as_ref(),
        |_| command.to_owned(),
        parse,
    )
    .await
    {
        Ok((_, repos)) => Ok(repos),
        Err(mut errors) => Err(errors
            .pop()
            .unwrap_or_else(|| anyhow!("no machine to read it on"))),
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
    use std::time::{Duration, UNIX_EPOCH};

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
