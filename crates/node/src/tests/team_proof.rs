use super::*;
use std::process::Command;

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn repo() -> TeamRepo {
    "github.com/org/repo".parse().unwrap()
}

/// A bare remote and a clone of it.
fn pair() -> (tempfile::TempDir, std::path::PathBuf, String) {
    let root = tempfile::tempdir().unwrap();
    let bare = root.path().join("remote.git");
    std::fs::create_dir(&bare).unwrap();
    run(&bare, &["init", "--bare"]);
    let clone = root.path().join("clone");
    std::fs::create_dir(&clone).unwrap();
    run(&clone, &["init"]);
    let remote = bare.to_string_lossy().into_owned();

    (root, clone, remote)
}

#[tokio::test]
async fn proof_is_pushed_seen_and_dropped() {
    let (_root, clone, remote) = pair();
    let nonce = Nonce::generate();

    assert!(!proof_seen(&clone, &remote, &nonce).await.unwrap());
    push_proof(&clone, &remote, &repo(), &nonce).await.unwrap();
    assert!(proof_seen(&clone, &remote, &nonce).await.unwrap());
    drop_proof(&clone, &remote, &nonce).await.unwrap();
    assert!(!proof_seen(&clone, &remote, &nonce).await.unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn refused_ref_is_reported() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, clone, remote) = pair();
    let hook = Path::new(&remote).join("hooks/pre-receive");
    std::fs::write(
        &hook,
        "#!/bin/sh\nwhile read old new ref; do case \"$ref\" in refs/beep/*) exit 1;; esac; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let error = push_proof(&clone, &remote, &repo(), &Nonce::generate())
        .await
        .unwrap_err();

    assert!(matches!(error, ProofError::RefRefused), "{error}");
}

#[test]
fn classify_denied_stderr() {
    let error = classify(
        ProofError::Git("remote: Permission to org/repo.git denied".into()),
        "org/repo",
    );

    assert!(matches!(error, ProofError::Denied(remote) if remote == "org/repo"));
}
