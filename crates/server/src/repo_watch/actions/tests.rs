use super::*;

const GITHUB_RUNS: &str = r#"{"total_count": 3, "workflow_runs": [
  {"id": 9001, "name": "CI", "display_title": "Retry uploads when the token expires",
   "run_number": 412, "event": "pull_request", "status": "in_progress", "conclusion": null,
   "workflow_id": 77, "head_branch": "retry-uploads", "path": ".github/workflows/ci.yml",
   "html_url": "https://github.com/acme/widget/actions/runs/9001",
   "run_started_at": "2026-11-03T10:00:00Z", "updated_at": "2026-11-03T10:03:00Z",
   "actor": {"login": "some-user"},
   "pull_requests": [{"number": 142, "head": {"ref": "retry-uploads"}}]},
  {"id": 9000, "name": "CI", "display_title": "Bump serde", "run_number": 411,
   "event": "push", "status": "completed", "conclusion": "failure",
   "head_branch": "main", "html_url": "https://github.com/acme/widget/actions/runs/9000",
   "run_started_at": "2026-11-03T09:00:00Z", "updated_at": "2026-11-03T09:07:30Z",
   "actor": {"login": "dependabot[bot]"}, "pull_requests": []},
  {"id": 8999, "name": "Release", "display_title": "v1.2.0", "run_number": 40,
   "event": "push", "status": "queued", "head_branch": "main",
   "html_url": "https://github.com/acme/widget/actions/runs/8999",
   "run_started_at": "2026-11-03T08:00:00Z", "actor": {"login": "some-user"}}
]}"#;

fn at(text: &str) -> SystemTime {
    DateTime::parse_from_rfc3339(text)
        .expect("a test time")
        .with_timezone(&Utc)
        .into()
}

fn runs(json: &str) -> Vec<WorkflowRun> {
    serde_json::from_str::<RunsReply>(json)
        .expect("the fixture is a run list")
        .runs()
        .into_iter()
        .map(|listed| listed.run)
        .collect()
}

#[test]
fn forgejo_runs_parse_from_their_own_field_names() {
    let json = r#"{"workflow_runs": [
      {"id": 31, "title": "Fix the login copy", "workflow_id": "build.yml",
       "index_in_repo": 12, "trigger_user": {"login": "kim"}, "prettyref": "login-copy",
       "event": "push", "status": "running",
       "started": "2026-11-03T10:00:00Z", "stopped": "0001-01-01T00:00:00Z",
       "html_url": "https://git.example.com/acme/widget/actions/runs/12"},
      {"id": 30, "title": "Spanish strings", "workflow_id": "build.yml",
       "index_in_repo": 11, "prettyref": "refs/pull/7/head", "event": "pull_request",
       "status": "success", "started": "2026-11-02T10:00:00Z",
       "stopped": "2026-11-02T10:04:00Z"}
    ], "total_count": 2}"#;
    let got = runs(json);
    assert_eq!(got[0].workflow, "build.yml");
    assert_eq!(got[0].number, 12);
    assert_eq!(got[0].actor, "kim");
    assert_eq!(got[0].branch, "login-copy");
    assert_eq!(got[0].state, RunState::Running {});
    assert_eq!(got[0].finished_at, None);
    assert_eq!(got[1].pull_requests, [7]);
    assert_eq!(got[1].state, RunState::Passed {});
    assert_eq!(got[1].finished_at, Some(at("2026-11-02T10:04:00Z")));
}

#[test]
fn a_pull_requests_checks_take_the_newest_run_of_each_workflow() {
    let mut list = runs(GITHUB_RUNS);
    // An older CI run for #142 that passed is not what decides.
    list.push(WorkflowRun {
        id: 8000,
        state: RunState::Passed {},
        ..list[0].clone()
    });
    // A pull request event on #57's head branch, with no number named.
    list.push(WorkflowRun {
        id: 7000,
        workflow: "Lint".into(),
        branch: "faster-start".into(),
        event: "pull_request".into(),
        state: RunState::Failed {},
        pull_requests: Vec::new(),
        ..list[1].clone()
    });
    // A push to a fork PR's branch name is not the PR's.
    list.push(WorkflowRun {
        id: 6000,
        branch: "main".into(),
        event: "push".into(),
        state: RunState::Failed {},
        pull_requests: Vec::new(),
        ..list[1].clone()
    });
    let got = pr_checks(
        &list,
        &[
            PrHeadRef {
                number: 142,
                head: "retry-uploads",
            },
            PrHeadRef {
                number: 57,
                head: "faster-start",
            },
            PrHeadRef {
                number: 9,
                head: "main",
            },
            PrHeadRef {
                number: 3,
                head: "",
            },
        ],
    );
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].number, 142);
    assert_eq!(got[0].state, RunState::Running {});
    assert_eq!(got[0].run_id, 9001);
    assert_eq!(got[1].number, 57);
    assert_eq!(got[1].state, RunState::Failed {});
    assert_eq!(got[1].run_id, 7000);
}

#[test]
fn the_failing_steps_lines_are_cut_from_a_github_log() {
    let log = "2026-11-03T10:00:00.1000000Z ##[group]Run actions/checkout@v4\n\
               2026-11-03T10:00:01.1000000Z Fetching the repository\n\
               2026-11-03T10:00:02.1000000Z ##[endgroup]\n\
               2026-11-03T10:00:03.1000000Z ##[group]Run cargo test\n\
               2026-11-03T10:00:04.1000000Z \u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m widget v0.1.0\n\
               2026-11-03T10:00:05.1000000Z test upload::retries ... FAILED\n\
               2026-11-03T10:00:06.1000000Z ##[error]Process completed with exit code 101.\n\
               2026-11-03T10:00:07.1000000Z Post job cleanup.\n";
    let cut = failing_part(log, 50);
    assert_eq!(cut.step.as_deref(), Some("Run cargo test"));
    assert!(cut.truncated, "the checkout step is left out");
    assert_eq!(
        cut.text,
        "##[group]Run cargo test\n   Compiling widget v0.1.0\ntest upload::retries ... FAILED\n\
         ##[error]Process completed with exit code 101.\nPost job cleanup."
    );
    let short = failing_part(log, 2);
    assert_eq!(
        short.text,
        "##[error]Process completed with exit code 101.\nPost job cleanup."
    );
}
