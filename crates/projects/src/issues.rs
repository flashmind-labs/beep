//! Reading a project's issues host through its machines: which machines can reach it, and trying
//! each in turn.

use crate::{
    Shell,
    commands::HostCommand,
    model::{IssueHost, RepoName},
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use iroh::EndpointId;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use url::Url;

/// How long one host command may take.
pub const HOST_LIMIT: Duration = Duration::from_secs(30);

/// Why a host could not be read.
#[derive(Debug, Error)]
pub enum SourceError {
    /// For the user, saying why.
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

/// What the issue modules read about one project, kept by the server in Postgres and by the
/// desktop in its mesh state.
#[async_trait]
pub trait ProjectStore: Send + Sync {
    fn name(&self) -> &RepoName;

    /// Where issues are kept, as the home checkout's setup says.
    fn issues_role(&self) -> Option<&IssueHost>;

    /// The machine of the project's home checkout.
    async fn home_machine(&self) -> Result<Option<EndpointId>>;

    /// The places the project was imported on, matched to it by name.
    async fn imports(&self) -> Result<Vec<ImportedPlace>>;
}

/// A machine the project was imported on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPlace {
    /// The repository's web page.
    pub url: String,
    pub machine: EndpointId,
}

/// A machine's id and the name its user gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineName {
    pub id: EndpointId,
    pub name: String,
}

/// Where a project's open issues are read: its host, via a shell on a machine with the project.
pub struct IssueSource {
    pub host: IssueHost,
    pub machine: EndpointId,
    pub shell: Arc<dyn Shell>,
}

/// The issues host a repository web page is on.
pub fn host_of_url(url: &str) -> Option<IssueHost> {
    IssueHost::of_hostname(Url::parse(url).ok()?.host_str()?)
}

/// A project's issues host and the machines that can reach it.
pub struct HostAndMachines {
    pub host: IssueHost,
    /// The home checkout's machine, else the machines it was imported on.
    pub machines: Vec<EndpointId>,
    /// Whether the machines are the home checkout's.
    pub home: bool,
}

/// From the home checkout's issues role and machine, else the imported web page's host and
/// machines. `None` without a GitHub or Forgejo host.
pub async fn host_and_machines(store: &dyn ProjectStore) -> Result<Option<HostAndMachines>> {
    if let Some(machine) = store.home_machine().await? {
        return Ok(store.issues_role().cloned().map(|host| HostAndMachines {
            host,
            machines: vec![machine],
            home: true,
        }));
    }
    let imports = store.imports().await?;
    Ok(imports
        .iter()
        .find_map(|import| host_of_url(&import.url))
        .map(|host| HostAndMachines {
            host,
            machines: imports.iter().map(|import| import.machine).collect(),
            home: false,
        }))
}

/// Every online place to read the project's issues: the home machine, else import machines by
/// id. `None` without a GitHub or Forgejo host; refused when none is online.
pub async fn issue_sources(
    store: &dyn ProjectStore,
    shell: impl Fn(&EndpointId) -> Option<Arc<dyn Shell>>,
) -> Result<Option<Vec<IssueSource>>, SourceError> {
    let Some(found) = host_and_machines(store).await? else {
        return Ok(None);
    };
    let mut seen = Vec::new();
    let sources: Vec<IssueSource> = found
        .machines
        .iter()
        .filter(|machine| {
            let new = !seen.contains(*machine);
            seen.push(**machine);
            new
        })
        .filter_map(|machine| {
            shell(machine).map(|shell| IssueSource {
                host: found.host.clone(),
                machine: *machine,
                shell,
            })
        })
        .collect();
    if sources.is_empty() {
        return Err(SourceError::Refused(if found.home {
            "The home checkout's machine is offline or offers no shell.".to_owned()
        } else {
            "No machine the project is on is online.".to_owned()
        }));
    }
    Ok(Some(sources))
}

/// Runs `command` against `repo`'s `host`, failing unless it exits 0.
pub async fn run_host(
    shell: &dyn Shell,
    host: &IssueHost,
    repo: &RepoName,
    command: HostCommand<'_>,
) -> Result<String> {
    run_rendered(shell, &command.render(host, repo)).await
}

/// Runs the host command `rendered`, failing unless it exits 0.
pub async fn run_rendered(shell: &dyn Shell, rendered: &str) -> Result<String> {
    let out = shell.run(rendered, HOST_LIMIT).await?;
    match out.code {
        Some(0) => Ok(out.out),
        code => {
            let tail: String = out
                .out
                .chars()
                .rev()
                .take(400)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            anyhow::bail!(
                "the host command exited with {}: {}",
                code.map_or("no code".to_owned(), |code| code.to_string()),
                tail.trim()
            )
        }
    }
}

