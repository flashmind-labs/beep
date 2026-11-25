use super::*;
use beep_mesh::machine::listing::Host;
use beep_proto::{AgentQuestion, node::agent::ParentLink};
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

    fn me(&self) -> EndpointId {
        mac()
    }

    fn members(&self) -> Option<HashSet<EndpointId>> {
        self.members.lock().unwrap().clone()
    }

    async fn list(&self, machine: EndpointId) -> Option<Vec<Linked>> {
        let rows = self.ask(machine, false).await?;

        Some(rows.into_iter().map(top).collect())
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

/// A session no agent started.
fn top(session: MachineSession) -> Linked {
    Linked {
        session,
        parent: None,
        reviewer: false,
    }
}

/// This Mac, as the fake outside names it.
fn mac() -> EndpointId {
    machine(0)
}

/// The next question to a machine; a test that waits for one that never
/// comes fails instead of hanging.
async fn next(asks: &mut mpsc::UnboundedReceiver<Ask>) -> Ask {
    timeout(Duration::from_secs(5), asks.recv())
        .await
        .expect("a machine is asked")
        .expect("the mirror runs")
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
        place: None,
        continues: None,
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
    timeout(
        Duration::from_secs(5),
        watched::wait_for(&mut rows, |rows| ready(rows)),
    )
    .await
    .expect("the rows come")
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

    rig.mirror.moved(a);
    let ask = next(&mut rig.asks).await;
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
        let ask = next(&mut rig.asks).await;
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
async fn a_push_replaces_the_rows_without_asking_the_machine() {
    let a = machine(1);
    let mut rig = rig(vec![a], vec![session(a, "a1", AgentStatus::Running {})]);

    rig.mirror
        .pushed(a, vec![top(session(a, "a2", AgentStatus::Running {}))]);
    rows_where(&rig.mirror, |rows| rows.live(a)).await;
    let rows = rig.mirror.current();

    assert_eq!(ids(&rows, a), ["a2"]);
    assert!(rows.live(a));

    // The mirror took every message before this edit, and asked for nothing.
    rig.mirror.closed(&a.to_string(), &"s1".into());
    assert_eq!(rig.drawn.recv().await, Some(Drawn::Session("s1".into())));
    assert!(rig.asks.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_pushes_is_not_listed_again_when_its_rows_age() {
    let a = machine(1);
    let mut rig = rig(vec![a], Vec::new());

    rig.mirror
        .pushed(a, vec![top(session(a, "a1", AgentStatus::Running {}))]);
    rig.mirror.rows();

    // Only the first read scans.
    let ask = next(&mut rig.asks).await;
    assert!(ask.scan);
    ask.reply.send(Some(Vec::new())).unwrap();
    rows_where(&rig.mirror, |rows| rows.live(a)).await;
    tokio::time::sleep(FRESH * 2).await;
    rig.mirror.rows();
    tokio::task::yield_now().await;
    assert!(rig.asks.try_recv().is_err());

    // A push of what the scan reads scans again.
    rig.mirror.rescan(a);
    assert!(next(&mut rig.asks).await.scan);
}

#[tokio::test(start_paused = true)]
async fn a_waiting_outside_session_leaves_when_its_day_ends_though_nothing_is_pushed() {
    let a = machine(1);
    let mut rig = rig(vec![a], Vec::new());
    rig.mirror.pushed(a, Vec::new());
    rig.mirror.rows();
    let mut waiting = external(a, "x", AgentStatus::Waiting {});
    waiting.last_activity = SystemTime::now() - IDLE_SHOWN + Duration::from_secs(60);
    let ask = next(&mut rig.asks).await;
    ask.reply.send(Some(vec![waiting])).unwrap();
    rows_where(&rig.mirror, |rows| ids(rows, a) == ["x"]).await;

    // Nothing was pushed; the minute passes and the machine is scanned again.
    tokio::time::sleep(Duration::from_secs(70)).await;
    let ask = next(&mut rig.asks).await;
    assert!(ask.scan);
    ask.reply.send(Some(Vec::new())).unwrap();
    rows_where(&rig.mirror, |rows| ids(rows, a).is_empty()).await;
}

#[tokio::test(start_paused = true)]
async fn a_stale_machine_does_not_make_a_pushing_one_listed() {
    let (a, b) = (machine(1), machine(2));
    let mut rig = rig(vec![a, b], Vec::new());

    for m in [a, b] {
        rig.mirror
            .pushed(m, vec![top(session(m, "s", AgentStatus::Running {}))]);
    }
    rig.mirror.rows();
    // Both are scanned once. A scan under way is not started again, so wait
    // until the mirror has taken both answers: each adds the row `x`.
    for _ in 0..2 {
        let ask = next(&mut rig.asks).await;
        assert!(ask.scan);
        let found = vec![external(ask.machine, "x", AgentStatus::Running {})];
        ask.reply.send(Some(found)).unwrap();
    }
    rows_where(&rig.mirror, |rows| {
        [a, b]
            .iter()
            .all(|m| ids(rows, *m).contains(&"x".to_string()))
    })
    .await;

    rig.mirror.stale(a);
    rig.mirror.rows();
    let first = next(&mut rig.asks).await;
    let second = next(&mut rig.asks).await;
    assert_eq!([first.machine, second.machine], [a, a]);
    tokio::task::yield_now().await;
    assert!(rig.asks.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn a_machine_whose_push_ended_is_listed_again_when_its_rows_age() {
    let a = machine(1);
    let mut rig = rig(vec![a], Vec::new());

    rig.mirror
        .pushed(a, vec![top(session(a, "a1", AgentStatus::Running {}))]);
    rig.mirror.unpushed(a);
    tokio::time::sleep(FRESH * 2).await;
    rig.mirror.rows();

    let first = next(&mut rig.asks).await;
    let second = next(&mut rig.asks).await;
    assert!(first.scan != second.scan);
}

#[tokio::test]
async fn a_push_during_a_listing_is_not_undone_by_the_older_answer() {
    let a = machine(1);
    let mut rig = rig(vec![a], Vec::new());

    rig.mirror.moved(a);
    let first = next(&mut rig.asks).await;
    rig.mirror
        .pushed(a, vec![top(session(a, "new", AgentStatus::Running {}))]);
    rows_where(&rig.mirror, |rows| rows.live(a)).await;
    // The listing was asked before the push and answers with the older rows.
    first
        .reply
        .send(Some(vec![session(a, "old", AgentStatus::Running {})]))
        .unwrap();

    let second = next(&mut rig.asks).await;
    second
        .reply
        .send(Some(vec![session(a, "new", AgentStatus::Running {})]))
        .unwrap();

    assert_eq!(ids(&rig.mirror.current(), a), ["new"]);
}

#[tokio::test]
async fn children_and_reviewers_stay_out_of_the_rows_and_are_edited_with_them() {
    let a = machine(1);
    let rig = rig(vec![a], Vec::new());
    let child = Linked {
        session: session(a, "kid", AgentStatus::Working {}),
        parent: Some(ParentLink {
            machine: None,
            session: "parent".into(),
            key: "parent".into(),
            root_machine: None,
            root: "parent".into(),
            depth: 1,
            others: 0,
            agent: "claude".into(),
            mode: None,
            background: true,
        }),
        reviewer: false,
    };
    let reviewer = Linked {
        session: session(a, "review", AgentStatus::Working {}),
        parent: None,
        reviewer: true,
    };

    rig.mirror.pushed(
        a,
        vec![
            top(session(a, "top", AgentStatus::Running {})),
            child,
            reviewer,
        ],
    );
    let rows = rows_where(&rig.mirror, |rows| rows.live(a)).await;
    let machine_id = a.to_string();

    assert_eq!(ids(&rows, a), ["top"]);
    assert_eq!(rows.children_of(None).count(), 2);
    assert!(rows.link(&machine_id, &"review".into()).unwrap().reviewer);
    let parent = rows
        .link(&machine_id, &"kid".into())
        .and_then(|link| link.parent(&machine_id))
        .unwrap();
    assert_eq!(parent.id, "parent");
    assert_eq!(parent.machine_id, machine_id);

    // A delete takes a child with the rows.
    rig.mirror.deleted(&machine_id, &"kid".into());
    let rows = rows_where(&rig.mirror, |rows| rows.children_of(None).count() == 1).await;
    assert!(rows.link(&machine_id, &"kid".into()).is_none());
}

#[tokio::test]
async fn native_reports_win_over_the_scans_that_come_after_them() {
    let a = machine(1);
    let mut rig = rig(vec![a], Vec::new());
    let host = Host {
        id: a.to_string(),
        name: "build-box".into(),
    };
    let report = |presence| ExternalSession {
        id: "ext".into(),
        agent: "claude".into(),
        cwd: "/srv/project".into(),
        presence,
        ..ExternalSession::default()
    };

    rig.mirror
        .presence(a, host.clone(), vec![report(Presence::Working)]);
    assert_eq!(rig.drawn.recv().await, Some(Drawn::Listed));
    assert_eq!(ids(&rig.mirror.current(), a), ["ext"]);

    // A scan that does not find the session leaves it: the report is newer.
    rig.mirror.stale(a);
    rig.mirror.rows();
    let scan = loop {
        let ask = next(&mut rig.asks).await;

        if ask.scan {
            break ask;
        }

        ask.reply.send(Some(Vec::new())).unwrap();
    };
    scan.reply.send(Some(Vec::new())).unwrap();
    rig.mirror.stale(a);
    // The mirror took the scan before this report, which ends the session.
    rig.mirror
        .presence(a, host, vec![report(Presence::Stopped)]);
    let rows = rows_where(&rig.mirror, |rows| rows.iter().count() == 0).await;

    assert!(ids(&rows, a).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_does_not_answer_is_asked_again_and_keeps_its_rows() {
    let a = machine(1);
    let mut rig = rig(vec![a], vec![session(a, "a1", AgentStatus::Running {})]);

    rig.mirror.moved(a);
    next(&mut rig.asks).await.reply.send(None).unwrap();
    // The pause passes on the paused clock.
    let again = next(&mut rig.asks).await;
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

    rig.mirror.moved(a);
    let first = next(&mut rig.asks).await;
    rig.mirror.answered(&a.to_string(), &"s1".into(), Some(7));
    rows_where(&rig.mirror, |rows| {
        rows.iter().all(|row| row.question.is_none())
    })
    .await;
    // The machine answered before it heard of the answer.
    first.reply.send(Some(vec![asking])).unwrap();

    let second = next(&mut rig.asks).await;
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
    let mut rig = rig(
        vec![],
        vec![
            session(a, "a1", AgentStatus::Running {}),
            session(b, "b1", AgentStatus::Running {}),
        ],
    );

    // Until the mesh opens nothing is dropped: the read came before the
    // change, whose listing starts only after the mirror took the read.
    rig.mirror.rows();
    rig.mirror.moved(a);
    let ask = next(&mut rig.asks).await;
    assert_eq!(rig.mirror.rows().iter().count(), 2);
    drop(ask);

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
        let ask = next(&mut rig.asks).await;
        ask.reply
            .send(Some(vec![session(me, "s1", AgentStatus::Running {})]))
            .unwrap();
    };
    let (tray, window, ()) = tokio::join!(
        rig.mirror.launch_rows(me, true),
        rig.mirror.launch_rows(me, true),
        driver,
    );

    assert!(tray.live(me));
    assert!(window.live(me));
}

#[tokio::test(start_paused = true)]
async fn callers_stop_waiting_for_this_mac_at_the_end_of_the_launch_wait() {
    let me = machine(1);
    let mut rig = rig(vec![me], Vec::new());
    let started = Instant::now();

    let hold = async { next(&mut rig.asks).await };
    let (tray, window, _unanswered) = tokio::join!(
        rig.mirror.launch_rows(me, true),
        rig.mirror.launch_rows(me, true),
        hold,
    );

    assert!(!tray.live(me) && !window.live(me));
    assert_eq!(started.elapsed(), LOCAL_WAIT);

    // A caller after the wait does not wait at all.
    let late = Instant::now();
    rig.mirror.launch_rows(me, true).await;
    assert_eq!(late.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn at_most_four_machines_are_listed_at_once() {
    let machines: Vec<EndpointId> = (1..=6).map(machine).collect();
    let mut rig = rig(machines, Vec::new());

    rig.mirror.rows();
    // Four listings and four scans start, the rest wait for a slot.
    let (mut lists, mut scans) = (Vec::new(), Vec::new());
    for _ in 0..8 {
        let ask = next(&mut rig.asks).await;
        // An ask left unanswered holds its slot; a dropped one frees it.
        if ask.scan {
            scans.push(ask);
        } else {
            lists.push(ask);
        }
    }
    tokio::task::yield_now().await;
    assert_eq!((lists.len(), scans.len()), (4, 4));
    assert!(rig.asks.try_recv().is_err());

    lists.pop().unwrap().reply.send(None).unwrap();
    assert!(!next(&mut rig.asks).await.scan);
}

#[tokio::test(start_paused = true)]
async fn this_macs_listing_does_not_wait_for_a_free_slot() {
    let others: Vec<EndpointId> = (1..=5).map(machine).collect();
    let mut online = others.clone();
    online.push(mac());
    let mut rig = rig(online, Vec::new());

    rig.mirror.rows();
    let mut held = Vec::new();
    let mut mine = false;
    for _ in 0..10 {
        let ask = next(&mut rig.asks).await;
        mine |= ask.machine == mac() && !ask.scan;
        held.push(ask);
    }

    // Four remote machines hold the slots, a fifth waits, this Mac asked anyway.
    assert!(mine);
}

/// The next write to the views file; a test that waits for one that never
/// comes fails instead of hanging.
async fn next_write(
    written: &mut mpsc::UnboundedReceiver<Vec<MachineSession>>,
) -> Vec<MachineSession> {
    timeout(KEEP_EVERY * 2, written.recv())
        .await
        .expect("the rows are written")
        .expect("the mirror runs")
}

#[test]
fn the_views_file_takes_every_field_but_the_last_activity() {
    let me = machine(1);
    let old = vec![session(me, "s1", AgentStatus::Running {})];
    let changed = |change: fn(&mut MachineSession)| {
        let mut new = old.clone();
        change(&mut new[0]);
        kept_between(&old, &new)
    };

    assert!(!changed(
        |row| row.last_activity = UNIX_EPOCH + Duration::from_secs(5)
    ));
    assert!(changed(|row| row.title = Some("Fix the build".into())));
    assert!(changed(|row| row.cwd = "/srv/other".into()));
    assert!(changed(|row| row.status = AgentStatus::Ended {}));
    assert!(kept_between(&old, &[]));
}

#[tokio::test(start_paused = true)]
async fn the_views_file_is_written_at_most_every_thirty_seconds() {
    let me = machine(1);
    let rig = rig(
        vec![me],
        vec![
            external(me, "s1", AgentStatus::Running {}),
            external(me, "s2", AgentStatus::Running {}),
        ],
    );
    let (saved, mut written) = mpsc::unbounded_channel();
    tokio::spawn(keep(rig.mirror.watch(), move |rows| {
        let _ = saved.send(rows.to_vec());
    }));
    // The task takes the rows it starts from before the first change.
    tokio::task::yield_now().await;
    let started = Instant::now();

    // The first change is written at once.
    rig.mirror.closed(&me.to_string(), &"s1".into());
    let first = next_write(&mut written).await;
    assert!(
        first
            .iter()
            .any(|row| row.id == "s1" && row.status == AgentStatus::Ended {})
    );
    assert!(started.elapsed() < Duration::from_secs(1));

    // One inside the window waits for the end of it, and is still written.
    sleep(Duration::from_secs(10)).await;
    rig.mirror.reported(
        &me.to_string(),
        &ExternalSession {
            id: "s2".into(),
            agent: "claude".into(),
            presence: Presence::Waiting,
            ..ExternalSession::default()
        },
    );
    rows_where(&rig.mirror, |rows| {
        rows.iter().any(|row| row.status == AgentStatus::Waiting {})
    })
    .await;
    assert!(written.try_recv().is_err());

    let second = next_write(&mut written).await;
    assert!(
        second
            .iter()
            .any(|row| row.id == "s2" && row.status == AgentStatus::Waiting {})
    );
    assert!(started.elapsed() >= KEEP_EVERY);
}
