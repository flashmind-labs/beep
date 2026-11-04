//! Reading a project's issues host through its machines: which machines can reach it, and trying
//! each in turn.

use crate::{
    Shell,
    commands::{
        HostCommand, OpenIssue, OpenPr, PrHead, parse_login, parse_open_issues, parse_open_prs,
        parse_pr_head,
    },
    model::{IssueHost, RepoName},
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use beep_proto::{ProjectIssue, ProjectPullRequest};
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

    /// The numbers of the issues on `host` that tasks of the project track.
    async fn tracked_issues(&self, host: &IssueHost) -> Result<Vec<u64>>;
}

/// The user's machines, for reading a host through one of them.
#[async_trait]
pub trait Machines: Send + Sync {
    /// A shell on `machine`, while it is online and offers one.
    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>>;

    /// Every machine's name, for messages; empty when they are unknown.
    async fn names(&self) -> Vec<MachineName>;
}

/// A machine the project was imported on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPlace {
    /// The repository's web page, which names its host.
    pub url: String,
    /// `None` when the machine is not known by an endpoint id; the page still names the host.
    pub machine: Option<EndpointId>,
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
            machines: imports.iter().filter_map(|import| import.machine).collect(),
            home: false,
        }))
}

/// The project's issues host, if it has one.
pub async fn issue_host(store: &dyn ProjectStore) -> Result<Option<IssueHost>> {
    Ok(host_and_machines(store).await?.map(|found| found.host))
}

