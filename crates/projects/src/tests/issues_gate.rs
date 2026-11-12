use super::*;
use crate::{
    Shell, ShellOutput,
    commands::HostCommand,
    issues::{
        ImportedPlace, MachineName, Machines, ProjectStore, SourceError, read_any, read_open,
        run_source,
    },
    model::RepoName,
};
use async_trait::async_trait;
use futures::future::join_all;
use iroh::SecretKey;
use std::{
    cell::RefCell,
    str::FromStr,
    sync::{
        Once,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::time::advance;

const NO_HOST: &str = r#"{"error":true,"code":1,"message":"failed to create client: Get \"http://forge.example/api/v1/version\": dial tcp: lookup forge.example: no such host"}"#;

/// Answers every command with what `answer` holds now, counting the commands by kind.
struct Forge {
    answer: std::sync::Mutex<ShellOutput>,
    reads: AtomicUsize,
    whoamis: AtomicUsize,
}

impl Forge {
    fn new(code: i32, out: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: std::sync::Mutex::new(ShellOutput::exited(code, out)),
            reads: AtomicUsize::new(0),
            whoamis: AtomicUsize::new(0),
        })
    }

    fn answers(&self, code: i32, out: &str) {
        *self.answer.lock().expect("not poisoned") = ShellOutput::exited(code, out);
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Shell for Forge {
    async fn run(&self, script: &str, _: Duration) -> Result<ShellOutput> {
        let counter = if script.contains("whoami") {
            &self.whoamis
        } else {
            &self.reads
        };
        counter.fetch_add(1, Ordering::SeqCst);
        // Lets other reads start while this one is out.
        tokio::task::yield_now().await;
        Ok(self.answer.lock().expect("not poisoned").clone())
    }
}

struct Gated {
    shell: Arc<Forge>,
    gates: HostGates,
}

#[async_trait]
impl Machines for Gated {
    fn shell(&self, _: &EndpointId) -> Option<Arc<dyn Shell>> {
        Some(self.shell.clone())
    }

    async fn names(&self) -> Vec<MachineName> {
        Vec::new()
    }

    fn gates(&self) -> Option<&HostGates> {
        Some(&self.gates)
    }
}

fn gated(code: i32, out: &str) -> (Gated, IssueSource) {
    let shell = Forge::new(code, out);
    let source = IssueSource {
        host: IssueHost::of_hostname("forge.example").expect("a Forgejo host"),
        machine: SecretKey::from_bytes(&[7; 32]).public(),
        shell: shell.clone(),
    };
    let machines = Gated {
        shell,
        gates: HostGates::default(),
    };
    (machines, source)
}

async fn list(machines: &Gated, source: &IssueSource) -> Result<String> {
    let repo = RepoName::from_str("acme/widget").expect("a repo");
    run_source(machines, source, &repo, HostCommand::ListOpenIssues).await
}

#[tokio::test(start_paused = true)]
async fn an_unreachable_host_is_left_alone_for_a_doubling_pause() {
    let (machines, source) = gated(1, NO_HOST);

    let first = list(&machines, &source).await.unwrap_err();
    assert!(is_unreachable(&first), "{first:#}");
    assert!(format!("{first:#}").contains("no such host"));
    assert_eq!(machines.shell.reads(), 1);

    let remembered = list(&machines, &source).await.unwrap_err();
    assert_eq!(format!("{remembered:#}"), format!("{first:#}"));
    assert_eq!(machines.shell.reads(), 1);

    advance(FIRST_PAUSE).await;
    list(&machines, &source).await.unwrap_err();
    assert_eq!(machines.shell.reads(), 2);

    // The second failure in a row pauses for twice as long.
    advance(FIRST_PAUSE).await;
    list(&machines, &source).await.unwrap_err();
    assert_eq!(machines.shell.reads(), 2);
    advance(FIRST_PAUSE).await;
    list(&machines, &source).await.unwrap_err();
    assert_eq!(machines.shell.reads(), 3);
}

#[tokio::test(start_paused = true)]
async fn an_answer_resets_the_pause() {
    let (machines, source) = gated(1, NO_HOST);
    list(&machines, &source).await.unwrap_err();
    advance(FIRST_PAUSE).await;
    list(&machines, &source).await.unwrap_err();

    advance(FIRST_PAUSE * 2).await;
    machines.shell.answers(0, "[]");
    assert_eq!(list(&machines, &source).await.expect("it answers"), "[]");

    machines.shell.answers(1, NO_HOST);
    list(&machines, &source).await.unwrap_err();
    advance(FIRST_PAUSE).await;
    list(&machines, &source).await.unwrap_err();
    assert_eq!(machines.shell.reads(), 5, "back to the first pause");
}

#[tokio::test(start_paused = true)]
async fn the_pause_stops_growing_at_fifteen_minutes() {
    let (machines, source) = gated(1, NO_HOST);

    for _ in 0..8 {
        list(&machines, &source).await.unwrap_err();
        advance(LONGEST_PAUSE).await;
    }

    assert_eq!(machines.shell.reads(), 8);
}

#[tokio::test(start_paused = true)]
async fn a_host_that_answers_with_an_error_is_not_paused() {
    let (machines, source) = gated(4, "401 Unauthorized: token is invalid");

    for _ in 0..3 {
        let error = list(&machines, &source).await.unwrap_err();
        assert!(!is_unreachable(&error), "{error:#}");
    }

    assert_eq!(machines.shell.reads(), 3);
}

#[tokio::test(start_paused = true)]
async fn reads_at_once_share_one_attempt() {
    let (machines, source) = gated(1, NO_HOST);

    let reads = join_all((0..20).map(|_| list(&machines, &source))).await;

    assert!(reads.iter().all(|read| read.is_err()));
    assert_eq!(machines.shell.reads(), 1);
}

#[tokio::test(start_paused = true)]
async fn identical_reads_at_once_run_the_cli_once() {
    let (machines, source) = gated(0, "[]");

    let reads = join_all((0..5).map(|_| list(&machines, &source))).await;

    assert!(reads.iter().all(|read| read.as_deref().ok() == Some("[]")));
    assert_eq!(machines.shell.reads(), 1);

    list(&machines, &source).await.expect("it answers");
    assert_eq!(machines.shell.reads(), 2, "a later read runs again");
}

#[tokio::test(start_paused = true)]
async fn different_reads_at_once_each_run() {
    let (machines, source) = gated(0, "[]");
    let repo = RepoName::from_str("acme/widget").expect("a repo");

    let (issues, prs) = tokio::join!(
        run_source(&machines, &source, &repo, HostCommand::ListOpenIssues),
        run_source(&machines, &source, &repo, HostCommand::ListOpenPrs),
    );

    assert!(issues.is_ok() && prs.is_ok());
    assert_eq!(machines.shell.reads(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_joined_read_keeps_the_kind_of_failure() {
    let (machines, source) = gated(1, NO_HOST);

    let reads = join_all((0..3).map(|_| list(&machines, &source))).await;

    for read in reads {
        let error = read.unwrap_err();
        assert!(error.downcast_ref::<Unreachable>().is_some(), "{error:#}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_dropped_attempt_gives_its_turn_to_the_next_read() {
    let (machines, source) = gated(1, NO_HOST);

    // Dropped while the CLI is out, as a timeout does.
    let mut first = Box::pin(list(&machines, &source));
    assert!(futures::poll!(first.as_mut()).is_pending());
    drop(first);

    list(&machines, &source).await.unwrap_err();
    assert_eq!(machines.shell.reads(), 2);
}

#[tokio::test(start_paused = true)]
async fn an_unreachable_host_skips_the_who_am_i_check() {
    let (machines, source) = gated(1, NO_HOST);
    let repo = RepoName::from_str("acme/widget").expect("a repo");
    let sources = [source];

    for _ in 0..3 {
        let read = read_any(
            &machines,
            &sources,
            &repo,
            HostCommand::ListOpenIssues,
            |out| Ok(out.to_owned()),
        )
        .await;
        assert!(matches!(read, Err(SourceError::Refused(_))));
    }

    assert_eq!(machines.shell.reads(), 1);
    assert_eq!(machines.shell.whoamis.load(Ordering::SeqCst), 0);
}

/// Machines by id, each with its own CLI, sharing one set of gates.
struct Fleet {
    shells: Vec<(EndpointId, Arc<Forge>)>,
    gates: HostGates,
}

#[async_trait]
impl Machines for Fleet {
    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>> {
        self.shells
            .iter()
            .find(|(id, _)| id == machine)
            .map(|(_, shell)| -> Arc<dyn Shell> { shell.clone() })
    }

    async fn names(&self) -> Vec<MachineName> {
        Vec::new()
    }

    fn gates(&self) -> Option<&HostGates> {
        Some(&self.gates)
    }
}

/// A project on the Forgejo host, checked out on `machines` in that order.
struct Project {
    name: RepoName,
    machines: Vec<EndpointId>,
}

#[async_trait]
impl ProjectStore for Project {
    fn name(&self) -> &RepoName {
        &self.name
    }

    fn issues_role(&self) -> Option<&IssueHost> {
        None
    }

    async fn home_machine(&self) -> Result<Option<EndpointId>> {
        Ok(None)
    }

    async fn imports(&self) -> Result<Vec<ImportedPlace>> {
        Ok(self
            .machines
            .iter()
            .map(|machine| ImportedPlace {
                url: "http://forge.example/acme/widget".to_owned(),
                machine: Some(*machine),
            })
            .collect())
    }

    async fn tracked_issues(&self, _: &IssueHost) -> Result<Vec<u64>> {
        Ok(Vec::new())
    }
}

/// A machine that cannot resolve the host, first, and one that reaches it.
fn split_network() -> (Fleet, Project) {
    let lost = SecretKey::from_bytes(&[1; 32]).public();
    let reaches = SecretKey::from_bytes(&[2; 32]).public();
    let fleet = Fleet {
        shells: vec![
            (lost, Forge::new(1, NO_HOST)),
            (reaches, Forge::new(0, "[]")),
        ],
        gates: HostGates::default(),
    };
    let project = Project {
        name: RepoName::from_str("acme/widget").expect("a repo"),
        machines: vec![lost, reaches],
    };
    (fleet, project)
}

impl Fleet {
    fn reads(&self) -> Vec<usize> {
        self.shells.iter().map(|(_, shell)| shell.reads()).collect()
    }
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_reaches_the_host_answers_for_one_that_does_not() {
    let (fleet, project) = split_network();

    let read = read_open(&project, &fleet)
        .await
        .expect("one machine answers");
    assert!(read.is_some_and(|read| read.open.is_empty()));
    assert_eq!(fleet.reads(), [1, 1]);

    // The machine that answered is asked first from now on, even once the pause is over.
    for _ in 0..3 {
        read_open(&project, &fleet).await.expect("it answers");
        advance(FIRST_PAUSE).await;
    }

    assert_eq!(fleet.reads(), [1, 4]);
}

#[tokio::test(start_paused = true)]
async fn a_host_no_machine_reaches_is_tried_again_from_each_after_the_pause() {
    let (fleet, project) = split_network();
    fleet.shells[1].1.answers(1, NO_HOST);

    for _ in 0..10 {
        let read = read_open(&project, &fleet).await;
        assert!(matches!(read, Err(SourceError::Refused(_))));
    }

    assert_eq!(fleet.reads(), [1, 1]);
    advance(FIRST_PAUSE).await;
    read_open(&project, &fleet).await.unwrap_err();
    assert_eq!(fleet.reads(), [2, 2]);
}

thread_local! {
    static LOGGED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Keeps what this thread logs, for the tests on one thread to read.
struct Captured;

impl log::Log for Captured {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        LOGGED.with(|logged| logged.borrow_mut().push(record.args().to_string()));
    }

    fn flush(&self) {}
}

/// What this thread logged since it was last asked.
fn logged() -> Vec<String> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        if log::set_logger(&Captured).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
    });
    LOGGED.with(|logged| logged.take())
}

#[tokio::test(start_paused = true)]
async fn a_paused_host_is_logged_when_it_pauses_and_answers_again_only() {
    let (machines, source) = gated(1, NO_HOST);
    let repo = RepoName::from_str("acme/widget").expect("a repo");
    let sources = [source];
    let read = || {
        read_any(
            &machines,
            &sources,
            &repo,
            HostCommand::ListOpenIssues,
            |out| Ok(out.to_owned()),
        )
    };
    logged();

    assert!(read().await.is_err(), "no machine reaches it");
    let first = logged();
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(first[0].contains("cannot reach"), "{first:?}");

    for _ in 0..50 {
        assert!(read().await.is_err(), "still paused");
    }

    assert_eq!(logged(), Vec::<String>::new());
    advance(FIRST_PAUSE).await;
    assert!(read().await.is_err(), "still unreachable");
    let again = logged();
    assert_eq!(again.len(), 1, "{again:?}");
    assert!(again[0].contains("still cannot reach"), "{again:?}");

    advance(FIRST_PAUSE * 2).await;
    machines.shell.answers(0, "[]");
    assert!(read().await.is_ok(), "it answers");
    let back = logged();
    assert_eq!(back.len(), 1, "{back:?}");
    assert!(back[0].contains("reaches"), "{back:?}");
}

#[test]
fn connection_failures_are_told_from_answers() {
    for printed in [
        NO_HOST,
        "dial tcp 10.0.0.1:80: connect: connection refused",
        "dial tcp 10.0.0.1:80: i/o timeout",
        "error connecting to forge.example\ncheck your internet connection",
    ] {
        assert!(no_connection(printed), "{printed}");
    }

    for printed in ["not found", "401 Unauthorized", "no rule"] {
        assert!(!no_connection(printed), "{printed}");
    }
}
