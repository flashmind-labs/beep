use super::*;
use crate::{Shell, ShellOutput};
use iroh::SecretKey;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

#[tokio::test]
async fn host_timeout_does_not_include_partial_json() {
    let shell = FixedShell {
        answer: ShellOutput {
            exit_code: None,
            timed_out: true,
            output: r#"{"body":"example description","comments":1}"#.to_owned(),
        },
        ran: Mutex::new(0),
    };
    let error = run_rendered(&shell, "read-issue").await.unwrap_err();

    assert_eq!(
        host_failed(&error).to_string(),
        "Could not read the issues host: the host command timed out after 30 seconds"
    );
}

#[tokio::test]
async fn host_failure_preserves_diagnostic_output() {
    let shell = FixedShell::exits(4, "permission denied");
    let error = run_rendered(shell.as_ref(), "read-issue")
        .await
        .unwrap_err();

    assert_eq!(
        host_failed(&error).to_string(),
        "Could not read the issues host: the host command exited with 4: permission denied"
    );
}

struct Store {
    name: RepoName,
    imports: Vec<ImportedPlace>,
}

/// The project imported from `url` on `machine(1)`.
fn imported(url: &str) -> Vec<ImportedPlace> {
    vec![ImportedPlace {
        url: url.into(),
        machine: Some(machine(1)),
    }]
}

#[async_trait]
impl ProjectStore for Store {
    fn name(&self) -> &RepoName {
        &self.name
    }
    async fn imports(&self) -> Result<Vec<ImportedPlace>> {
        Ok(self.imports.clone())
    }
}

