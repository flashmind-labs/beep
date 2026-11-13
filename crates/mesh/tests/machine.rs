//! What `machine` makes of an app's asks and a node's answers, with no node
//! on the other end.

mod agent {
    use beep_mesh::machine::{Makes, checkout_for, node_request, queued_instead};
    use beep_proto::{
        AgentSteer, ApiError, Upload,
        node::agent::{CheckoutPlan, Request},
    };

    fn png(name: &str, size: usize) -> Upload {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(size, 0);
        Upload {
            name: name.into(),
            mime: "image/png".into(),
            data,
        }
    }

    #[test]
    fn a_named_branch_needs_a_branch_name_and_a_node_that_makes_it() {
        let named = |branch: &str| {
            Some(CheckoutPlan::NamedWorktree {
                base: Some("main".into()),
                branch: branch.into(),
            })
        };
        let new = Makes {
            checkouts: true,
            branches: true,
        };
        let plain = Makes {
            checkouts: true,
            branches: false,
        };
        assert!(
            checkout_for(new, "box", named("issue-52-crash"))
                .unwrap()
                .is_some()
        );
        for bad in [
            "", "-b", "a..b", "HEAD", "x~1", "a/.b", "a.lock", "a//b", "a b",
        ] {
            assert!(
                matches!(
                    checkout_for(new, "box", named(bad)),
                    Err(ApiError::BadRequest { .. })
                ),
                "{bad:?} is refused"
            );
        }
        let Err(ApiError::BadRequest { message }) = checkout_for(plain, "old-box", named("ok"))
        else {
            panic!("a bad request");
        };
        assert!(message.contains("old-box is too old"), "{message}");
    }

    #[test]
    fn a_steer_goes_to_the_node_and_falls_back_to_a_prompt() {
        let steer = node_request(AgentSteer::Steer {
            text: "Use the fixture instead".into(),
            images: vec![png("shot.png", 1024)],
        })
        .unwrap();
        assert!(matches!(&steer, Request::Steer { images, .. } if images.len() == 1));
        assert!(matches!(
            queued_instead(&steer),
            Some(Request::PromptImages { text, images }) if text == "Use the fixture instead" && images.len() == 1
        ));
        assert_eq!(queued_instead(&Request::Cancel {}), None);
        let empty = node_request(AgentSteer::Steer {
            text: " ".into(),
            images: Vec::new(),
        });
        assert!(empty.is_err(), "nothing to send");
    }
}

mod listing {
    use beep_mesh::machine::{Host, entries_for, external_session, past_session};
    use beep_proto::{
        AgentEntryKind, AgentStatus,
        node::agent::{self, ExternalSession, Presence},
    };

    #[test]
    fn project_history_keeps_older_agents_from_other_machines() {
        use beep_mesh::machine::listing::{MAX_PROJECT_SESSIONS, project_history};
        use std::time::{Duration, UNIX_EPOCH};

        let host = Host {
            id: "machine-a".into(),
            name: "build-box".into(),
        };
        let mut codex: Vec<_> = (0..MAX_PROJECT_SESSIONS)
            .map(|index| {
                past_session(
                    &host,
                    ExternalSession {
                        id: format!("codex-{index}"),
                        agent: "codex".into(),
                        last_activity: 100 + u64::try_from(index).unwrap(),
                        presence: Presence::Stopped,
                        ..ExternalSession::default()
                    },
                )
            })
            .collect();
        let other = Host {
            id: "machine-b".into(),
            name: "gpu-box".into(),
        };
        let claude = past_session(
            &other,
            ExternalSession {
                id: "claude-history".into(),
                agent: "claude".into(),
                last_activity: 1,
                presence: Presence::Stopped,
                ..ExternalSession::default()
            },
        );
        codex.insert(0, claude);
        let history = project_history(codex);
        assert_eq!(history.len(), MAX_PROJECT_SESSIONS + 1);
        assert!(
            history
                .windows(2)
                .all(|pair| pair[0].last_activity >= pair[1].last_activity)
        );
        let oldest = history.last().unwrap();
        assert_eq!(oldest.agent, "claude");
        assert_eq!(oldest.machine_id, other.id);
        assert_eq!(oldest.status, AgentStatus::Ended {});
        assert_eq!(oldest.last_activity, UNIX_EPOCH + Duration::from_secs(1));
    }

