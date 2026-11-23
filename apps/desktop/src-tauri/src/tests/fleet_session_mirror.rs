use super::*;
use beep_proto::AgentQuestion;
use iroh::SecretKey;
use std::sync::Mutex;
use tokio::sync::oneshot;

/// One question to a machine, which the test answers.
struct Ask {
    machine: EndpointId,
    scan: bool,
    reply: oneshot::Sender<Option<Vec<MachineSession>>>,
}

struct Fake {
    online: Mutex<Vec<EndpointId>>,
    members: Arc<Mutex<Option<HashSet<EndpointId>>>>,
    asks: mpsc::UnboundedSender<Ask>,
    drawn: mpsc::UnboundedSender<Drawn>,
}

impl Fake {
    async fn ask(&self, machine: EndpointId, scan: bool) -> Option<Vec<MachineSession>> {
        let (reply, answer) = oneshot::channel();
        let _ = self.asks.send(Ask {
            machine,
            scan,
            reply,
        });
        answer.await.ok().flatten()
    }
}

impl Outside for Fake {
    fn online(&self) -> Vec<EndpointId> {
        self.online.lock().unwrap().clone()
    }

    fn members(&self) -> Option<HashSet<EndpointId>> {
        self.members.lock().unwrap().clone()
    }

    async fn list(&self, machine: EndpointId) -> Option<Vec<MachineSession>> {
        self.ask(machine, false).await
    }

    async fn scan(&self, machine: EndpointId) -> Option<Vec<MachineSession>> {
        self.ask(machine, true).await
    }

    fn drawn(&self, drawn: Drawn) {
        let _ = self.drawn.send(drawn);
    }
}

struct Rig {
    mirror: SessionMirror,
    members: Arc<Mutex<Option<HashSet<EndpointId>>>>,
    asks: mpsc::UnboundedReceiver<Ask>,
    drawn: mpsc::UnboundedReceiver<Drawn>,
}

fn rig(online: Vec<EndpointId>, kept: Vec<MachineSession>) -> Rig {
    let (asks, asked) = mpsc::unbounded_channel();
    let (drawn, told) = mpsc::unbounded_channel();
    let members = Arc::new(Mutex::new(None));
    let fake = Fake {
        online: Mutex::new(online),
        members: members.clone(),
        asks,
        drawn,
    };

    Rig {
        mirror: SessionMirror::start(fake, kept),
        members,
        asks: asked,
        drawn: told,
    }
}