/// Tries `command` on each source in turn so one broken CLI does not hide the list. Gives the
/// source that answered; when all fail, every machine's error.
pub async fn read_any<'s, T>(
    sources: &'s [IssueSource],
    repo: &RepoName,
    command: HostCommand<'_>,
    parse: impl Fn(&str) -> Result<T>,
    names: impl AsyncFnOnce() -> Vec<MachineName>,
) -> Result<(&'s IssueSource, T), SourceError> {
    let mut failures = Vec::new();
    for source in sources {
        let read = run_host(source.shell.as_ref(), &source.host, repo, command)
            .await
            .and_then(|out| parse(&out));
        match read {
            Ok(value) => return Ok((source, value)),
            Err(error) => {
                log::info!("projects: reading the host failed on one machine: {error:#}");
                failures.push(error);
            }
        }
    }
    if failures.len() < 2 {
        let error = failures
            .pop()
            .unwrap_or_else(|| anyhow!("no machine to read it on"));
        return Err(host_failed(&error));
    }
    // Names only make the message clearer: without them it still says why.
    let machines = names().await;
    let tried: Vec<String> = sources
        .iter()
        .zip(&failures)
        .map(|(source, error)| {
            let name = machines
                .iter()
                .find(|machine| machine.id == source.machine)
                .map_or("a machine", |machine| machine.name.as_str());
            format!("{name}: {error:#}")
        })
        .collect();
    Err(SourceError::Refused(format!(
        "Could not read the issues host. {}",
        tried.join("; ")
    )))
}

pub fn host_failed(error: &anyhow::Error) -> SourceError {
    SourceError::Refused(format!("Could not read the issues host: {error:#}"))
}

/// The start of `text`, at most `max` bytes, cut on a character boundary.
pub fn head(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandOutput;
    use iroh::SecretKey;
    use std::sync::Mutex;

    /// Answers every command the same way and counts them.
    struct FixedShell {
        answer: CommandOutput,
        ran: Mutex<usize>,
    }

    impl FixedShell {
        fn exits(code: i32, out: &str) -> Arc<FixedShell> {
            Arc::new(FixedShell {
                answer: CommandOutput {
                    code: Some(code),
                    out: out.to_owned(),
                },
                ran: Mutex::new(0),
            })
        }
    }

    #[async_trait]
    impl Shell for FixedShell {
        async fn run(&self, _: &str, _: Duration) -> Result<CommandOutput> {
            *self.ran.lock().expect("the counter is not poisoned") += 1;
            Ok(self.answer.clone())
        }
    }

    struct Store {
        name: RepoName,
        home: Option<EndpointId>,
        imports: Vec<ImportedPlace>,
    }

    #[async_trait]
    impl ProjectStore for Store {
        fn name(&self) -> &RepoName {
            &self.name
        }
        fn issues_role(&self) -> Option<&IssueHost> {
            Some(&IssueHost::Github)
        }
        async fn home_machine(&self) -> Result<Option<EndpointId>> {
            Ok(self.home)
        }
        async fn imports(&self) -> Result<Vec<ImportedPlace>> {
            Ok(self.imports.clone())
        }
    }

    fn machine(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn repo() -> RepoName {
        RepoName::parse("acme/widget").expect("a repo")
    }

    fn source(machine: EndpointId, shell: Arc<FixedShell>) -> IssueSource {
        IssueSource {
            host: IssueHost::Github,
            machine,
            shell,
        }
    }

    fn names() -> Vec<MachineName> {
        vec![
            MachineName {
                id: machine(1),
                name: "build-box".into(),
            },
            MachineName {
                id: machine(2),
                name: "mac-box".into(),
            },
        ]
    }

    #[tokio::test]
    async fn issues_read_tries_each_machine_and_reports_every_failure() {
        let first = FixedShell::exits(1, "not logged in");
        let second = FixedShell::exits(1, "unknown flag");
        let sources = [
            source(machine(1), first.clone()),
            source(machine(2), second.clone()),
        ];
        let read = read_any(
            &sources,
            &repo(),
            HostCommand::ListOpenIssues,
            |out| Ok(out.to_owned()),
            async || names(),
        )
        .await;
        let Err(SourceError::Refused(why)) = read else {
            panic!("expected the host's errors");
        };
        for part in ["build-box", "not logged in", "mac-box", "unknown flag"] {
            assert!(why.contains(part), "{part} missing from {why}");
        }
        assert_eq!(*first.ran.lock().expect("not poisoned"), 1);
        assert_eq!(*second.ran.lock().expect("not poisoned"), 1);
    }

    #[tokio::test]
    async fn a_machine_that_fails_gives_way_to_the_next() {
        let broken = FixedShell::exits(1, "not logged in");
        let working = FixedShell::exits(0, "[]");
        let sources = [
            source(machine(1), broken),
            source(machine(2), working.clone()),
        ];
        let (answered, out) = read_any(
            &sources,
            &repo(),
            HostCommand::ListOpenIssues,
            |out| Ok(out.to_owned()),
            async || panic!("a read that worked needs no names"),
        )
        .await
        .expect("the second machine answers");
        assert_eq!(answered.machine, machine(2));
        assert_eq!(out, "[]");
    }

    #[tokio::test]
    async fn sources_skip_offline_machines_and_repeats() {
        let store = Store {
            name: repo(),
            home: None,
            imports: [1, 1, 2]
                .map(|seed| ImportedPlace {
                    url: "https://github.com/acme/widget".into(),
                    machine: machine(seed),
                })
                .to_vec(),
        };
        let online = FixedShell::exits(0, "");
        let sources = issue_sources(&store, |id| {
            (*id == machine(2)).then(|| online.clone() as Arc<dyn Shell>)
        })
        .await
        .expect("a machine is online")
        .expect("a GitHub host");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].machine, machine(2));
        let offline = issue_sources(&store, |_| None).await;
        assert!(matches!(offline, Err(SourceError::Refused(_))));
    }
}
