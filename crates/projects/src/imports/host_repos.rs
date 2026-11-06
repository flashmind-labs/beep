//! The user's repositories on their git hosts, read with `gh` and `teacli` on their machines, for
//! cloning from the Add a project sheet.

use crate::{
    Shell,
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

const GITHUB: &str = "github.com";
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
                    server.as_str(),
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
            Ok(url) => url.as_str() == server.as_str(),
            Err(_) => self.url.trim().trim_end_matches('/') == server.as_str(),
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
        |run| run.shell.as_ref(),
        |run| run.command.clone(),
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
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    /// A shell whose command always fails with `why`.
    struct Broken(&'static str);

    #[async_trait::async_trait]
    impl Shell for Broken {
        async fn run(&self, _: &str, _: Duration) -> Result<crate::ShellOutput> {
            Err(anyhow!(self.0))
        }
    }

    #[tokio::test]
    async fn a_list_no_machine_could_read_says_why_for_each() {
        let shells: Vec<Arc<dyn Shell>> = vec![
            Arc::new(Broken("gh is not signed in")),
            Arc::new(Broken("gh is not installed")),
        ];
        let hosts = read_hosts(&shells, &[], &[]).await;
        let said = hosts[0].error.clone().expect("both machines fail");
        assert!(
            said.contains("not signed in") && said.contains("not installed"),
            "{said}"
        );
    }

    /// A machine whose teacli is set up for the servers in `config`, and lists one repository
    /// for any of them. What it ran is kept.
    struct Tea {
        config: &'static str,
        ran: std::sync::Mutex<Vec<String>>,
    }

    impl Tea {
        fn new(config: &'static str) -> Arc<Self> {
            Arc::new(Self {
                config,
                ran: Default::default(),
            })
        }

        fn ran(&self) -> Vec<String> {
            self.ran.lock().expect("not poisoned").clone()
        }
    }

    #[async_trait::async_trait]
    impl Shell for Tea {
        async fn run(&self, script: &str, _: Duration) -> Result<crate::ShellOutput> {
            self.ran
                .lock()
                .expect("not poisoned")
                .push(script.to_owned());
            let listed = "    \"full_name\": \"team/infra\",\n    \"archived\": false,\n    \"updated_at\": \"2026-01-01T00:00:00Z\",";
            Ok(crate::ShellOutput::exited(
                0,
                if script == TEA_SERVERS {
                    self.config
                } else {
                    listed
                },
            ))
        }
    }

    #[tokio::test]
    async fn a_forgejo_list_runs_only_where_teacli_has_that_server_by_its_name() {
        let other =
            Tea::new(r#"[{"name":"work","url":"https://git.other.example","default":true}]"#);
        let has = Tea::new(r#"[{"name":"home","url":"https://GIT.example.com/","default":false}]"#);
        let tea: Vec<Arc<dyn Shell>> = vec![other.clone(), has.clone()];
        let server: ForgejoUrl = "https://git.example.com".parse().expect("a server URL");
        let hosts = read_hosts(&[], &tea, std::slice::from_ref(&server)).await;
        assert_eq!(hosts[0].error, None);
        assert_eq!(hosts[0].repos.len(), 1);
        assert_eq!(other.ran(), vec![TEA_SERVERS.to_owned()]);
        let ran = has.ran();
        assert_eq!(ran.len(), 2, "{ran:?}");
        assert!(ran[1].contains("--host home "), "{}", ran[1]);

        let none: Vec<Arc<dyn Shell>> = vec![Tea::new("[]")];
        let hosts = read_hosts(&[], &none, &[server]).await;
        assert_eq!(
            hosts[0].error.as_deref(),
            Some("No machine's teacli is signed in to git.example.com.")
        );
    }

    #[test]
    fn gh_and_teacli_lists_parse_newest_first_without_archived_repos() {
        let gh = "me/tool\t2026-01-01T00:00:00Z\nacme/app\t2026-01-01T00:00:10Z\n\n";
        assert_eq!(
            parse_gh(gh).expect("gh output parses"),
            vec![
                Listed {
                    name: "acme/app".into(),
                    updated_at: at(1_767_225_610)
                },
                Listed {
                    name: "me/tool".into(),
                    updated_at: at(1_767_225_600)
                }
            ]
        );
        assert!(parse_gh("not a repo line").is_err());
        let many: String = (0..GH_MOST + 5)
            .map(|n| format!("acme/r{n}\t2026-01-01T00:00:00Z\n"))
            .collect();
        assert_eq!(parse_gh(&many).expect("a long list parses").len(), GH_MOST);
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
