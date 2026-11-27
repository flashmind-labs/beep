use super::*;
use std::time::Duration;

#[tokio::test]
async fn cancelling_and_rewatching_a_pid_keeps_the_new_registration() {
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", "read message"])
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    drop(Exit::watch(pid).unwrap());
    let replacement = Exit::watch(pid).unwrap();
    // Let the cancelled registration clean up after its replacement was installed.
    tokio::task::yield_now().await;
    drop(child.stdin.take());
    tokio::time::timeout(Duration::from_secs(5), replacement.wait())
        .await
        .unwrap();
    child.wait().await.unwrap();
}

#[tokio::test]
async fn one_kernel_registration_is_shared_and_notifies_without_a_file_write() {
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", "read message"])
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let first = Exit::watch(pid).unwrap();
    let second = Exit::watch(pid).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    drop(child.stdin.take());
    tokio::time::timeout(Duration::from_secs(5), first.wait())
        .await
        .unwrap();
    child.wait().await.unwrap();
    drop(first);
    drop(second);
    assert!(
        crate::lock(&PROCESSES)
            .get(&pid)
            .unwrap()
            .upgrade()
            .is_none()
    );
}