fn machine(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

fn repo() -> RepoName {
    RepoName::from_str("acme/widget").expect("a repo")
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
    let sources = [
        by_command(
            1,
            &[("issue list", 1, "not logged in"), ("whoami", 0, "kim")],
        ),
        by_command(
            2,
            &[("issue list", 1, "unknown flag"), ("whoami", 0, "kim")],
        ),
    ];
    let why = refused(read_issues(&sources).await);

    for part in ["build-box", "not logged in", "mac-box", "unknown flag"] {
        assert!(why.contains(part), "{part} missing from {why}");
    }
}

#[tokio::test]
async fn the_error_lists_only_machines_with_the_host_set_up() {
    let sources = [
        by_command(
            1,
            &[("issue list", 3, "not found"), ("whoami", 3, "not found")],
        ),
        by_command(
            2,
            &[
                ("issue list", 1, "rate limited"),
                ("whoami", 0, "{\"login\":\"kim\"}"),
            ],
        ),
        by_command(
            3,
            &[
                ("issue list", 1, "no such host"),
                ("whoami", 1, "no such host"),
            ],
        ),
    ];
    let why = refused(read_issues(&sources).await);
    assert!(
        why.contains("mac-box") && why.contains("rate limited"),
        "{why}"
    );
    assert!(
        !why.contains("build-box") && !why.contains("no such host"),
        "{why}"
    );
}

#[tokio::test]
async fn no_machine_with_the_host_set_up_says_so_plainly() {
    let sources = [
        by_command(
            1,
            &[("issue list", 3, "not found"), ("whoami", 3, "not found")],
        ),
        by_command(
            2,
            &[
                ("issue list", 1, "no such host"),
                ("whoami", 1, "no such host"),
            ],
        ),
    ];
    let why = refused(read_issues(&sources).await);
    assert_eq!(why, "No online machine has forge.example set up.");
}

#[tokio::test]
async fn a_lone_source_keeps_its_own_error() {
    let sources = [by_command(
        1,
        &[("issue list", 3, "not found"), ("whoami", 3, "not found")],
    )];
    let why = refused(read_issues(&sources).await);
    assert!(why.contains("exited with 3"), "{why}");
}

#[tokio::test]
async fn a_machine_without_the_host_does_not_hide_a_later_success() {
    let sources = [
        by_command(
            1,
            &[
                ("issue list", 1, "no such host"),
                ("whoami", 1, "no such host"),
            ],
        ),
        by_command(2, &[("issue list", 0, "[]")]),
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
async fn a_missing_repository_is_an_error_for_issues_but_no_pull_requests() {
    let shell = Arc::new(ByCommand(vec![
        ("issue list", ShellOutput::exited(3, "not found")),
        ("pr list", ShellOutput::exited(3, "not found")),
        ("whoami", ShellOutput::exited(0, "{\"login\":\"kim\"}")),
    ]));
    let store = forgejo_store();
    let fleet = Fleet2(shell);
    let prs = list_open_prs(&store, &fleet).await.expect("none open");
    assert!(prs.is_empty());
    let issues = list_open(&store, &fleet).await;
    assert!(matches!(issues, Err(SourceError::Refused(_))));
}

#[tokio::test]
async fn pull_requests_not_found_without_the_host_is_an_error() {
    let shell = Arc::new(ByCommand(vec![
        ("pr list", ShellOutput::exited(3, "not found")),
        ("whoami", ShellOutput::exited(3, "not found")),
    ]));
    let prs = list_open_prs(&forgejo_store(), &Fleet2(shell)).await;
    assert!(matches!(prs, Err(SourceError::Refused(_))));
}

#[tokio::test]
async fn gh_failures_stay_errors_on_the_pull_request_list() {
    let shell = Arc::new(ByCommand(vec![
        ("pr list", ShellOutput::exited(3, "not found")),
        ("api user", ShellOutput::exited(0, "kim")),
    ]));
    let store = Store {
        name: repo(),
        imports: imported("https://github.com/acme/widget"),
    };
    let prs = list_open_prs(&store, &Fleet2(shell)).await;
    assert!(matches!(prs, Err(SourceError::Refused(_))));
}

fn forge() -> IssueHost {
    IssueHost::of_hostname("forge.example").expect("a Forgejo host")
}

fn forgejo_store() -> Store {
    Store {
        name: repo(),
        imports: imported("https://forge.example/acme/widget"),
    }
}

/// A source on Forgejo whose shell answers `rules` as (command text, exit code, output).
fn by_command(seed: u8, rules: &[(&'static str, i32, &str)]) -> IssueSource {
    IssueSource {
        host: forge(),
        machine: machine(seed),
        shell: Arc::new(ByCommand(
            rules
                .iter()
                .map(|(holds, code, out)| (*holds, ShellOutput::exited(*code, out)))
                .collect(),
        )),
    }
}

async fn read_issues(sources: &[IssueSource]) -> Result<(), SourceError> {
    read_any(
        &Fleet { online: Vec::new() },
        sources,
        &repo(),
        HostCommand::ListOpenIssues,
        |out| Ok(out.to_owned()),
    )
    .await
    .map(|_| ())
}

fn refused(read: Result<(), SourceError>) -> String {
    let Err(SourceError::Refused(why)) = read else {
        panic!("expected the host's errors");
    };
    why
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
        imports: imported("https://github.com/acme/widget"),
    };
    let listed = list_open_prs(&store, &fleet).await.expect("the list reads");
    let marks: Vec<(u64, bool)> = listed.iter().map(|p| (p.pr.number, p.mine)).collect();
    assert_eq!(marks, [(2, false), (1, true)]);
}

#[tokio::test]
async fn an_import_on_an_unknown_machine_still_names_the_host() {
    let store = Store {
        name: repo(),
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
    assert!(found.machines.is_empty());
}

fn github_home() -> Store {
    Store {
        name: repo(),
        imports: imported("https://github.com/acme/widget"),
    }
}

#[tokio::test]
async fn a_pull_request_reads_in_full_through_a_machine() {
    let pr = r#"{"number": 41, "title": "Cut the cache", "body": "Shorter", "url": "https://github.com/acme/widget/pull/41", "author": {"login": "sam"}, "state": "OPEN", "isDraft": false, "labels": [], "createdAt": "2026-10-30T09:00:00Z", "comments": [{"author": {"login": "alice"}, "createdAt": "2026-10-30T11:00:00Z", "body": "Looks good"}], "reviews": [], "reviewDecision": "", "statusCheckRollup": [{"conclusion": "FAILURE"}]}"#;
    let fleet = Fleet2(Arc::new(ByCommand(vec![(
        "pr view 41",
        ShellOutput::exited(0, pr),
    )])));
    let store = github_home();
    let detail = crate::thread_detail::read(&store, &fleet, 41)
        .await
        .expect("the pull request reads");
    assert_eq!(detail.title, "Cut the cache");
    assert_eq!(detail.comments.len(), 1);
    assert_eq!(detail.checks, Some(beep_proto::RunState::Failed {}));
    let other = crate::thread_detail::read(&store, &fleet, 42).await;
    assert!(other.is_err(), "no rule answers #42");
}

#[tokio::test]
async fn an_issue_reads_with_its_linked_pull_requests() {
    let issue = r#"{"number": 12, "title": "Crash", "body": "", "url": "https://github.com/acme/widget/issues/12", "author": {"login": "sam"}, "state": "OPEN", "labels": [], "createdAt": "2026-10-30T09:00:00Z", "comments": []}"#;
    let linked = r#"[{"number": 13, "title": "Fix the crash", "url": "https://github.com/acme/widget/pull/13", "state": "OPEN"},
        {"number": 4, "title": "Mention", "url": "https://github.com/acme/other/pull/4", "state": "MERGED"}]"#;
    let read = |linked: ShellOutput| async move {
        let fleet = Fleet2(Arc::new(ByCommand(vec![
            ("graphql", linked),
            ("issue view 12", ShellOutput::exited(0, issue)),
        ])));
        crate::thread_detail::read(&github_home(), &fleet, 12)
            .await
            .expect("the issue reads")
            .linked_prs
    };

    let prs = read(ShellOutput::exited(0, linked)).await;
    assert_eq!(
        prs.iter()
            .map(|pr| (pr.number, pr.state))
            .collect::<Vec<_>>(),
        [
            (13, beep_proto::ThreadState::Open {}),
            (4, beep_proto::ThreadState::Merged {})
        ]
    );
    // Without them the issue still reads.
    assert!(
        read(ShellOutput::exited(1, "rate limited"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_comment_on_a_pull_request_goes_through_gh_pr_comment() {
    let fleet = Fleet2(Arc::new(ByCommand(vec![(
        "pr comment 41",
        ShellOutput::exited(0, ""),
    )])));
    let pr = beep_proto::GitItemKind::PullRequest {};
    crate::thread_detail::comment(&github_home(), &fleet, pr, 41, "Thanks")
        .await
        .expect("the comment posts");
    let issue = beep_proto::GitItemKind::Issue {};
    let store = github_home();
    let refused = crate::thread_detail::comment(&store, &fleet, issue, 41, "Thanks");
    assert!(
        refused.await.is_err(),
        "an issue comment is not a pr comment"
    );
}

#[tokio::test]
async fn issues_a_pull_request_is_linked_to_are_marked() {
    let issues = r#"[{"number": 12, "title": "Crash", "createdAt": "2026-10-30T09:00:00Z"},
        {"number": 13, "title": "Slow", "createdAt": "2026-10-29T09:00:00Z"}]"#;
    let read = |linked: ShellOutput| async move {
        let fleet = Fleet2(Arc::new(ByCommand(vec![
            ("linked:pr", linked),
            ("issue list", ShellOutput::exited(0, issues)),
        ])));
        let open = list_open(&github_home(), &fleet)
            .await
            .expect("the issues list");
        open.iter()
            .map(|issue| (issue.number, issue.linked_pr))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        read(ShellOutput::exited(0, "[12]")).await,
        [(12, true), (13, false)]
    );
    // Without the linked search the issues still list, untagged.
    assert_eq!(
        read(ShellOutput::exited(1, "rate limited")).await,
        [(12, false), (13, false)]
    );
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
