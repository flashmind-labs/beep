use super::*;
use crate::repo_watch::test_host::{Reply, TestHost};
use beep_proto::RunState;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Semaphore;

fn test_target(api: Api, base: &str) -> Target {
    Target {
        api,
        base: base.to_owned(),
        repo: RepoName::from_str("acme/widget").expect("a repository name"),
        host: "git.example.com".to_owned(),
        token: SecretString::from("tok"),
        who: Who("alice git.example.com".to_owned()),
    }
}

const RUNS: &str = r#"{"total_count": 1, "workflow_runs": [
      {"id": 77, "name": "CI", "run_number": 5, "event": "push", "status": "in_progress",
       "head_branch": "main", "actor": {"login": "some-user"}}]}"#;

const JOBS: &str = r#"{"jobs": [{"id": 3, "name": "Build", "status": "in_progress",
      "steps": [{"name": "Set up job", "status": "completed", "conclusion": "success", "number": 1},
                {"name": "Run tests", "status": "in_progress", "number": 2},
                {"name": "Complete job", "status": "queued", "number": 3}]}]}"#;

#[tokio::test]
async fn runs_and_jobs_read_with_etags_and_steps() {
    let host = TestHost::start(|seen| match seen.path.as_str() {
        "/repos/acme/widget/actions/runs?per_page=20" => {
            if seen.header("if-none-match").as_deref() == Some("\"r1\"") {
                Reply::status("304 Not Modified")
            } else {
                Reply::ok(RUNS).with("etag", "\"r1\"")
            }
        }
        "/repos/acme/widget/actions/runs/77/jobs?per_page=50" => Reply::ok(JOBS),
        _ => Reply::status("404 Not Found"),
    })
    .await;
    let target = test_target(Api::GitHub, &host.base);
    let client = beep_proto::https::client().build().expect("a client");
    let limit = Arc::new(Semaphore::new(2));
    let caller = Caller {
        client: &client,
        calls: &limit,
        host: &target.host,
        token: &target.token,
    };
    let calls = HostCalls::default();
    let first = list(&calls, &caller, &target).await.expect("the runs");
    let again = list(&calls, &caller, &target).await.expect("the runs");
    assert_eq!(first, again);
    assert_eq!(first[0].state, RunState::Running {});
    let jobs = jobs(&calls, &caller, &target, 77)
        .await
        .expect("the jobs")
        .expect("GitHub gives jobs");
    let now = crate::repo_watch::actions::progress(&jobs).expect("a job runs");
    let step = now.step.expect("with steps");
    assert_eq!(
        (step.number, step.of, step.name.as_str()),
        (2, 3, "Run tests")
    );
    let seen = host.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1].header("if-none-match").as_deref(), Some("\"r1\""));
}

#[tokio::test]
async fn forgejo_run_lists_request_a_page_to_enable_pagination() {
    let host = TestHost::start(|seen| match seen.path.as_str() {
        "/repos/acme/widget/actions/runs?limit=20&page=1" => Reply::ok(RUNS),
        _ => Reply::status("400 Bad Request"),
    })
    .await;
    let target = test_target(Api::Forgejo, &host.base);
    let client = beep_proto::https::client().build().expect("a client");
    let limit = Arc::new(Semaphore::new(2));
    let caller = target.caller(&limit, &client);
    let calls = HostCalls::default();
    let runs = list(&calls, &caller, &target).await.expect("paged runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, 77);
}

#[tokio::test]
async fn a_forgejo_without_a_jobs_api_gives_no_jobs_not_an_error() {
    let host = TestHost::start(|_| Reply::status("404 Not Found")).await;
    let target = test_target(Api::Forgejo, &host.base);
    let client = beep_proto::https::client().build().expect("a client");
    let limit = Arc::new(Semaphore::new(2));
    let caller = Caller {
        client: &client,
        calls: &limit,
        host: &target.host,
        token: &target.token,
    };
    let calls = HostCalls::default();
    let got = jobs(&calls, &caller, &target, 12).await.expect("no error");
    assert_eq!(got, None);
    assert_eq!(
        host.seen()[0].path,
        "/repos/acme/widget/actions/runs/12/jobs?limit=50&page=1"
    );
    // GitHub's 404 is an error: the run is gone.
    let github = test_target(Api::GitHub, &host.base);
    assert!(jobs(&calls, &caller, &github, 12).await.is_err());
}

#[tokio::test]
async fn pull_request_heads_use_the_local_host_api() {
    let host = TestHost::start(|seen| match seen.path.as_str() {
        "/repos/acme/widget/pulls?state=open&per_page=100"
        | "/repos/acme/widget/pulls?state=open&limit=100&page=1" => {
            Reply::ok(r#"[{"number":42,"head":{"ref":"fix-ci"}}]"#)
        }
        _ => Reply::status("404 Not Found"),
    })
    .await;
    let client = beep_proto::https::client().build().expect("client");
    let limit = Arc::new(Semaphore::new(2));
    let calls = HostCalls::default();

    for api in [Api::GitHub, Api::Forgejo] {
        let target = test_target(api, &host.base);
        let caller = target.caller(&limit, &client);
        assert_eq!(
            heads(&calls, &caller, &target).await.expect("heads"),
            vec![(42, "fix-ci".to_owned())]
        );
    }
}
