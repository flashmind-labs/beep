use super::*;
use crate::{
    git_items::forks::Api,
    projects::model::{ForgejoHost, RepoName},
    protocol::GitHostKind,
    repo_watch::test_host::{Reply, TestHost},
};
use secrecy::SecretString;

const GITHUB_RUNS: &str = r#"{"total_count": 2, "workflow_runs": [
  {"id": 501, "name": "CI", "run_number": 12, "event": "push", "status": "completed",
   "conclusion": "success", "head_branch": "main", "head_sha": "abc",
   "run_started_at": "2026-11-01T10:20:00Z", "updated_at": "2026-11-01T10:25:00Z"},
  {"id": 500, "name": "CI", "run_number": 11, "event": "push", "status": "completed",
   "conclusion": "failure", "head_branch": "main", "head_sha": "old",
   "run_started_at": "2026-11-01T09:00:00Z", "updated_at": "2026-11-01T09:05:00Z"}]}"#;

const FORGEJO_RUNS: &str = r#"{"total_count": 1, "workflow_runs": [
  {"id": 77, "index_in_repo": 4, "workflow_id": "ci.yml", "event": "push",
   "status": "failure", "prettyref": "main", "commit_sha": "abc",
   "started": "2026-11-01T10:21:00Z", "stopped": "2026-11-01T10:24:00Z"}]}"#;

const JOBS: &str = r#"{"jobs": [{"id": 9, "name": "Build", "status": "completed",
  "conclusion": "success"}]}"#;

fn github() -> RepoOnHost {
    RepoOnHost {
        host: IssueHost::Github,
        repo: RepoName::parse("acme/widget").expect("a name"),
        primary: true,
    }
}

fn forge() -> RepoOnHost {
    RepoOnHost {
        host: IssueHost::Forgejo(ForgejoHost::parse("forge.example.com").expect("a host")),
        repo: RepoName::parse("mirrors/widget").expect("a name"),
        primary: false,
    }
}

fn target_at(place: &RepoOnHost, base: &str) -> Target {
    let api = match place.host {
        IssueHost::Github => Api::GitHub,
        IssueHost::Forgejo(_) => Api::Forgejo,
    };
    Target {
        api,
        base: base.to_owned(),
        repo: place.repo.clone(),
        host: place.name(),
        token: SecretString::from("tok"),
        who: Who(format!("alice {}", place.name())),
    }
}

fn copy_of(places: Vec<RepoOnHost>) -> RunsCopy {
    let mut copy = RunsCopy::default();
    copy.set_places(places);
    copy
}

const READ: Plan = Plan {
    forced: true,
    stale: true,
    poll: false,
};

/// GitHub and Forgejo hosts for tests, the Forgejo one answering with
/// `forgejo`.
async fn hosts(forgejo: fn(&str) -> Reply) -> (TestHost, TestHost) {
    let gh = TestHost::start(|seen| match seen.path.as_str() {
        "/repos/acme/widget/actions/runs?per_page=20" => Reply::ok(GITHUB_RUNS),
        "/repos/acme/widget/actions/runs/501/jobs?per_page=50" => Reply::ok(JOBS),
        _ => Reply::status("404 Not Found"),
    })
    .await;
    let fj = TestHost::start(move |seen| forgejo(&seen.path)).await;
    (gh, fj)
}

fn forgejo_ok(path: &str) -> Reply {
    match path {
        "/repos/mirrors/widget/actions/runs?limit=20" => Reply::ok(FORGEJO_RUNS),
        "/repos/mirrors/widget/actions/runs/77" => Reply::ok(
            r#"{"id": 77, "index_in_repo": 4, "workflow_id": "ci.yml", "event": "push",
                "status": "failure", "prettyref": "main"}"#,
        ),
        "/repos/mirrors/widget/actions/runs/77/jobs?limit=50" => Reply::ok(JOBS),
        _ => Reply::status("404 Not Found"),
    }
}

fn forgejo_down(_: &str) -> Reply {
    Reply::status("502 Bad Gateway")
}

