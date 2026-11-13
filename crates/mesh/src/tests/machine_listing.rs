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

#[tokio::test]
async fn project_history_hides_children_but_keeps_the_parent() {
    use crate::MeshError;
    use crate::machine::sessions::Reading;
    use beep_proto::node::agent::{SessionsReply, SessionsRequest};

    struct Node;

    impl Sessions for Node {
        async fn sessions(
            &self,
            request: SessionsRequest,
            _: Reading,
            _: Duration,
        ) -> Result<SessionsReply, MeshError> {
            match request {
                SessionsRequest::ListAll {} => {
                    let parent = SessionInfo {
                        id: "parent".into(),
                        agent: "codex".into(),
                        ..SessionInfo::default()
                    };
                    let mut child = parent.clone();
                    child.id = "child".into();
                    child.parent = Some(ParentLink {
                        machine: None,
                        session: "missing-parent".into(),
                        key: String::new(),
                        root_machine: None,
                        root: "missing-parent".into(),
                        depth: 1,
                        others: 0,
                        agent: "codex".into(),
                        mode: None,
                    });
                    Ok(SessionsReply::Sessions {
                        sessions: vec![parent, child],
                    })
                }
                SessionsRequest::InFolders { .. } => Ok(SessionsReply::External {
                    sessions: ["parent", "child", "outside"]
                        .into_iter()
                        .map(|id| ExternalSession {
                            id: id.into(),
                            agent: "codex".into(),
                            cwd: "/srv/project".into(),
                            taken_over: (id != "outside").then(|| id.to_owned()),
                            ..ExternalSession::default()
                        })
                        .collect(),
                }),
                other => panic!("unexpected request: {other:?}"),
            }
        }
    }

    let host = Host {
        id: "machine-a".into(),
        name: "build-box".into(),
    };
    let sessions = project_sessions(&Node, &host, vec!["/srv/project".into()]).await;
    assert_eq!(
        sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        ["parent", "outside"]
    );
}
