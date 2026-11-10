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
        subagents: 0,
    }
}

fn listed(sessions: Vec<MachineSession>) -> AgentSessions {
    AgentSessions {
        last: Mutex::new(Some(Listed {
            age: Age::Listed(Instant::now()),
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
    assert!(matches!(last.age, Age::Listed(_)));
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

#[test]
fn a_change_marks_a_listed_list_stale_and_leaves_one_from_disk_drawing_at_once() {
    let sessions = listed(vec![external("s1", AgentStatus::Running {})]);
    sessions.stale();
    assert_eq!(sessions.last().as_ref().map(|l| l.age), Some(Age::Stale));

    if let Some(listed) = sessions.last().as_mut() {
        listed.age = Age::Saved;
    }
    sessions.stale();
    assert_eq!(sessions.last().as_ref().map(|l| l.age), Some(Age::Saved));
}
