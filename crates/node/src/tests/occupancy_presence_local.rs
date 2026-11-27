use super::*;
use beep_proto::node::agent::Presence;
use beep_proto::watched;

#[test]
fn an_old_listener_leaves_a_replacement_locator_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("presence.json");
    let old = publish(&path, "/srv/node/presence.sock").unwrap();
    let new = publish(&path, "/srv/node/presence.sock").unwrap();
    let before = fs::read(&path).unwrap();
    drop(old);
    assert_eq!(fs::read(&path).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    drop(new);
    assert!(!path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn an_idle_connection_stays_present_until_eof_or_cancellation() {
    for cancel in [false, true] {
        let registry = Arc::new(Registry::default());
        let mut changes = registry.subscribe();
        let (mut client, server) = tokio::net::UnixStream::pair().unwrap();
        let serving = registry.clone();
        let task = tokio::spawn(async move { connection(server, &serving).await });
        let registration = serde_json::json!({
            "version": 1, "agent": "flsh", "session_id": "session_a",
            "cwd": "/srv/project", "pid": 123
        });
        client
            .write_all(format!("{registration}\n").as_bytes())
            .await
            .unwrap();
        let mut reader = BufReader::new(client);
        let mut answer = String::new();
        timeout(HELLO_WAIT, reader.read_line(&mut answer))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&answer).unwrap()["ok"],
            true
        );
        assert_eq!(registry.snapshot()[0].presence, Presence::Running);
        watched::read_and_update(&mut changes, |_| ());
        if cancel {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            drop(reader);
            task.await.unwrap().unwrap();
        }

        timeout(HELLO_WAIT, changes.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(registry.snapshot()[0].presence, Presence::Stopped);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn oversized_registrations_are_refused_without_tracking_a_session() {
    let registry = Arc::new(Registry::default());
    let serving = registry.clone();
    let (mut client, server) = tokio::net::UnixStream::pair().unwrap();
    let task = tokio::spawn(async move { connection(server, &serving).await });
    client
        .write_all(&vec![b'x'; MAX_LINE as usize])
        .await
        .unwrap();
    assert!(timeout(HELLO_WAIT, task).await.unwrap().unwrap().is_err());
    assert!(registry.snapshot().is_empty());
}

#[test]
fn a_node_with_its_own_folder_keeps_its_own_locator() {
    let usual = crate::fs::note::default_dir().unwrap();
    assert_eq!(
        locator_path(None, Some(&usual)).unwrap(),
        locator_path(None, None).unwrap(),
        "the default folder keeps the user's locator"
    );
    assert_eq!(
        locator_path(None, None).unwrap(),
        usual.join("presence.json")
    );
    let own = Path::new("/srv/node");
    assert_eq!(
        locator_path(None, Some(own)).unwrap(),
        own.join("presence.json")
    );
    let set = Some(OsString::from("/srv/presence.json"));
    assert_eq!(
        locator_path(set, Some(own)).unwrap(),
        PathBuf::from("/srv/presence.json")
    );
    assert!(locator_path(Some("relative.json".into()), None).is_err());
}