    #[test]
    fn only_an_app_that_reads_thoughts_gets_them() {
        let entries = vec![
            agent::Entry::new(agent::EntryKind::User, "go".into()),
            agent::Entry {
                tool: None,
                sent_at: None,
                kind: agent::EntryKind::Thought,
                text: "hmm".into(),
                ms: Some(4000),
            },
            agent::Entry::new(agent::EntryKind::Agent, "done".into()),
        ];
        let kinds = |thoughts| -> Vec<AgentEntryKind> {
            entries_for(entries.clone(), thoughts)
                .into_iter()
                .map(|entry| entry.kind)
                .collect()
        };
        assert_eq!(kinds(true).len(), 3);
        assert_eq!(
            kinds(false),
            [AgentEntryKind::User {}, AgentEntryKind::Agent {}]
        );
    }

    #[test]
    fn a_recent_outside_session_without_a_process_has_ended() {
        let host = Host {
            id: "m".into(),
            name: "build-box".into(),
        };
        let codex = ExternalSession {
            id: "s".into(),
            agent: "codex".into(),
            presence: Presence::Recent,
            ..ExternalSession::default()
        };
        assert_eq!(
            past_session(&host, codex.clone()).status,
            AgentStatus::Ended {}
        );
        let claude = ExternalSession {
            agent: "claude".into(),
            ..codex
        };
        assert_eq!(past_session(&host, claude).status, AgentStatus::Ended {});
    }

    #[test]
    fn live_outside_sessions_have_the_same_status_in_history_and_running() {
        let host = Host {
            id: "m".into(),
            name: "build-box".into(),
        };

        for agent in ["claude", "codex"] {
            for presence in [Presence::Working, Presence::Waiting, Presence::Running] {
                let session = ExternalSession {
                    id: "s".into(),
                    agent: agent.into(),
                    presence,
                    last_activity: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    ..ExternalSession::default()
                };
                assert_eq!(
                    past_session(&host, session.clone()).status,
                    external_session(&host, session).status
                );
            }
        }
    }

    #[tokio::test]
    async fn project_history_uses_the_owned_sessions_actual_state() {
        use beep_mesh::{
            MeshError,
            machine::{Sessions, listing::project_sessions},
        };
        use beep_proto::node::agent::{SessionInfo, SessionsReply, SessionsRequest, Status};
        use std::time::Duration;

        struct Node(Status);
        impl Sessions for Node {
            async fn sessions(
                &self,
                request: SessionsRequest,
                _: beep_mesh::machine::sessions::Reading,
                _: Duration,
            ) -> Result<SessionsReply, MeshError> {
                Ok(match request {
                    SessionsRequest::ListAll {} => {
                        let mut info = SessionInfo {
                            id: "owned".into(),
                            agent: "codex".into(),
                            cwd: "/srv/project".into(),
                            title: Some("Current title".into()),
                            ..SessionInfo::default()
                        };
                        info.state.status = self.0;
                        SessionsReply::Sessions {
                            sessions: vec![info],
                        }
                    }
                    SessionsRequest::InFolders { .. } => SessionsReply::External {
                        sessions: vec![ExternalSession {
                            id: "transcript".into(),
                            agent: "codex".into(),
                            taken_over: Some("owned".into()),
                            presence: Presence::Recent,
                            ..ExternalSession::default()
                        }],
                    },
                    _ => unreachable!("only session lists are requested"),
                })
            }
        }
        let host = Host {
            id: "m".into(),
            name: "build-box".into(),
        };
        for (state, expected) in [
            (Status::Ended, AgentStatus::Ended {}),
            (Status::Working, AgentStatus::Working {}),
            (Status::Idle, AgentStatus::Idle {}),
        ] {
            let listed = project_sessions(&Node(state), &host, vec!["/srv/project".into()]).await;
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, "owned");
            assert_eq!(listed[0].status, expected);
            assert_eq!(listed[0].title.as_deref(), Some("Current title"));
            assert!(!listed[0].external);
        }
    }

    #[test]
    fn an_outside_session_nothing_runs_has_ended() {
        let host = Host {
            id: "m".into(),
            name: "build-box".into(),
        };
        let session = |presence| ExternalSession {
            id: "s".into(),
            agent: "claude".into(),
            presence,
            ..ExternalSession::default()
        };
        let status = |presence| external_session(&host, session(presence)).status;
        assert_eq!(status(Presence::Stopped), AgentStatus::Ended {});
        assert_eq!(status(Presence::Recent), AgentStatus::Running {});
        assert_eq!(status(Presence::Unknown), AgentStatus::Running {});
    }
}

