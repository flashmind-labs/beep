use super::{WatchError, Watched, read_watch};
use beep_proto::RunState;

const RUNNING: &str = r#"{"run_id":7,"status":"running","conclusion":null,"workflow":"CI","branch":"main","event":"push","actor":"someone","url":"","started_at":"","updated_at":"","jobs":[]}"#;

#[tokio::test]
async fn the_last_snapshot_says_how_the_run_ended() {
    let output = format!(
        "{RUNNING}\nnot json\n{}\n{}\n",
        r#"{"run_id":7,"status":"completed","conclusion":"failure","jobs":[]}"#,
        r#"{"error":true,"code":1,"message":"workflow run 7 finished: failure"}"#,
    );
    let watched = read_watch(output.as_bytes()).await;
    assert_eq!(watched.last, Some(RunState::Failed {}));
    assert!(matches!(watched.outcome(), Some(Ok(false))));
    let passed = r#"{"run_id":7,"status":"success","conclusion":"success","jobs":[]}"#;
    let watched = read_watch(passed.as_bytes()).await;
    assert!(matches!(watched.outcome(), Some(Ok(true))));
}

#[tokio::test]
async fn an_error_line_is_a_finish_only_with_code_one() {
    let finished = format!(
        "{RUNNING}\n{}\n",
        r#"{"error":true,"code":1,"message":"workflow run 7 finished: cancelled"}"#
    );
    let watched = read_watch(finished.as_bytes()).await;
    assert_eq!(
        watched.error,
        Some(WatchError {
            code: 1,
            message: "workflow run 7 finished: cancelled".into()
        })
    );
    assert!(matches!(watched.outcome(), Some(Ok(false))));
    let failed = r#"{"error":true,"code":6,"message":"API error"}"#;
    assert!(matches!(
        read_watch(failed.as_bytes()).await.outcome(),
        Some(Err(_))
    ));
    assert_eq!(read_watch(&b""[..]).await, Watched::default());
}
