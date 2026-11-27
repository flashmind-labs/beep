use super::*;

fn kind(log: &Path) -> AgentKind {
    AgentKind {
        name: "test-agent".into(),
        program: "python3".into(),
        args: vec![
            concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/fake_acp_v2.py").into(),
            log.display().to_string(),
        ],
        env: [("BEEP_ACP_VERSION".into(), "2".into())].into(),
        compact: Some(AutoCompact {
            enabled: false,
            percent: None,
            tokens: None,
        }),
        compact_models: Vec::new(),
        default_model: None,
        bridge: None,
    }
}

#[tokio::test]
async fn v2_acceptance_does_not_end_the_turn_and_cancel_waits_for_idle() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let kind = kind(&log);
    let session = Session::start(Start {
        begin: Begin {
            prompt: Some(String::from("wait")),
            ..begun(None)
        },
        ..start_on(&kind, temp.path(), None)
    })
    .unwrap();
    until(&session, |live| {
        live.info.question.is_some() || live.status() == Status::Ended
    })
    .await;
    assert_eq!(
        session.info().state.status,
        Status::Asking,
        "{:?}",
        session.info().state.error
    );
    assert_eq!(
        session.info().turns,
        0,
        "acceptance must not finish the turn"
    );
    let entries = watched::read(&session.live, |live| live.transcript(10).0);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::User)
            .count(),
        1
    );
    session
        .order(Command::Cancel { by_user: true })
        .await
        .unwrap();
    until(&session, |live| {
        live.info.state.last_stop == Some(StopReason::Cancelled) || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().state.status, Status::Idle);
    assert_eq!(session.info().turns, 1);
    assert!(session.info().question.is_none());
}

#[tokio::test]
async fn v2_late_acceptance_and_duplicate_idle_do_not_repeat_a_turn_or_usage() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let kind = kind(&log);
    let session = Session::start(Start {
        begin: Begin {
            prompt: Some(String::from("late")),
            ..begun(None)
        },
        ..start_on(&kind, temp.path(), None)
    })
    .unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
    assert_eq!(session.info().tokens, 7);
    assert_eq!(session.info().last_message, "Done");
    let entries = watched::read(&session.live, |live| live.transcript(10).0);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::User)
            .count(),
        1
    );
    session
        .order(Command::Prompt(String::from("go").into()))
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 2 || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().turns, 2);
    assert_eq!(session.info().tokens, 14);
}

#[tokio::test]
async fn v2_replays_with_resume_and_configures_modes_and_models() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let kind = kind(&log);
    let session = Session::start(Start {
        begin: Begin {
            resume: Some("session-test".into()),
            mode: Some("plan".into()),
            ..begun(Some("other"))
        },
        ..start_on(&kind, temp.path(), Some("other"))
    })
    .unwrap();
    settle(&session).await;
    let info = session.info();
    assert_eq!(info.turns, 1, "{:?}", info.state.error);
    assert_eq!(info.mode.as_deref(), Some("plan"));
    assert_eq!(info.model.as_ref().map(AsRef::as_ref), Some("other"));
    let requests = sent(&log);
    assert_eq!(requests[1]["method"], "session/resume");
    assert_eq!(requests[1]["params"]["replayFrom"]["type"], "start");
    assert!(
        requests
            .iter()
            .all(|request| request["method"] != "session/set_mode")
    );
    let entries = watched::read(&session.live, |live| live.transcript(10).0);
    assert!(entries.iter().any(|entry| entry.text == "earlier prompt"));
    session
        .order(Command::SetMode("agent".into()))
        .await
        .unwrap();
    until(&session, |live| {
        live.info.mode.as_deref() == Some("agent") || live.status() == Status::Ended
    })
    .await;
}

#[tokio::test]
async fn v2_failures_and_unknown_stop_reasons_still_finish() {
    for (prompt, stop, error) in [
        ("error", StopReason::Failed, Some("work failed")),
        ("reject", StopReason::Failed, Some("rejected input")),
        ("unknown", StopReason::Other, None),
        ("missing", StopReason::Other, None),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("requests.jsonl");
        let kind = kind(&log);
        let session = Session::start(Start {
            begin: Begin {
                prompt: Some(String::from(prompt)),
                ..begun(None)
            },
            ..start_on(&kind, temp.path(), None)
        })
        .unwrap();
        settle(&session).await;
        assert_eq!(
            session.info().state.status,
            Status::Idle,
            "{prompt}: {:?}",
            session.info().state.error
        );
        assert_eq!(
            watched::read(&session.live, |live| live.info.state.last_stop),
            Some(stop),
            "{prompt}"
        );
        if let Some(error) = error {
            assert!(
                session
                    .info()
                    .state
                    .error
                    .as_deref()
                    .unwrap()
                    .contains(error)
            );
        }
    }
}

