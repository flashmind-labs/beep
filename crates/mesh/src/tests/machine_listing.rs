use super::*;
use beep_proto::SessionId;

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

#[test]
fn a_listed_session_keeps_the_assistant_flag() {
    let host = Host {
        id: "machine-a".into(),
        name: "build-box".into(),
    };
    let info = SessionInfo {
        id: "a3f9c1".into(),
        agent: "codex".into(),
        assistant: true,
        ..SessionInfo::default()
    };
    assert!(beep_session(&host, info).assistant);
}

#[tokio::test]
async fn project_history_hides_background_children_only() {
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
                        key: SessionId::default(),
                        root_machine: None,
                        root: "missing-parent".into(),
                        depth: 1,
                        others: 0,
                        agent: "codex".into(),
                        mode: None,
                        background: true,
                    });
                    let mut front = child.clone();
                    front.id = "front".into();
                    front.parent = child.parent.clone().map(|link| ParentLink {
                        background: false,
                        ..link
                    });
                    Ok(SessionsReply::Sessions {
                        sessions: vec![parent, child, front],
                    })
                }
                SessionsRequest::InFoldersPage { .. } => {
                    Err(MeshError::Link(anyhow::anyhow!("unsupported request")))
                }
                SessionsRequest::InFolders { .. } => Ok(SessionsReply::External {
                    sessions: ["parent", "child", "front", "outside"]
                        .into_iter()
                        .map(|id| ExternalSession {
                            id: id.into(),
                            agent: "codex".into(),
                            cwd: "/srv/project".into(),
                            taken_over: (id != "outside").then(|| SessionId::from(id)),
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
            .map(|session| session.id.as_ref())
            .collect::<Vec<_>>(),
        ["parent", "front", "outside"]
    );
}

#[tokio::test]
async fn project_history_reads_every_metadata_page_without_transcripts() {
    use crate::MeshError;
    use crate::machine::sessions::Reading;
    use beep_proto::node::agent::{HistoryCursor, SessionPage, SessionsReply, SessionsRequest};

    struct Node;
    impl Sessions for Node {
        async fn sessions(
            &self,
            request: SessionsRequest,
            _: Reading,
            _: Duration,
        ) -> Result<SessionsReply, MeshError> {
            match request {
                SessionsRequest::ListAll {} => Ok(SessionsReply::Sessions { sessions: vec![] }),
                SessionsRequest::InFoldersPage { limit, before, .. } => {
                    let start =
                        before.map_or(0, |cursor| cursor.id.as_ref().parse::<usize>().unwrap() + 1);
                    let end = (start + usize::try_from(limit).unwrap()).min(125);
                    Ok(SessionsReply::SessionPage {
                        page: SessionPage {
                            sessions: (start..end)
                                .map(|i| ExternalSession {
                                    id: SessionId::from(i.to_string()),
                                    agent: "codex".into(),
                                    title: Some(format!("Session {i}")),
                                    last_activity: 1000 - u64::try_from(i).unwrap(),
                                    presence: Presence::Stopped,
                                    ..ExternalSession::default()
                                })
                                .collect(),
                            next: (end < 125).then(|| HistoryCursor {
                                id: SessionId::from((end - 1).to_string()),
                                agent: "codex".into(),
                                last_activity: 1000 - u64::try_from(end - 1).unwrap(),
                            }),
                        },
                    })
                }
                other => panic!("history must only request metadata: {other:?}"),
            }
        }
    }
    let host = Host {
        id: "machine-a".into(),
        name: "build-box".into(),
    };
    let listed = project_sessions(&Node, &host, vec!["/srv/project".into()]).await;
    assert_eq!(listed.len(), 125);
    assert!(listed.iter().all(|s| s.status == AgentStatus::Ended {}));
    assert_eq!(listed.last().unwrap().title.as_deref(), Some("Session 124"));
}

#[tokio::test]
async fn history_continues_after_filtered_pages_and_rejects_stuck_cursors() {
    use crate::MeshError;
    use crate::machine::sessions::Reading;
    use beep_proto::node::agent::{HistoryCursor, SessionPage, SessionsReply, SessionsRequest};

    struct Node {
        stuck: bool,
    }
    impl Sessions for Node {
        async fn sessions(
            &self,
            request: SessionsRequest,
            _: Reading,
            _: Duration,
        ) -> Result<SessionsReply, MeshError> {
            let SessionsRequest::InFoldersPage { before, .. } = request else {
                panic!("only metadata pages are expected");
            };
            let done = before.is_some() && !self.stuck;
            Ok(SessionsReply::SessionPage {
                page: SessionPage {
                    sessions: if done {
                        vec![ExternalSession {
                            id: "older".into(),
                            ..ExternalSession::default()
                        }]
                    } else {
                        vec![]
                    },
                    next: (!done).then(|| HistoryCursor {
                        last_activity: 100,
                        agent: "codex".into(),
                        id: "filtered".into(),
                    }),
                },
            })
        }
    }
    let found = project_metadata(&Node { stuck: false }, vec![], 50)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert!(
        project_metadata(&Node { stuck: true }, vec![], 50)
            .await
            .is_err()
    );
}