fn machine(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

fn session(machine: EndpointId, id: &str, status: AgentStatus) -> MachineSession {
    MachineSession {
        reply: None,
        changes: None,
        machine_id: machine.to_string(),
        machine_name: "build-box".into(),
        id: id.into(),
        agent: "claude".into(),
        title: None,
        cwd: "/srv/project".into(),
        status,
        doing: None,
        started_at: UNIX_EPOCH,
        last_activity: UNIX_EPOCH,
        external: false,
        question: None,
        subagents: 0,
        model: None,
        assistant: false,
        account: None,
        persona: None,
    }
}

fn external(machine: EndpointId, id: &str, status: AgentStatus) -> MachineSession {
    MachineSession {
        external: true,
        ..session(machine, id, status)
    }
}

fn ids(rows: &Rows, machine: EndpointId) -> Vec<String> {
    let machine = machine.to_string();
    let mut ids: Vec<String> = rows
        .iter()
        .filter(|row| row.machine_id == machine)
        .map(|row| row.id.to_string())
        .collect();
    ids.sort();
    ids
}

async fn rows_where(mirror: &SessionMirror, ready: impl Fn(&Rows) -> bool) -> Arc<Rows> {
    let mut rows = mirror.watch();
    watched::wait_for(&mut rows, |rows| ready(rows))
        .await
        .expect("the mirror runs");
    watched::read(&rows, Arc::clone)
}

#[tokio::test]
async fn one_machines_answer_replaces_only_its_rows() {
    let (a, b) = (machine(1), machine(2));
    let kept = vec![
        session(a, "a1", AgentStatus::Running {}),
        session(b, "b1", AgentStatus::Running {}),
    ];
    let mut rig = rig(vec![a, b], kept);

    rig.mirror.changed(a);
    let ask = rig.asks.recv().await.unwrap();
    assert_eq!(ask.machine, a);
    ask.reply
        .send(Some(vec![session(a, "a2", AgentStatus::Running {})]))
        .unwrap();
    let rows = rows_where(&rig.mirror, |rows| rows.live(a)).await;

    assert_eq!(ids(&rows, a), ["a2"]);
    assert_eq!(ids(&rows, b), ["b1"]);
    assert!(!rows.live(b));
}

#[tokio::test]
async fn an_answered_question_is_gone_from_the_rows_at_once() {
    let a = machine(1);
    let mut asking = session(a, "s1", AgentStatus::Asking {});
    asking.question = Some(AgentQuestion {
        id: 7,
        text: "Run cargo test".into(),
        detail: None,
        options: Vec::new(),
    });
    let mut rig = rig(vec![a], vec![asking]);

    rig.mirror.answered(&a.to_string(), &"s1".into(), Some(7));
    let rows = rows_where(&rig.mirror, |rows| {
        rows.iter().all(|row| row.question.is_none())
    })
    .await;

    assert_eq!(rows.iter().next().unwrap().status, AgentStatus::Working {});
    assert_eq!(rig.drawn.recv().await, Some(Drawn::Session("s1".into())));
}

#[tokio::test]
async fn a_session_closed_on_this_mac_lists_as_ended_at_once() {
    let me = machine(1);
    let mut working = session(me, "s1", AgentStatus::Working {});
    working.doing = Some("Edit src/main.rs".into());
    let rig = rig(
        vec![me],
        vec![working, session(me, "s2", AgentStatus::Running {})],
    );

    rig.mirror.closed(&me.to_string(), &"s1".into());
    let rows = rows_where(&rig.mirror, |rows| {
        rows.iter().any(|row| row.status == AgentStatus::Ended {})
    })
    .await;
    let closed = rows.iter().find(|row| row.id == "s1").unwrap();
    let other = rows.iter().find(|row| row.id == "s2").unwrap();

    assert_eq!(closed.doing, None);
    assert_eq!(other.status, AgentStatus::Running {});
}

#[tokio::test]
async fn a_machine_absent_from_the_first_listing_keeps_its_kept_rows() {
    let (a, b) = (machine(1), machine(2));
    let kept = vec![
        session(a, "a1", AgentStatus::Running {}),
        session(b, "b1", AgentStatus::Running {}),
        external(b, "b2", AgentStatus::Running {}),
    ];
    let mut rig = rig(vec![a], kept);

    // A read lists and scans the one online machine.
    rig.mirror.rows();
    for _ in 0..2 {
        let ask = rig.asks.recv().await.unwrap();
        assert_eq!(ask.machine, a);
        let found = if ask.scan {
            Vec::new()
        } else {
            vec![session(a, "a3", AgentStatus::Running {})]
        };
        ask.reply.send(Some(found)).unwrap();
    }
    let rows = rows_where(&rig.mirror, |rows| rows.live(a)).await;

    assert_eq!(ids(&rows, a), ["a3"]);
    assert_eq!(ids(&rows, b), ["b1", "b2"]);
    assert_eq!(rows.machines[&b].source, Source::Kept);
}

#[tokio::test]
async fn two_changes_while_a_listing_runs_cause_one_more_listing() {
    let a = machine(1);
    let mut rig = rig(vec![a], Vec::new());

    rig.mirror.changed(a);
    let first = rig.asks.recv().await.unwrap();
    rig.mirror.changed(a);
    rig.mirror.changed(a);
    first.reply.send(Some(Vec::new())).unwrap();
    assert_eq!(rig.drawn.recv().await, Some(Drawn::Pushed(a)));

    let second = rig.asks.recv().await.unwrap();
    second.reply.send(Some(Vec::new())).unwrap();
    assert_eq!(rig.drawn.recv().await, Some(Drawn::Pushed(a)));
    // The lists that ask the machine themselves hear once, when the chain ends.
    assert_eq!(rig.drawn.recv().await, Some(Drawn::Machine(a)));

    // The mirror took every message before this edit, and asked for nothing more.
    rig.mirror.closed(&a.to_string(), &"s1".into());
    assert_eq!(rig.drawn.recv().await, Some(Drawn::Session("s1".into())));
    assert!(rig.asks.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_does_not_answer_is_asked_again_and_keeps_its_rows() {
    let a = machine(1);
    let mut rig = rig(vec![a], vec![session(a, "a1", AgentStatus::Running {})]);

    rig.mirror.changed(a);
    rig.asks.recv().await.unwrap().reply.send(None).unwrap();
    // The pause passes on the paused clock.
    let again = rig.asks.recv().await.unwrap();
    again
        .reply
        .send(Some(vec![session(a, "a2", AgentStatus::Running {})]))
        .unwrap();
    let rows = rows_where(&rig.mirror, |rows| rows.live(a)).await;

    assert_eq!(ids(&rows, a), ["a2"]);
}

#[test]
fn a_live_report_updates_its_session() {
    let me = machine(1);
    let mut rows = vec![
        external(me, "s1", AgentStatus::Running {}),
        external(me, "s2", AgentStatus::Running {}),
    ];
    let report = ExternalSession {
        id: "s1".into(),
        agent: "claude".into(),
        title: Some("Fix the build".into()),
        last_activity: 60,
        presence: Presence::Waiting,
        ..ExternalSession::default()
    };

    assert!(report_in(&mut rows, &me.to_string(), &report));

    assert_eq!(rows[0].status, AgentStatus::Waiting {});
    assert_eq!(rows[0].title.as_deref(), Some("Fix the build"));
    assert_eq!(rows[0].last_activity, UNIX_EPOCH + Duration::from_secs(60));
    assert_eq!(rows[1].status, AgentStatus::Running {});
    assert_eq!(rows[1].last_activity, UNIX_EPOCH);
}

#[test]
fn a_report_without_presence_or_from_another_machine_changes_nothing() {
    let (me, other) = (machine(1), machine(2));
    let mut rows = vec![external(me, "s1", AgentStatus::Running {})];
    let unknown = ExternalSession {
        id: "s1".into(),
        agent: "claude".into(),
        ..ExternalSession::default()
    };

    assert!(!report_in(&mut rows, &me.to_string(), &unknown));
    assert!(!report_in(
        &mut rows,
        &other.to_string(),
        &ExternalSession {
            presence: Presence::Stopped,
            ..unknown
        },
    ));
    assert_eq!(rows[0].status, AgentStatus::Running {});
}

#[tokio::test]
async fn an_answer_during_a_listing_does_not_come_back() {
    let a = machine(1);
    let mut asking = session(a, "s1", AgentStatus::Asking {});
    asking.question = Some(AgentQuestion {
        id: 7,
        text: "Run cargo test".into(),
        detail: None,
        options: Vec::new(),
    });
    let mut rig = rig(vec![a], vec![asking.clone()]);

    rig.mirror.changed(a);
    let first = rig.asks.recv().await.unwrap();
    rig.mirror.answered(&a.to_string(), &"s1".into(), Some(7));
    rows_where(&rig.mirror, |rows| {
        rows.iter().all(|row| row.question.is_none())
    })
    .await;
    // The machine answered before it heard of the answer.
    first.reply.send(Some(vec![asking])).unwrap();

    let second = rig.asks.recv().await.unwrap();
    second
        .reply
        .send(Some(vec![session(a, "s1", AgentStatus::Working {})]))
        .unwrap();
    let rows = rows_where(&rig.mirror, |rows| rows.live(a)).await;

    assert!(rows.iter().all(|row| row.question.is_none()));
}

#[test]
fn a_machine_that_is_off_shows_no_rows_until_it_is_back() {
    let (online, off, connecting, me) = (machine(1), machine(2), machine(3), machine(4));
    let rows = Rows::kept(vec![
        session(online, "a", AgentStatus::Running {}),
        session(off, "b", AgentStatus::Running {}),
        session(connecting, "c", AgentStatus::Running {}),
        session(me, "d", AgentStatus::Running {}),
    ]);
    let shown = |online: &[EndpointId]| {
        let shown = showing(online, &[connecting], me);
        let mut ids: Vec<String> = rows
            .iter_of(Some(&shown))
            .map(|row| row.id.to_string())
            .collect();
        ids.sort();
        ids
    };

    // A connecting machine keeps showing its kept rows, so does this Mac.
    assert_eq!(shown(&[online]), ["a", "c", "d"]);
    assert_eq!(shown(&[online, off]), ["a", "b", "c", "d"]);
    assert_eq!(rows.iter_of(None).count(), 4);
}

#[tokio::test]
async fn a_machine_that_left_the_mesh_is_dropped_from_the_rows() {
    let (a, b) = (machine(1), machine(2));
    let rig = rig(
        vec![],
        vec![
            session(a, "a1", AgentStatus::Running {}),
            session(b, "b1", AgentStatus::Running {}),
        ],
    );

    // Until the mesh opens nothing is dropped.
    rig.mirror.rows();
    assert_eq!(rig.mirror.rows().iter().count(), 2);

    *rig.members.lock().unwrap() = Some(HashSet::from([a]));
    rig.mirror.rows();
    let rows = rows_where(&rig.mirror, |rows| rows.machines.len() == 1).await;

    assert_eq!(ids(&rows, a), ["a1"]);
}

#[tokio::test(start_paused = true)]
async fn every_caller_of_the_launch_waits_for_this_macs_first_listing() {
    let me = machine(1);
    let mut rig = rig(vec![me], Vec::new());

    let driver = async {
        let ask = rig.asks.recv().await.unwrap();
        ask.reply
            .send(Some(vec![session(me, "s1", AgentStatus::Running {})]))
            .unwrap();
    };
    let (tray, window, ()) =
        tokio::join!(rig.mirror.rows_live(me), rig.mirror.rows_live(me), driver,);

    assert!(tray.live(me));
    assert!(window.live(me));
}

#[tokio::test(start_paused = true)]
async fn at_most_four_machines_are_listed_at_once() {
    let machines: Vec<EndpointId> = (1..=6).map(machine).collect();
    let mut rig = rig(machines, Vec::new());

    rig.mirror.rows();
    // Four listings and four scans start, the rest wait for a slot.
    let mut lists = Vec::new();
    for _ in 0..8 {
        let ask = rig.asks.recv().await.unwrap();
        if !ask.scan {
            lists.push(ask);
        }
    }
    tokio::task::yield_now().await;
    assert_eq!(lists.len(), 4);
    assert!(rig.asks.try_recv().is_err());

    lists.pop().unwrap().reply.send(None).unwrap();
    assert!(!rig.asks.recv().await.unwrap().scan);
}

#[test]
fn the_views_file_takes_rows_that_differ_in_what_is_drawn() {
    let me = machine(1);
    let old = vec![session(me, "s1", AgentStatus::Running {})];
    let mut moved = old.clone();
    moved[0].last_activity = UNIX_EPOCH + Duration::from_secs(5);
    let mut ended = old.clone();
    ended[0].status = AgentStatus::Ended {};

    assert!(!drawn_between(&old, &moved));
    assert!(drawn_between(&old, &ended));
    assert!(drawn_between(&old, &[]));
}