mod sessions {
    use anyhow::anyhow;
    use beep_mesh::MeshError;
    use beep_mesh::machine::Sessions;
    use beep_mesh::machine::sessions::{
        external, found_folders, harnesses, in_folders, live, models, update_harness,
    };
    use beep_proto::node::agent::{SessionsReply, SessionsRequest};
    use std::time::Duration;

    /// A node whose link is down.
    struct Down;

    impl Sessions for Down {
        async fn sessions(
            &self,
            _: SessionsRequest,
            _: beep_mesh::machine::sessions::Reading,
            _: Duration,
        ) -> Result<SessionsReply, MeshError> {
            Err(MeshError::Link(anyhow!("the node closed the stream")))
        }
    }

    #[tokio::test]
    async fn model_lookup_preserves_connection_errors() {
        let error = models(&Down, "codex").await.expect_err("link failed");
        assert!(matches!(error, MeshError::Link(_)));
        assert!(format!("{error:#}").contains("the node closed the stream"));
    }

    #[tokio::test]
    async fn a_node_that_cannot_say_has_no_harnesses() {
        assert!(harnesses(&Down, false).await.expect("no error").is_none());
        assert!(
            update_harness(&Down, "claude")
                .await
                .expect("no error")
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_older_node_is_told_to_update() {
        let said = |e: MeshError| format!("{e:#}");
        let e = external(&Down, "claude", 1, None).await.expect_err("down");
        assert!(said(e).contains("the node may need an update to list them"));
        let e = live(&Down, "s", None).await.expect_err("down");
        assert!(said(e).contains("the node may need an update for live status"));
        let e = found_folders(&Down, 1).await.expect_err("down");
        assert!(said(e).contains("the node may need an update to find them"));
        let e = in_folders(&Down, Vec::new(), 1).await.expect_err("down");
        assert!(said(e).contains("the node may need an update to list them"));
    }
}

mod terminals {
    use beep_mesh::machine::terminals::{Kept, ended_for_good, state_of};
    use beep_proto::TerminalState;
    use beep_proto::node::process::{Task, TaskId, TaskState};

    fn kept(pty: bool) -> Kept<'static> {
        Kept {
            task: 3,
            pty,
            command: "npm run dev",
        }
    }

    fn task(id: u32, command: &str, state: TaskState) -> Task {
        Task {
            id: TaskId(id),
            command: command.into(),
            state,
        }
    }

    #[test]
    fn a_terminal_is_its_task_only_while_the_command_matches() {
        let kept = kept(false);
        assert_eq!(
            state_of(&kept, &[task(3, "npm run dev", TaskState::Running)]),
            TerminalState::Running {}
        );
        assert_eq!(
            state_of(&kept, &[task(3, "cargo watch", TaskState::Running)]),
            TerminalState::Gone {}
        );
        assert_eq!(state_of(&kept, &[]), TerminalState::Gone {});
    }

    #[test]
    fn only_an_ended_interactive_terminal_is_dropped() {
        assert!(ended_for_good(&kept(true), &TerminalState::Stopped {}));
        assert!(!ended_for_good(&kept(true), &TerminalState::Offline {}));
        assert!(!ended_for_good(
            &kept(false),
            &TerminalState::Exited { code: Some(1) }
        ));
    }
}