#[tokio::test]
async fn v2_opt_in_negotiates_down_to_a_v1_agent() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let mut kind = script(
        "test-agent",
        r#"
handshake "$1"
read -r line; log "$1"
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#,
        &log,
    );
    kind.env.insert("BEEP_ACP_VERSION".into(), "2".into());
    let session = Session::start(start_on(&kind, temp.path(), None)).unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
    assert_eq!(sent(&log)[0]["params"]["protocolVersion"], 2);
}

#[tokio::test]
async fn a_v2_resume_snapshot_keeps_queued_input_behind_active_work() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let kind = kind(&log);
    let session = Session::start(Start {
        begin: Begin {
            resume: Some("active-session".into()),
            ..begun(None)
        },
        ..start_on(&kind, temp.path(), None)
    })
    .unwrap();
    until(&session, |live| {
        (live.status() == Status::Working && live.info.state.queued == 1)
            || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().state.status, Status::Working);
    assert_eq!(session.info().turns, 0);
    assert_eq!(session.info().state.queued, 1);
    assert!(
        sent(&log)
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
    session
        .order(Command::Cancel { by_user: true })
        .await
        .unwrap();
    until(&session, |live| {
        live.info.state.last_stop == Some(StopReason::Cancelled) || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().turns, 1);
    session
        .order(Command::Prompt(String::from("go").into()))
        .await
        .unwrap();
    until(&session, |live| {
        live.info.turns >= 2 || live.status() == Status::Ended
    })
    .await;
    assert_eq!(session.info().turns, 2);
}

#[tokio::test]
async fn an_explicit_v1_override_uses_legacy_initialization() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let mut kind = script(
        "test-agent",
        r#"
handshake "$1"
read -r line; log "$1"
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#,
        &log,
    );
    kind.env.insert("BEEP_ACP_VERSION".into(), "1".into());
    let session = Session::start(start_on(&kind, temp.path(), None)).unwrap();
    settle(&session).await;
    assert_eq!(session.info().turns, 1, "{:?}", session.info().state.error);
    let requests = sent(&log);
    assert_eq!(requests[0]["params"]["protocolVersion"], 1);
    assert!(requests[0]["params"].get("capabilities").is_none());
    assert_eq!(
        requests[0]["params"]["clientCapabilities"]["session"]["compaction"],
        serde_json::json!({})
    );
}

#[tokio::test]
async fn a_finished_turn_counts_the_lines_its_checkout_changed() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let repo = changed_repo(temp.path());
    let kind = kind(&log);
    let session = Session::start(Start {
        begin: Begin {
            prompt: Some(String::from("late")),
            ..begun(None)
        },
        ..start_on(&kind, &repo, None)
    })
    .unwrap();
    settle(&session).await;
    until(&session, |live| live.info.changes.is_some()).await;

    assert_eq!(session.info().changes, Some(THREE_TWO));
}

/// A session resumed after the node restarts counts before its next turn.
#[tokio::test]
async fn a_session_counts_its_checkout_when_it_starts() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("requests.jsonl");
    let repo = changed_repo(temp.path());
    let kind = kind(&log);
    let session = Session::start(Start {
        begin: Begin {
            prompt: None,
            ..begun(None)
        },
        ..start_on(&kind, &repo, None)
    })
    .unwrap();
    until(&session, |live| live.info.changes.is_some()).await;

    assert_eq!(session.info().changes, Some(THREE_TWO));
}

const THREE_TWO: beep_proto::node::agent::LineChanges = beep_proto::node::agent::LineChanges {
    additions: 3,
    deletions: 2,
};

/// A repository with one line changed, one removed and a new file of two.
fn changed_repo(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.test"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(done.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("list.txt"), "a\nb\nc\n").unwrap();
    git(&["add", "list.txt"]);
    git(&["commit", "-q", "-m", "start"]);
    std::fs::write(repo.join("list.txt"), "a\nB\n").unwrap();
    std::fs::write(repo.join("new.txt"), "x\ny\n").unwrap();
    repo
}
