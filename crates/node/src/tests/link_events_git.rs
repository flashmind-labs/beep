use super::*;
use crate::agents::checkouts::watch_paths;
use crate::link::Offers;
use beep_projects::{Shell, ShellOutput};
use beep_proto::node::{
    LinkRx, LinkTx,
    events::{Query, Read, Reply, Request, Snapshot, Topic, Value},
};
use std::{path::Path, time::Duration};
use tokio::{process::Command, time::timeout};

async fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-c")
        .arg("user.name=Example")
        .arg("-c")
        .arg("user.email=developer@example.test")
        .args(args)
        .current_dir(root)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Recorded(GitOutput);

#[async_trait::async_trait]
impl Shell for Recorded {
    async fn run(&self, _script: &str, _limit: Duration) -> Result<ShellOutput> {
        Ok(ShellOutput {
            exit_code: self.0.exit_code,
            timed_out: self.0.timed_out,
            output: self.0.output.clone(),
        })
    }
}

async fn projection(rx: &mut LinkRx, query: u64) -> GitOutput {
    timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv::<Reply>().await.unwrap().unwrap() {
                Reply::Snapshot(snapshot) if snapshot.query == Some(query) => {
                    let Snapshot {
                        value: Value::Git(value),
                        ..
                    } = *snapshot
                    else {
                        panic!("expected Git state")
                    };
                    return value;
                }
                Reply::SnapshotError {
                    query: Some(id),
                    reason,
                    ..
                } if id == query => panic!("{reason}"),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}

async fn subscribe(root: &Path) -> (LinkTx, LinkRx, AbortOnDropHandle<Result<()>>) {
    let offers = Offers::default();
    offers.agents.share_folders(vec![root.to_owned()]);
    let link = local_link();
    let task = AbortOnDropHandle::new(tokio::spawn(super::super::serve(
        link.node.0,
        link.node.1,
        offers,
        iroh::SecretKey::generate().public(),
    )));
    let (mut tx, mut rx) = link.caller;
    assert_eq!(rx.recv::<Reply>().await.unwrap(), Some(Reply::Ready));
    let root_name = root.to_str().unwrap().to_owned();
    let topic = Topic::keyed("files", &root_name);
    tx.send(&Request::Watch {
        id: 1,
        topics: vec![topic.clone()],
        queries: vec![Query {
            id: 7,
            topic,
            read: Read::Git {
                root: root_name,
                query: GitRead::History {
                    revision: None,
                    offset: 0,
                    search: String::new(),
                },
            },
        }],
    })
    .await
    .unwrap();
    (tx, rx, task)
}

#[tokio::test]
async fn history_subscription_pushes_the_first_committed_file_without_polling() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--initial-branch=main"]).await;
    git(root.path(), &["commit", "--allow-empty", "-m", "first"]).await;
    let (_tx, mut rx, _task) = subscribe(root.path()).await;
    let first = projection(&mut rx, 7).await;
    let first =
        history::parse_history(history::run(&Recorded(first), "").await.unwrap(), 0).unwrap();
    assert_eq!(first.commits.len(), 1);
    std::fs::write(root.path().join("example.txt"), "first file").unwrap();
    git(root.path(), &["add", "example.txt"]).await;
    git(root.path(), &["commit", "-m", "second"]).await;

    loop {
        let next = projection(&mut rx, 7).await;
        let next =
            history::parse_history(history::run(&Recorded(next), "").await.unwrap(), 0).unwrap();
        if next.commits.len() == 2 {
            assert_eq!(next.commits[0].subject, "second");
            break;
        }
    }
}

