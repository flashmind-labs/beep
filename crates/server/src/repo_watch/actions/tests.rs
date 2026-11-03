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
}

#[test]
fn github_runs_parse_with_their_branch_event_actor_and_times() {
    let got = runs(GITHUB_RUNS);
    assert_eq!(got.len(), 3);
    let running = &got[0];
    assert_eq!(running.id, 9001);
    assert_eq!(running.number, 412);
    assert_eq!(running.workflow, "CI");
    assert_eq!(running.title, "Retry uploads when the token expires");
    assert_eq!(running.branch, "retry-uploads");
    assert_eq!(running.event, "pull_request");
    assert_eq!(running.actor, "some-user");
    assert_eq!(running.state, RunState::Running {});
    assert_eq!(running.started_at, Some(at("2026-11-03T10:00:00Z")));
    assert_eq!(running.finished_at, None, "still running");
    assert_eq!(running.pull_requests, [142]);
    let failed = &got[1];
    assert_eq!(failed.state, RunState::Failed {});
    assert_eq!(failed.finished_at, Some(at("2026-11-03T09:07:30Z")));
    assert_eq!(got[2].state, RunState::Queued {});
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
fn a_gitea_workflow_is_named_by_its_file() {
    let json = r#"{"workflow_runs": [{"id": 1, "path": ".gitea/workflows/test.yaml@refs/heads/main",
      "status": "completed", "conclusion": "timed_out", "head_branch": "main"}]}"#;
    let got = runs(json);
    assert_eq!(got[0].workflow, "test.yaml");
    assert_eq!(got[0].state, RunState::Failed {});
}

#[test]
fn states_follow_status_then_conclusion() {
    assert_eq!(state_of("completed", Some("success")), RunState::Passed {});
    assert_eq!(
        state_of("completed", Some("cancelled")),
        RunState::Cancelled {}
    );
    assert_eq!(state_of("completed", Some("skipped")), RunState::Skipped {});
    assert_eq!(state_of("completed", None), RunState::Unknown {});
    assert_eq!(state_of("waiting", None), RunState::Waiting {});
    assert_eq!(state_of("blocked", None), RunState::Waiting {});
    assert_eq!(state_of("failure", None), RunState::Failed {});
    assert_eq!(state_of("something-new", None), RunState::Unknown {});
}

const JOBS: &str = r#"{"total_count": 2, "jobs": [
  {"id": 501, "run_id": 9001, "name": "Lint", "status": "completed", "conclusion": "success",
   "started_at": "2026-11-03T10:00:00Z", "completed_at": "2026-11-03T10:01:00Z",
   "html_url": "https://github.com/acme/widget/actions/runs/9001/job/501",
   "steps": [{"name": "Set up job", "status": "completed", "conclusion": "success", "number": 1}]},
  {"id": 502, "run_id": 9001, "name": "Build", "status": "in_progress", "conclusion": null,
   "started_at": "2026-11-03T10:01:00Z", "completed_at": null,
   "steps": [
     {"name": "Set up job", "status": "completed", "conclusion": "success", "number": 1},
     {"name": "Checkout", "status": "completed", "conclusion": "success", "number": 2},
     {"name": "Install Rust", "status": "completed", "conclusion": "success", "number": 3},
     {"name": "Run tests", "status": "in_progress", "conclusion": null, "number": 4,
      "started_at": "2026-11-03T10:02:00Z"},
     {"name": "Clippy", "status": "queued", "number": 5},
     {"name": "Build docs", "status": "queued", "number": 6},
     {"name": "Upload", "status": "queued", "number": 7},
     {"name": "Post Checkout", "status": "queued", "number": 8},
     {"name": "Complete job", "status": "queued", "number": 9}
   ]}
]}"#;

fn jobs(json: &str) -> Vec<RunJob> {
    serde_json::from_str::<JobsReply>(json)
        .expect("the fixture is a job list")
        .jobs()
}

#[test]
fn a_running_run_shows_its_job_and_step() {
    let got = jobs(JOBS);
    assert_eq!(got[0].steps.len(), 1);
    assert_eq!(got[1].steps.len(), 9);
    let now = progress(&got).expect("a job runs");
    assert_eq!(now.job, "Build");
    assert_eq!(now.jobs_done, 1);
    assert_eq!(now.jobs, 2);
    assert_eq!(now.job_started_at, Some(at("2026-11-03T10:01:00Z")));
    let step = now.step.expect("the host gives steps");
    assert_eq!(step.number, 4);
    assert_eq!(step.of, 9);
    assert_eq!(step.name, "Run tests");
    assert_eq!(step.started_at, Some(at("2026-11-03T10:02:00Z")));
}

#[test]
fn jobs_without_steps_give_job_level_progress() {
    let json = r#"{"jobs": [
      {"id": 1, "name": "build", "status": "success"},
      {"id": 2, "name": "test", "status": "running", "started_at": "2026-11-03T10:00:00Z"},
      {"id": 3, "name": "deploy", "status": "waiting"}
    ]}"#;
    let got = jobs(json);
    assert!(got.iter().all(|job| job.steps.is_empty()));
    let now = progress(&got).expect("a job runs");
    assert_eq!(now.job, "test");
    assert_eq!((now.jobs_done, now.jobs), (1, 3));
    assert_eq!(now.step, None);
    assert_eq!(progress(&got[..1]), None, "nothing left to run");
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

#[test]
fn a_log_without_marks_gives_its_last_lines() {
    let log: String = (1..=10).map(|i| format!("line {i}\n")).collect();
    let cut = failing_part(&log, 3);
    assert_eq!(cut.text, "line 8\nline 9\nline 10");
    assert!(cut.truncated);
    assert_eq!(cut.step, None);
    assert!(!failing_part("one\ntwo", 5).truncated);
}
