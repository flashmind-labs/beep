use super::*;

#[test]
fn native_open_and_close_override_a_stale_scan_without_duplicating_owned_sessions() {
    let host = Host {
        id: "machine-a".into(),
        name: "build-box".into(),
    };
    let mut native = ExternalSession {
        id: "session_a".into(),
        agent: "flsh".into(),
        cwd: "/srv/project".into(),
        presence: Presence::Running,
        ..ExternalSession::default()
    };
    let mut sessions = vec![];
    apply_presence(&host, &[native.clone()], &mut sessions);
    assert_eq!(sessions.len(), 1);
    sessions[0].title = Some("Stored title".into());
    apply_presence(&host, &[native.clone()], &mut sessions);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title.as_deref(), Some("Stored title"));
    native.presence = Presence::Stopped;
    apply_presence(&host, &[native.clone()], &mut sessions);
    assert!(sessions.is_empty());
    native.presence = Presence::Running;
    native.taken_over = Some("owned-session".into());
    apply_presence(&host, &[native], &mut sessions);
    assert!(sessions.is_empty());
}

#[test]
fn an_idle_session_with_running_subagents_lists_as_working() {
    let host = Host {
        id: "machine-a".into(),
        name: "build-box".into(),
    };
    let mut info = SessionInfo {
        id: "a3f9c1".into(),
        agent: "codex".into(),
        ..SessionInfo::default()
    };
    info.state.status = Status::Idle;
    info.state.subagents = 1;
    let view = beep_view(&host, info.clone());
    assert_eq!(view.session.status, AgentStatus::Working {});

    info.state.subagents = 0;
    assert_eq!(beep_session(&host, info).status, AgentStatus::Idle {});
}