#[tokio::test]
async fn linked_worktree_metadata_pushes_an_empty_commit_without_checkout_writes() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--initial-branch=main"]).await;
    git(root.path(), &["commit", "--allow-empty", "-m", "first"]).await;
    let checkout = root.path().join("linked");
    git(
        root.path(),
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            checkout.to_str().unwrap(),
        ],
    )
    .await;
    let paths = watch_paths(&checkout).await.unwrap();
    assert_eq!(paths.len(), 8);
    assert!(paths.iter().all(|path| !path.starts_with(&checkout)));
    let (_tx, mut rx, _task) = subscribe(&checkout).await;
    projection(&mut rx, 7).await;
    git(
        &checkout,
        &["commit", "--allow-empty", "-m", "metadata only"],
    )
    .await;

    loop {
        let next = projection(&mut rx, 7).await;
        let next =
            history::parse_history(history::run(&Recorded(next), "").await.unwrap(), 0).unwrap();
        if next.commits.len() == 2 {
            assert_eq!(next.commits[0].subject, "metadata only");
            break;
        }
    }
}

#[tokio::test]
async fn git_reads_validate_revisions_authorize_roots_and_leave_refs_unchanged() {
    assert!(
        !script("/srv/project", GitRead::State)
            .unwrap()
            .contains("git fetch")
    );
    assert!(
        script(
            "/srv/project",
            GitRead::Files {
                commit: "HEAD; false".into()
            }
        )
        .is_err()
    );
    assert!(
        script(
            "/srv/project",
            GitRead::History {
                revision: Some("HEAD".into()),
                offset: 0,
                search: String::new()
            }
        )
        .is_err()
    );
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--initial-branch=main"]).await;
    let agents = Agents::default();
    agents.share_folders(vec![root.path().to_owned()]);
    assert!(
        read(&agents, outside.path().to_str().unwrap(), GitRead::State)
            .await
            .is_err()
    );
    let before = std::fs::read(root.path().join(".git/HEAD")).unwrap();
    let output = read(&agents, root.path().to_str().unwrap(), GitRead::State)
        .await
        .unwrap();
    let state = checkout_sync::parse_state(&output.output).unwrap();
    assert_eq!(state.branch.as_deref(), Some("main"));
    assert_eq!(
        std::fs::read(root.path().join(".git/HEAD")).unwrap(),
        before
    );
    assert!(!root.path().join(".git/FETCH_HEAD").exists());
}

mod current {
    use super::*;
    use beep_proto::node::subscriptions::{Reply, Request};

