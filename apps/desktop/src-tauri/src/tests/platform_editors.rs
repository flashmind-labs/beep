use super::*;

#[tokio::test]
async fn failed_launch_returns_the_launcher_error() {
    let mut command = Command::new("sh");
    command.args(["-c", "printf 'Cannot launch application' >&2; exit 1"]);
    let error = launch(&mut command).await.unwrap_err();

    assert_eq!(error.to_string(), "Cannot launch application");
}

#[tokio::test]
async fn failed_launch_without_stderr_still_returns_an_error() {
    let mut command = Command::new("sh");
    command.args(["-c", "exit 7"]);
    let error = launch(&mut command).await.unwrap_err();

    assert!(error.to_string().contains("exit status: 7"));
}

#[tokio::test]
async fn successful_launch_returns_success() {
    let mut command = Command::new("sh");
    command.args(["-c", "exit 0"]);

    launch(&mut command).await.unwrap();
}
