use super::*;
use crate::{
    Shell, ShellOutput,
    commands::HostCommand,
    issues::{MachineName, Machines, SourceError, read_any, run_source},
    model::RepoName,
};
use async_trait::async_trait;
use futures::future::join_all;
use iroh::SecretKey;
use std::{
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
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
async fn reads_at_once_go_ahead_together_once_the_host_answered() {
    let (machines, source) = gated(0, "[]");

    let reads = join_all((0..5).map(|_| list(&machines, &source))).await;

    assert!(reads.iter().all(|read| read.is_ok()));
    assert_eq!(machines.shell.reads(), 5);
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
