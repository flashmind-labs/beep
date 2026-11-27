use super::*;
use crate::logs::{claude::Store, found::Stores};
use crate::occupancy::presence::Registration;
use beep_proto::SessionId;
use serde_json::json;
use uuid::Uuid;

fn registration(id: &str, cwd: &Path) -> Registration {
    serde_json::from_value(json!({
        "version": 1, "agent": "flsh", "session_id": id, "cwd": cwd, "pid": 123
    }))
    .unwrap()
}

fn refusal(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[tokio::test]
async fn a_folder_another_hold_has_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let occupancy = Occupancy::default();
    let _held = occupancy.holds().hold(folder.path()).await.unwrap();

    let refused = occupancy.reserve(folder.path()).await.err().unwrap();

    assert!(refusal(refused).contains("already runs"));
}

#[tokio::test]
async fn a_folder_a_session_of_this_node_runs_in_is_refused_until_it_ends() {
    let folder = tempfile::tempdir().unwrap();
    let occupancy = Occupancy::default();
    let running = occupancy.holds().running(folder.path().to_owned());

    let refused = occupancy.reserve(folder.path()).await.err().unwrap();
    assert!(refusal(refused).contains("close running sessions"));

    drop(running);
    occupancy.reserve(folder.path()).await.unwrap();
}

#[tokio::test]
async fn a_folder_an_open_session_runs_in_is_refused_and_a_stopped_one_admits() {
    let folder = tempfile::tempdir().unwrap();
    let occupancy = Occupancy::default();
    let connection = Uuid::new_v4();
    occupancy
        .presence()
        .register(registration("session_a", folder.path()), connection)
        .unwrap();

    let refused = occupancy.reserve(folder.path()).await.err().unwrap();
    assert!(refusal(refused).contains("external session is running"));

    occupancy
        .presence()
        .close(&SessionId::from("session_a"), connection);
    let hold = occupancy.reserve(folder.path()).await.unwrap();

    // The hold keeps new sessions out of the checkout while it lives.
    assert!(occupancy.holds().check_start(folder.path()).is_err());
    drop(hold);
    assert!(occupancy.holds().check_start(folder.path()).is_ok());
}

#[tokio::test]
async fn a_session_written_a_moment_ago_is_refused() {
    let (claude, folder) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let project = claude.path().join("projects/-checkout");
    std::fs::create_dir_all(&project).unwrap();
    let line = format!(
        r#"{{"type":"user","cwd":{},"timestamp":"{}","message":{{"role":"user","content":"hi"}}}}"#,
        serde_json::to_string(&folder.path().to_string_lossy()).unwrap(),
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    );
    std::fs::write(
        project.join("0f3c2a9e-1111-4222-8333-944455556666.jsonl"),
        format!("{line}\n"),
    )
    .unwrap();
    let occupancy = Occupancy::over(Store::locate_in(claude.path()), Stores::default());

    let refused = occupancy.reserve(folder.path()).await.err().unwrap();

    assert!(refusal(refused).contains("external session is running"));
}

#[test]
fn only_stopped_sessions_leave_a_folder_idle() {
    let session = |presence| ExternalSession {
        presence,
        ..ExternalSession::default()
    };

    assert!(idle(&[]));
    assert!(idle(&[session(Presence::Stopped)]));

    for busy in [
        Presence::Working,
        Presence::Waiting,
        Presence::Running,
        Presence::Recent,
        Presence::Unknown,
    ] {
        assert!(!idle(&[session(Presence::Stopped), session(busy)]), "{busy}");
    }

    let many = vec![session(Presence::Stopped); MAX_IN_FOLDERS];
    assert!(!idle(&many), "a folder with that many cannot be known idle");
}
