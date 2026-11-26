use super::*;
use crate::agents::checkout::tests::{has_git, repo};
use std::fs;
use tokio::time::{advance, pause, timeout};

const WAIT: Duration = Duration::from_secs(20);

fn ask(context: u32) -> DiffAsk {
    DiffAsk {
        root: String::new(),
        base: String::new(),
        head: String::new(),
        only: Vec::new(),
        lockfiles: false,
        context,
    }
}

/// Reads until two reads in a row give the same diff: the first ones come
/// before the checkout's watch holds, and are not kept.
async fn kept(checkouts: &Checkouts, root: &Path) -> Arc<WorkDiff> {
    timeout(WAIT, async {
        loop {
            let first = checkouts.diff(root.to_owned(), ask(3)).await.unwrap();
            let second = checkouts.diff(root.to_owned(), ask(3)).await.unwrap();

            if Arc::ptr_eq(&first, &second) {
                return first;
            }
        }
    })
    .await
    .expect("the checkout keeps its diff")
}

#[tokio::test]
async fn readers_share_one_diff_until_the_checkout_changes() {
    if !has_git() {
        return;
    }

    let (_dir, work) = repo("shared");
    let root = crate::files::paths::real(&work).unwrap();
    let checkouts = Checkouts::default();
    let first = kept(&checkouts, &root).await;
    assert_eq!((first.additions, first.deletions), (0, 0));
    // A session counts its lines with the viewer's ask, so it starts no git.
    let counted = checkouts.diff(root.clone(), ask(0)).await.unwrap();
    assert!(Arc::ptr_eq(&first, &counted));

    fs::write(work.join("README.md"), "hi\nthere\n").unwrap();
    let changes = timeout(WAIT, async {
        loop {
            let changes = checkouts.changes(work.clone()).await.unwrap();

            if changes.additions == 1 {
                return changes;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the change is heard");
    assert_eq!(changes.deletions, 0);
    let after = kept(&checkouts, &root).await;
    assert!(!Arc::ptr_eq(&first, &after));
    assert_eq!(after.files.len(), 1);
}

#[tokio::test]
async fn readers_share_one_status_until_the_checkout_changes() {
    if !has_git() {
        return;
    }

    let (_dir, work) = repo("status");
    let root = crate::files::paths::real(&work).unwrap();
    let checkouts = Checkouts::default();
    // The first reads come before the watch holds: they are not kept, and
    // the checkout answers none.
    let first = timeout(WAIT, async {
        loop {
            let first = checkouts.status(root.clone()).await;
            let second = checkouts.status(root.clone()).await;

            if let (Some(first), Some(second)) = (first, second)
                && Arc::ptr_eq(&first, &second)
            {
                return first;
            }
        }
    })
    .await
    .expect("the checkout keeps its status");
    assert!(first.out.is_empty());

    fs::write(work.join("README.md"), "hi\nthere\n").unwrap();
    timeout(WAIT, async {
        loop {
            if checkouts
                .status(root.clone())
                .await
                .is_some_and(|status| !status.out.is_empty())
            {
                return;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the change is heard");
}

#[tokio::test]
async fn a_folder_that_is_not_a_checkout_has_no_status() {
    let dir = tempfile::tempdir().unwrap();
    let root = crate::files::paths::real(dir.path()).unwrap();
    assert!(Checkouts::default().status(root).await.is_none());
}

#[tokio::test]
async fn a_diff_of_another_branch_is_not_kept() {
    if !has_git() {
        return;
    }

    let (_dir, work) = repo("branch");
    let root = crate::files::paths::real(&work).unwrap();
    let checkouts = Checkouts::default();
    kept(&checkouts, &root).await;
    let branch = DiffAsk {
        head: "main".into(),
        ..ask(3)
    };
    let first = checkouts.diff(root.clone(), branch.clone()).await.unwrap();
    let second = checkouts.diff(root, branch).await.unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn a_checkout_nobody_reads_ends_its_task() {
    let dir = tempfile::tempdir().unwrap();
    let checkouts = Checkouts::default();
    let asks = checkouts
        .open(dir.path().to_owned())
        .await
        .unwrap()
        .downgrade();
    assert!(asks.upgrade().is_some());
    pause();

    for _ in 0..20 {
        if asks.upgrade().is_none() {
            break;
        }

        advance(SWEEP).await;
    }

    assert!(asks.upgrade().is_none());
}