/// The first online place to read the project's issues (see [`issue_sources`]).
pub async fn issue_source(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Option<IssueSource>, SourceError> {
    Ok(issue_sources(store, machines)
        .await?
        .and_then(|sources| sources.into_iter().next()))
}

/// Every online place to read the project's issues: the home machine, else import machines by
/// id. `None` without a GitHub or Forgejo host; refused when none is online.
pub async fn issue_sources(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
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
            machines.shell(machine).map(|shell| IssueSource {
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
    match out.exit_code {
        Some(0) => Ok(out.output),
        code => {
            let tail: String = out
                .output
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

/// Runs a host command on each of `items`' shells in turn, so one broken CLI does not hide the
/// answer. `command` renders the script for an item and `parse` reads what it printed. Gives the
/// item that answered; when none does, every error, in order.
pub async fn read_first<S, T>(
    items: &[S],
    shell_of: impl Fn(&S) -> &dyn Shell,
    command: impl Fn(&S) -> String,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&S, T), Vec<anyhow::Error>> {
    let mut failures = Vec::new();
    for item in items {
        let read = run_rendered(shell_of(item), &command(item))
            .await
            .and_then(|out| parse(&out));
        match read {
            Ok(value) => return Ok((item, value)),
            Err(error) => {
                log::info!("projects: reading failed on one machine: {error:#}");
                failures.push(error);
            }
        }
    }
    Err(failures)
}

/// Tries `command` on each source in turn so one broken CLI does not hide the list. Gives the
/// source that answered; when all fail, every machine's error.
pub async fn read_any<'s, T>(
    machines: &dyn Machines,
    sources: &'s [IssueSource],
    repo: &RepoName,
    command: HostCommand<'_>,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&'s IssueSource, T), SourceError> {
    let read = read_first(
        sources,
        |source| source.shell.as_ref(),
        |source| command.render(&source.host, repo),
        parse,
    )
    .await;
    let mut failures = match read {
        Ok(answered) => return Ok(answered),
        Err(failures) => failures,
    };
    if failures.len() < 2 {
        let error = failures
            .pop()
            .unwrap_or_else(|| anyhow!("no machine to read it on"));
        return Err(host_failed(&error));
    }
    // Names only make the message clearer: without them it still says why.
    let names = machines.names().await;
    let tried: Vec<String> = sources
        .iter()
        .zip(&failures)
        .map(|(source, error)| {
            let name = names
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

/// Every open issue on a project's host, as it was read.
#[derive(Debug, Clone)]
pub struct HostIssues {
    pub host: IssueHost,
    pub open: Vec<OpenIssue>,
}

/// Every open issue on the project's host, tracked or not; `None` without a host.
pub async fn read_open(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Option<HostIssues>, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Ok(None);
    };
    let (source, open) = read_any(
        machines,
        &sources,
        store.name(),
        HostCommand::ListOpenIssues,
        parse_open_issues,
    )
    .await?;
    Ok(Some(HostIssues {
        host: source.host.clone(),
        open,
    }))
}

/// The issues of `open` that no task tracks, newest first.
pub fn untracked(open: Vec<OpenIssue>, tracked: &[u64]) -> Vec<OpenIssue> {
    let mut left: Vec<OpenIssue> = open
        .into_iter()
        .filter(|issue| !tracked.contains(&issue.number))
        .collect();
    left.sort_by(|a, b| {
        b.opened_at
            .cmp(&a.opened_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    left
}

/// The issues of `read` that no task of the project tracks now, newest first.
pub async fn untracked_now(
    store: &dyn ProjectStore,
    read: &HostIssues,
) -> Result<Vec<OpenIssue>, SourceError> {
    let tracked = store.tracked_issues(&read.host).await?;
    Ok(untracked(read.open.clone(), &tracked))
}

/// Open issues on the project's host no task tracks, newest first; empty without a host.
pub async fn list_open(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Vec<OpenIssue>, SourceError> {
    match read_open(store, machines).await? {
        Some(read) => untracked_now(store, &read).await,
        None => Ok(Vec::new()),
    }
}

/// An open pull request on a project's host, and whether it is the user's.
#[derive(Debug, Clone)]
pub struct ListedPr {
    pub pr: OpenPr,
    /// Opened by the CLI's signed-in account; false when that is unknown.
    pub mine: bool,
}

/// Open pull requests on the project's host, any author, newest first; empty without a host.
pub async fn list_open_prs(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Vec<ListedPr>, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Ok(Vec::new());
    };
    let (source, mut prs) = read_any(
        machines,
        &sources,
        store.name(),
        HostCommand::ListOpenPrs,
        parse_open_prs,
    )
    .await?;
    // Asked on the machine that listed them, whose CLI is known to work.
    let whoami = run_host(
        source.shell.as_ref(),
        &source.host,
        store.name(),
        HostCommand::WhoAmI,
    )
    .await;
    let login = match whoami.and_then(|out| parse_login(&out)) {
        Ok(login) => Some(login),
        Err(error) => {
            log::info!(
                "projects: the host's account is unknown, so no PR counts as mine: {error:#}"
            );
            None
        }
    };
    prs.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    Ok(prs
        .into_iter()
        .map(|pr| ListedPr {
            mine: login.as_deref().is_some_and(|login| {
                !pr.author.is_empty() && login.eq_ignore_ascii_case(&pr.author)
            }),
            pr,
        })
        .collect())
}

/// An open issue as the apps list it.
pub fn issue_view(issue: OpenIssue) -> ProjectIssue {
    ProjectIssue {
        number: issue.number,
        title: issue.title,
        labels: issue.labels,
        author: issue.author,
        opened_at: issue.opened_at.into(),
        comments: issue.comments,
        url: issue.url,
    }
}

/// An open pull request as the apps list it.
pub fn pull_request_view(listed: ListedPr) -> ProjectPullRequest {
    let pr = listed.pr;
    ProjectPullRequest {
        number: pr.number,
        title: pr.title,
        url: pr.url,
        author: pr.author,
        draft: pr.draft,
        mine: listed.mine,
        created_at: pr.created_at.into(),
        updated_at: pr.updated_at.into(),
    }
}

/// The branch pull request `number` is made from, read from the host now.
pub async fn pull_request_head(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
    number: u64,
) -> Result<PrHead, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Err(SourceError::Refused(
            "The project has no GitHub or Forgejo host to read pull requests from.".into(),
        ));
    };
    let (_, head) = read_any(
        machines,
        &sources,
        store.name(),
        HostCommand::ViewPrHead { number },
        parse_pr_head,
    )
    .await?;
    Ok(head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShellOutput;
    use iroh::SecretKey;
    use std::sync::Mutex;

    /// Answers every command the same way and counts them.
    struct FixedShell {
        answer: ShellOutput,
        ran: Mutex<usize>,
    }

    impl FixedShell {
        fn exits(code: i32, out: &str) -> Arc<FixedShell> {
            Arc::new(FixedShell {
                answer: ShellOutput::exited(code, out),
                ran: Mutex::new(0),
            })
        }
    }

    #[async_trait]
    impl Shell for FixedShell {
        async fn run(&self, _: &str, _: Duration) -> Result<ShellOutput> {
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
        async fn tracked_issues(&self, _: &IssueHost) -> Result<Vec<u64>> {
            Ok(Vec::new())
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

    /// Machines by id: those in `online` have a shell.
    struct Fleet {
        online: Vec<(EndpointId, Arc<FixedShell>)>,
    }

    #[async_trait]
    impl Machines for Fleet {
        fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>> {
            let (_, shell) = self.online.iter().find(|(id, _)| id == machine)?;
            Some(shell.clone())
        }

        async fn names(&self) -> Vec<MachineName> {
            names()
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
            &Fleet { online: Vec::new() },
            &sources,
            &repo(),
            HostCommand::ListOpenIssues,
            |out| Ok(out.to_owned()),
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
            &Fleet { online: Vec::new() },
            &sources,
            &repo(),
            HostCommand::ListOpenIssues,
            |out| Ok(out.to_owned()),
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
                    machine: Some(machine(seed)),
                })
                .to_vec(),
        };
        let online = FixedShell::exits(0, "");
        let fleet = Fleet {
            online: vec![(machine(2), online)],
        };
        let sources = issue_sources(&store, &fleet)
            .await
            .expect("a machine is online")
            .expect("a GitHub host");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].machine, machine(2));
        let nobody = Fleet { online: Vec::new() };
        let offline = issue_sources(&store, &nobody).await;
        assert!(matches!(offline, Err(SourceError::Refused(_))));
    }

    #[tokio::test]
    async fn open_pull_requests_are_marked_when_the_account_opened_them() {
        let prs = r#"[{"number":1,"title":"Mine","author":{"login":"Kim"},"url":"https://github.com/acme/widget/pull/1","createdAt":"2026-10-01T10:00:00Z","updatedAt":"2026-10-01T10:00:00Z","isDraft":false,"headRefName":"a","labels":[]},
            {"number":2,"title":"Theirs","author":{"login":"sam"},"url":"https://github.com/acme/widget/pull/2","createdAt":"2026-10-02T10:00:00Z","updatedAt":"2026-10-02T10:00:00Z","isDraft":false,"headRefName":"b","labels":[]}]"#;
        let shell = Arc::new(ByCommand(vec![
            ("pr list", ShellOutput::exited(0, prs)),
            ("api user", ShellOutput::exited(0, "kim\n")),
        ]));
        let fleet = Fleet2(shell);
        let store = Store {
            name: repo(),
            home: Some(machine(1)),
            imports: Vec::new(),
        };
        let listed = list_open_prs(&store, &fleet).await.expect("the list reads");
        let marks: Vec<(u64, bool)> = listed.iter().map(|p| (p.pr.number, p.mine)).collect();
        assert_eq!(marks, [(2, false), (1, true)]);
    }

    #[tokio::test]
    async fn an_import_on_an_unknown_machine_still_names_the_host() {
        let store = Store {
            name: repo(),
            home: None,
            imports: vec![ImportedPlace {
                url: "https://github.com/acme/widget".into(),
                machine: None,
            }],
        };
        let found = host_and_machines(&store)
            .await
            .expect("the store reads")
            .expect("a host");
        assert_eq!(found.host, IssueHost::Github);
        assert!(found.machines.is_empty() && !found.home);
    }

    /// Answers by the first rule whose text the command holds.
    struct ByCommand(Vec<(&'static str, ShellOutput)>);

    #[async_trait]
    impl Shell for ByCommand {
        async fn run(&self, script: &str, _: Duration) -> Result<ShellOutput> {
            Ok(self
                .0
                .iter()
                .find(|(holds, _)| script.contains(holds))
                .map_or_else(|| ShellOutput::exited(1, "no rule"), |(_, out)| out.clone()))
        }
    }

    struct Fleet2(Arc<ByCommand>);

    #[async_trait]
    impl Machines for Fleet2 {
        fn shell(&self, _: &EndpointId) -> Option<Arc<dyn Shell>> {
            Some(self.0.clone())
        }

        async fn names(&self) -> Vec<MachineName> {
            Vec::new()
        }
    }
}
