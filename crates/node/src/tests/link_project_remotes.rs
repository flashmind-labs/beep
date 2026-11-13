use super::*;
use tempfile::TempDir;

async fn run(root: &Path, args: &[&str]) -> String {
    let out = git(root).args(args).output().await.unwrap();
    assert!(out.status.success(), "{args:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn offline_renames_update_fetch_and_push_urls_and_keep_forks_and_worktrees() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    run(root, &["init", "-q"]).await;
    run(
        root,
        &["remote", "add", "upstream", "git@github.com:team/old.git"],
    )
    .await;
    run(
        root,
        &[
            "config",
            "remote.upstream.pushurl",
            "ssh://git@github.com/team/old.git",
        ],
    )
    .await;
    run(
        root,
        &["remote", "add", "origin", "https://github.com/fork/old.git"],
    )
    .await;
    run(
        root,
        &[
            "-c",
            "user.name=Example",
            "-c",
            "user.email=example@example.com",
            "commit",
            "--allow-empty",
            "-qm",
            "Initial",
        ],
    )
    .await;
    let linked = root.join("linked");
    run(
        root,
        &[
            "worktree",
            "add",
            "-qb",
            "feature",
            linked.to_str().unwrap(),
        ],
    )
    .await;
    let renames = [
        RemoteRename {
            from: "https://github.com/team/old".into(),
            to: "https://github.com/team/middle".into(),
        },
        RemoteRename {
            from: "https://github.com/team/middle".into(),
            to: "https://github.com/team/new".into(),
        },
    ];
    update(root, &renames).await.unwrap();
    update(root, &renames).await.unwrap();
    assert_eq!(
        run(root, &["remote", "get-url", "upstream"]).await,
        "git@github.com:team/new.git"
    );
    assert_eq!(
        run(root, &["remote", "get-url", "--push", "upstream"]).await,
        "ssh://git@github.com/team/new.git"
    );
    assert_eq!(
        run(root, &["remote", "get-url", "origin"]).await,
        "https://github.com/fork/old.git"
    );
    assert_eq!(
        run(&linked, &["remote", "get-url", "upstream"]).await,
        "git@github.com:team/new.git"
    );
    assert_eq!(run(&linked, &["branch", "--show-current"]).await, "feature");
    // A later clone or restored config also gets the saved renames.
    run(
        root,
        &[
            "remote",
            "set-url",
            "upstream",
            "https://github.com/team/old.git",
        ],
    )
    .await;
    update(root, &renames).await.unwrap();
    assert_eq!(
        run(root, &["remote", "get-url", "upstream"]).await,
        "https://github.com/team/new.git"
    );
}