#[tokio::test]
async fn runs_from_two_hosts_merge_newest_first_each_naming_its_host() {
    let (gh, fj) = hosts(forgejo_ok).await;
    let watch = RepoWatch::default();
    let mut copy = copy_of(vec![github(), forge()]);
    let targets = vec![
        Ok(target_at(&github(), &gh.base)),
        Ok(target_at(&forge(), &fj.base)),
    ];
    let calls = Arc::new(Semaphore::new(4));
    let ok = watch
        .read_hosts(&calls, &Client::new(), &mut copy, targets, READ)
        .await;
    assert!(ok);
    let heads = [OpenHead {
        number: 3,
        branch: "main".to_owned(),
    }];
    let view = copy.hosted_view(&watch.calls, &heads);
    let got: Vec<(&str, u64)> = view
        .runs
        .iter()
        .map(|hosted| (hosted.host.as_str(), hosted.run.id))
        .collect();
    assert_eq!(
        got,
        [
            ("forge.example.com", 77),
            ("github.com", 501),
            ("github.com", 500)
        ]
    );
    assert_eq!(view.hosts.len(), 2);
    assert_eq!(view.hosts[0].kind, GitHostKind::GitHub {});
    assert!(view.hosts[0].primary);
    assert_eq!(view.hosts[1].repo, "mirrors/widget");
    assert!(view.hosts.iter().all(|host| host.error.is_none()));
    assert_eq!(view.error, None);
    // The old reply is the primary host's alone, as before.
    let old = copy.primary_view(&watch.calls, &heads);
    assert_eq!(
        old.runs.iter().map(|run| run.id).collect::<Vec<_>>(),
        [501, 500]
    );
    assert_eq!(old.error, None);
}

#[tokio::test]
async fn a_host_that_fails_says_so_while_the_other_still_shows() {
    let (gh, fj) = hosts(forgejo_down).await;
    let watch = RepoWatch::default();
    let mut copy = copy_of(vec![github(), forge()]);
    let targets = vec![
        Ok(target_at(&github(), &gh.base)),
        Ok(target_at(&forge(), &fj.base)),
    ];
    let calls = Arc::new(Semaphore::new(4));
    let ok = watch
        .read_hosts(&calls, &Client::new(), &mut copy, targets, READ)
        .await;
    assert!(!ok, "a host failed");
    let view = copy.hosted_view(&watch.calls, &[]);
    assert_eq!(view.runs.len(), 2);
    assert!(view.runs.iter().all(|hosted| hosted.host == "github.com"));
    assert_eq!(view.hosts[0].error, None);
    assert!(view.hosts[1].error.is_some(), "the Forgejo host says why");
    assert_eq!(view.error, None);

    // A host with no token says so too, and the other still reads.
    let targets = vec![
        Ok(target_at(&github(), &gh.base)),
        Err("Add a token for forge.example.com in Settings.".to_owned()),
    ];
    watch
        .read_hosts(&calls, &Client::new(), &mut copy, targets, READ)
        .await;
    let view = copy.hosted_view(&watch.calls, &[]);
    assert_eq!(view.runs.len(), 2);
    assert_eq!(
        view.hosts[1].error.as_deref(),
        Some("Add a token for forge.example.com in Settings.")
    );
    // The old reply sees only the primary host, which answered.
    assert_eq!(copy.primary_view(&watch.calls, &[]).error, None);
}

#[tokio::test]
async fn a_run_opened_on_the_mirror_is_read_from_the_mirror() {
    let (gh, fj) = hosts(forgejo_ok).await;
    let watch = RepoWatch::default();
    let mut copy = copy_of(vec![github(), forge()]);
    let targets = vec![
        Ok(target_at(&github(), &gh.base)),
        Ok(target_at(&forge(), &fj.base)),
    ];
    let calls = Arc::new(Semaphore::new(4));
    watch
        .read_hosts(&calls, &Client::new(), &mut copy, targets, READ)
        .await;
    let before = gh.seen().len();

    let at = copy
        .slot_of(Some(&forge().host))
        .expect("the mirror's slot");
    assert_eq!(at, 1);
    assert_eq!(copy.slot_of(None), Some(0), "the primary host by default");
    let target = target_at(&forge(), &fj.base);
    let client = Client::new();
    let read = DetailRead {
        calls: &calls,
        client: &client,
        target: &target,
        polled: false,
    };
    let RunsCopy { hosts, logs, .. } = &mut copy;
    let detail = watch
        .detail_on(&read, &mut hosts[at], logs, 77)
        .await
        .expect("the run");
    assert_eq!(detail.run.id, 77);
    assert_eq!(detail.run.state, RunState::Failed {});
    assert_eq!(detail.jobs.len(), 1);
    assert_eq!(gh.seen().len(), before, "nothing asked of GitHub");
    assert!(
        fj.seen()
            .iter()
            .any(|seen| seen.path == "/repos/mirrors/widget/actions/runs/77/jobs?limit=50")
    );
    // A host the project is not on has no slot.
    let other = IssueHost::Forgejo(ForgejoHost::parse("elsewhere.example.com").expect("a host"));
    assert_eq!(copy.slot_of(Some(&other)), None);
}
