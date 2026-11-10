use super::*;
use std::time::SystemTime;

fn external(id: &str, status: AgentStatus) -> MachineSession {
    MachineSession {
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        id: id.into(),
        agent: "claude".into(),
        title: None,
        cwd: "/srv/project".into(),
        status,
        doing: None,
        started_at: UNIX_EPOCH,
        last_activity: UNIX_EPOCH,
        external: true,
        question: None,
    }
}

fn listed(sessions: Vec<MachineSession>) -> AgentSessions {
    AgentSessions {
        last: Mutex::new(Some(Listed {
            at: Some(Instant::now()),
            sessions,
        })),
        asking: Arc::new(AsyncMutex::new(())),
    }
}

#[test]
fn a_live_report_updates_its_session_and_keeps_the_list_fresh() {
    let sessions = listed(vec![
        external("s1", AgentStatus::Running {}),
        external("s2", AgentStatus::Running {}),
    ]);
    let report = ExternalSession {
        id: "s1".into(),
        agent: "claude".into(),
        title: Some("Fix the build".into()),
        last_activity: 60,
        presence: Presence::Waiting,
        ..ExternalSession::default()
    };

    sessions.live("build-box", &report);

    let last = sessions.last().clone().unwrap();
    assert!(last.at.is_some());
    assert_eq!(last.sessions[0].status, AgentStatus::Waiting {});
    assert_eq!(last.sessions[0].title.as_deref(), Some("Fix the build"));
    assert_eq!(
        last.sessions[0].last_activity,
        UNIX_EPOCH + Duration::from_secs(60)
    );
    assert_eq!(last.sessions[1].status, AgentStatus::Running {});
    assert!(last.sessions[1].last_activity < SystemTime::now());
}

#[test]
fn a_report_without_presence_or_from_another_machine_changes_nothing() {
    let sessions = listed(vec![external("s1", AgentStatus::Running {})]);
    let unknown = ExternalSession {
        id: "s1".into(),
        agent: "claude".into(),
        ..ExternalSession::default()
    };
    sessions.live("build-box", &unknown);
    sessions.live(
        "gpu-box",
        &ExternalSession {
            presence: Presence::Stopped,
            ..unknown.clone()
        },
    );

    let last = sessions.last().clone().unwrap();
    assert_eq!(last.sessions[0].status, AgentStatus::Running {});
}