    async fn projection(rx: &mut LinkRx) -> GitOutput {
        timeout(Duration::from_secs(10), async {
            loop {
                match rx.recv::<Reply>().await.unwrap().unwrap() {
                    Reply::Snapshot(snapshot) if snapshot.query == Some(7) => {
                        let Snapshot {
                            value: Value::Git(value),
                            ..
                        } = *snapshot
                        else {
                            panic!("expected Git state")
                        };
                        return value;
                    }
                    Reply::SnapshotError {
                        query: Some(7),
                        reason,
                        ..
                    } => panic!("{reason}"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap()
    }

    async fn commits(rx: &mut LinkRx, wanted: usize) {
        loop {
            let next = projection(rx).await;
            if history::parse_history(history::run(&Recorded(next), "").await.unwrap(), 0)
                .is_ok_and(|history| history.commits.len() == wanted)
            {
                return;
            }
        }
    }

    async fn subscribe(
        offers: Offers,
        root: &Path,
    ) -> (LinkTx, LinkRx, AbortOnDropHandle<Result<()>>) {
        let link = local_link();
        let task = AbortOnDropHandle::new(tokio::spawn(crate::link::events::serve_subscriptions(
            link.node.0,
            link.node.1,
            offers,
            iroh::SecretKey::generate().public(),
            crate::link::events::Reach::Whole,
            false,
        )));
        let (mut tx, mut rx) = link.caller;
        assert_eq!(rx.recv::<Reply>().await.unwrap(), Some(Reply::Ready));
        let root_name = root.to_str().unwrap().to_owned();
        let topic = Topic::keyed("files", &root_name);
        tx.send(&Request::Watch {
            id: 1,
            topic: topic.clone(),
            queries: vec![Query {
                id: 7,
                topic,
                read: Read::Git {
                    root: root_name,
                    query: GitRead::History {
                        revision: None,
                        offset: 0,
                        search: String::new(),
                    },
                },
            }],
            deltas: false,
            query_deltas: false,
        })
        .await
        .unwrap();
        (tx, rx, task)
    }

    // The watcher is not installed while the first bind runs, so a folder
    // that becomes a repository then raises no event: the bind asks Git
    // again once the watcher is there.
    #[tokio::test]
    async fn a_folder_that_becomes_a_repository_while_binding_pushes_its_later_commits() {
        let root = tempfile::tempdir().unwrap();
        let offers = Offers::default();
        offers.agents.share_folders(vec![root.path().to_owned()]);
        let (reached, reached_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release) = tokio::sync::oneshot::channel();
        crate::lock(&offers.bind_gates).push(crate::link::events::sources::Gate {
            root: root.path().canonicalize().unwrap(),
            reached,
            release,
        });
        let (_tx, mut rx, _task) = subscribe(offers, root.path()).await;
        projection(&mut rx).await;
        // Git has been asked and found no repository; no watcher is installed.
        reached_rx.await.unwrap();

        git(root.path(), &["init", "--initial-branch=main"]).await;
        git(root.path(), &["commit", "--allow-empty", "-m", "first"]).await;
        release_tx.send(()).unwrap();
        commits(&mut rx, 1).await;

        // Only Git metadata changes now.
        git(root.path(), &["commit", "--allow-empty", "-m", "second"]).await;
        commits(&mut rx, 2).await;
    }
}

/// Runs `script` the way the shell read would, for comparing with gitoxide.
async fn sh(script: &str) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[tokio::test]
async fn gitoxide_reads_the_state_and_branches_the_scripts_print() {
    use beep_projects::{branches, checkout_sync};

    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let origin = base.join("origin");
    let work = base.join("work");
    std::fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]).await;

    for message in ["one", "two"] {
        git(&origin, &["commit", "-q", "--allow-empty", "-m", message]).await;
    }

    git(&base, &["clone", "-q", "origin", "work"]).await;
    git(
        &origin,
        &["commit", "-q", "--allow-empty", "-m", "upstream only"],
    )
    .await;
    git(&work, &["fetch", "-q"]).await;
    git(
        &work,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "local only\n\nwith a body",
        ],
    )
    .await;
    git(&work, &["branch", "old", "HEAD~2"]).await;
    git(&work, &["switch", "-q", "-c", "feature"]).await;

    for message in ["feature one", "feature two"] {
        git(&work, &["commit", "-q", "--allow-empty", "-m", message]).await;
    }

    git(&work, &["switch", "-q", "main"]).await;
    let linked = base.join("linked");
    git(
        &work,
        &["worktree", "add", "-q", linked.to_str().unwrap(), "feature"],
    )
    .await;

    for root in [&work, &linked] {
        let folder = root.to_str().unwrap();
        let scripted = sh(&checkout_sync::local_state_script(folder)).await;
        let native = super::super::git_reads::state(root).unwrap();
        assert_eq!(
            checkout_sync::parse_state(&native).unwrap(),
            checkout_sync::parse_state(&scripted).unwrap(),
            "{native}\n---\n{scripted}"
        );

        let scripted = sh(&branches::branches_script(folder, "main")).await;
        let native = super::super::git_reads::branches(root, "main").unwrap();
        assert_eq!(
            format!("{:?}", branches::parse_branches(&native).unwrap()),
            format!("{:?}", branches::parse_branches(&scripted).unwrap()),
            "{native}\n---\n{scripted}"
        );
    }

    git(&work, &["switch", "-q", "--detach", "HEAD"]).await;
    let scripted = sh(&checkout_sync::local_state_script(work.to_str().unwrap())).await;
    let native = super::super::git_reads::state(&work).unwrap();
    assert_eq!(
        checkout_sync::parse_state(&native).unwrap(),
        checkout_sync::parse_state(&scripted).unwrap()
    );
}
