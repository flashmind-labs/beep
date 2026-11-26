use super::*;
use crate::tray::waiting;
use tempfile::TempDir;

const DAY_MS: u64 = 24 * 3600 * 1000;

fn note(id: &str, at: u64) -> Note {
    Note {
        at,
        ..Note::new(
            NoteKind::Finished,
            id.into(),
            format!("Title {id}"),
            "build-box".into(),
        )
    }
}

fn ids(notes: &Notes) -> Vec<String> {
    notes.list().into_iter().map(|note| note.id).collect()
}

fn store(dir: &TempDir) -> Notes {
    Notes::load(dir.path().join("notes.json"))
}

#[test]
fn notes_are_newest_first_and_a_newer_one_replaces_its_id() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    let now = 100 * DAY_MS;
    notes.put(note("a", now - 3), now);
    notes.put(note("b", now - 1), now);
    // An offline note is dated when the machine went away, before the others.
    notes.put(note("c", now - 5), now);
    assert_eq!(ids(&notes), ["b", "a", "c"]);
    notes.put(note("a", now), now);
    assert_eq!(ids(&notes), ["a", "b", "c"]);
}

#[test]
fn only_the_newest_and_the_last_thirty_days_are_kept() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    let now = 100 * DAY_MS;
    notes.put(note("old", now - 31 * DAY_MS), now);
    notes.put(note("month", now - 30 * DAY_MS), now);
    assert_eq!(ids(&notes), ["month"]);

    for i in 0..KEEP as u64 + 5 {
        notes.put(note(&format!("n{i}"), now - 1000 + i), now);
    }

    let kept = ids(&notes);
    assert_eq!(kept.len(), KEEP);
    assert_eq!(kept[0], format!("n{}", KEEP + 4));
    assert!(!kept.contains(&"month".to_owned()));
}

#[test]
fn read_finish_and_dismiss_say_when_they_changed_something() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    notes.put(note("a", 1), 1);
    notes.put(note("b", 2), 2);
    assert_eq!(notes.read(&["a".into()]).len(), 1);
    assert!(notes.read(&["a".into(), "gone".into()]).is_empty());
    assert!(notes.get("a").is_some_and(|n| n.read));
    assert!(notes.get("b").is_some_and(|n| !n.read));
    assert!(notes.finish(|n| n.id == "b"));
    assert!(!notes.finish(|n| n.id == "b"));
    assert!(!notes.dismiss(&["a".into()]).is_empty());
    assert!(notes.dismiss(&["a".into()]).is_empty());
    assert_eq!(ids(&notes), ["b"]);
}

fn marks(list: &[&Note]) -> HashSet<Read> {
    list.iter().map(|n| (n.id.clone(), n.digest())).collect()
}

#[test]
fn a_read_on_another_app_reads_the_note_here() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    notes.put(note("a", 10), 10);
    notes.put(note("b", 10), 10);
    let other = note("other", 10);
    assert!(notes.shared_reads(marks(&[&note("a", 99), &other])));
    assert!(notes.get("a").is_some_and(|n| n.read));
    assert!(notes.get("b").is_some_and(|n| !n.read));
    // Told again, it changes nothing.
    assert!(!notes.shared_reads(marks(&[&note("a", 99)])));
}

#[test]
fn a_note_that_arrives_after_the_read_is_read_whatever_its_time() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    // This app was offline: the note is made here long after it was read elsewhere.
    notes.shared_reads(marks(&[&note("a", 20)]));
    notes.put(note("a", 5000), 5000);
    assert!(notes.get("a").is_some_and(|n| n.read));
}

#[test]
fn a_later_turn_under_the_same_id_is_unread() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    notes.shared_reads(marks(&[&note("a", 20)]));
    let mut again = note("a", 30);
    again.text = "Finished another turn".into();
    notes.put(again, 30);
    assert!(notes.get("a").is_some_and(|n| !n.read));
}

#[test]
fn notes_outlive_the_app() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    let mut asks = note("a", 7);
    asks.kind = NoteKind::Asks;
    asks.target = Some(CenterTarget::Home);
    notes.put(asks.clone(), 7);
    notes.save().expect("the notes are written");
    assert_eq!(store(&dir).list(), [asks]);
    // A file that does not read starts empty.
    fs::write(dir.path().join("notes.json"), "{").expect("written");
    assert!(store(&dir).list().is_empty());
}

#[test]
fn a_run_says_how_it_ended_on_its_branch_and_opens_it() {
    let mut news = RunNews {
        project: "acme/widget".into(),
        host: "github.com".into(),
        run: beep_proto::WorkflowRun {
            id: 9001,
            number: 351,
            workflow: "CI".into(),
            title: "Cut the cache warm-up".into(),
            branch: "main".into(),
            event: "push".into(),
            actor: "someone".into(),
            state: RunState::Failed {},
            url: String::new(),
            started_at: None,
            finished_at: None,
            progress: None,
            pull_requests: Vec::new(),
        },
    };
    let note = run_note(&news).expect("a failed run makes a note");
    assert_eq!(
        (note.title.as_str(), note.text.as_str()),
        ("Build failed on main", "CI #351")
    );
    assert_eq!(note.source, "acme/widget");
    assert_eq!(note.id, "run:acme/widget:github.com:9001");
    assert_eq!(
        note.target,
        Some(CenterTarget::Projects {
            project: Some("acme/widget".into()),
            run: Some(RunAt {
                id: 9001,
                host: "github.com".into()
            }),
        })
    );
    news.run.state = RunState::Passed {};
    news.run.branch.clear();
    assert_eq!(
        run_note(&news).map(|n| n.title),
        Some("Build passed".into())
    );
    news.run.state = RunState::Skipped {};
    assert_eq!(run_note(&news), None);
}

#[test]
fn an_ask_names_its_question_and_its_tray_item() {
    let key = AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "claude".into(),
        session: "s1".into(),
        question: Some(3),
        machine_name: "build-box".into(),
    };
    let mut asks = note(&key.to_string(), 1);
    assert_eq!(asked(&asks), None, "a finished turn asks nothing");
    asks.kind = NoteKind::Asks;
    assert_eq!(asked(&asks), Some(key));
    assert_eq!(
        waiting::agent_item("m1", &SessionId::from("s1"), 3),
        "wait:agent:m1:s1:3"
    );
}

fn ready_key() -> AgentNoticeKey {
    AgentNoticeKey {
        machine_id: "m1".into(),
        agent: "codex".into(),
        session: "s1".into(),
        question: None,
        machine_name: "build-box".into(),
    }
}

fn notice(key: &AgentNoticeKey) -> Notice {
    Notice {
        session_id: key.to_string(),
        title: "Codex is ready".into(),
        body: "The turn ended".into(),
    }
}

#[test]
fn a_watched_turn_leaves_no_ready_note_and_removes_its_question() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    let ready = ready_key();
    assert!(
        !notes
            .agent_notice(&ready, &notice(&ready), true, true, false)
            .changed
    );
    assert!(notes.list().is_empty());

    let question = AgentNoticeKey {
        question: Some(7),
        ..ready.clone()
    };
    assert!(
        notes
            .agent_notice(&question, &notice(&question), true, false, false)
            .changed
    );
    assert!(
        notes
            .agent_notice(&ready, &notice(&ready), true, false, false)
            .changed
    );
    assert!(
        notes
            .agent_notice(&ready, &notice(&ready), true, true, false)
            .changed
    );
    assert!(notes.list().is_empty());

    // A new question is still kept while the session is on screen.
    let next = AgentNoticeKey {
        question: Some(8),
        ..ready
    };
    assert!(
        notes
            .agent_notice(&next, &notice(&next), true, true, false)
            .changed
    );
    assert!(
        notes
            .get(&next.to_string())
            .is_some_and(|n| n.read && !n.done)
    );
}

#[test]
fn opening_a_session_removes_only_its_ready_notes_and_saves_the_removal() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    let ready = ready_key();
    let other_machine = AgentNoticeKey {
        machine_id: "m2".into(),
        ..ready.clone()
    };
    let other_agent = AgentNoticeKey {
        agent: "claude".into(),
        ..ready.clone()
    };
    let other_session = AgentNoticeKey {
        session: "s2".into(),
        ..ready.clone()
    };
    let question = AgentNoticeKey {
        question: Some(7),
        ..ready.clone()
    };
    for key in [
        &ready,
        &other_machine,
        &other_agent,
        &other_session,
        &question,
    ] {
        notes.agent_notice(key, &notice(key), true, false, false);
    }
    notes.read(&[ready.to_string()]);
    let mut renamed = note(
        &AgentNoticeKey {
            machine_name: "gpu-box".into(),
            ..ready.clone()
        }
        .to_string(),
        now_ms(),
    );
    renamed.kind = NoteKind::Finished;
    notes.put(renamed, now_ms());
    notes.put(note("unrelated", now_ms()), now_ms());

    assert!(
        !notes
            .viewed_agent("m1", "codex", &SessionId::from("s1"))
            .is_empty()
    );
    assert!(
        notes
            .viewed_agent("m1", "codex", &SessionId::from("s1"))
            .is_empty()
    );
    assert!(notes.get(&ready.to_string()).is_none());
    assert_eq!(notes.list().len(), 5);

    for key in [&other_machine, &other_agent, &other_session, &question] {
        assert!(notes.get(&key.to_string()).is_some());
    }

    notes.save().expect("the notes are written");
    assert_eq!(store(&dir).list(), notes.list());
}

/// Issue 555: a turn read on one app, by opening its session, must not show again on a phone
/// whose machine stream replays it when the phone opens.
#[test]
fn a_turn_read_on_one_app_does_not_notify_on_another() {
    let desktop_dir = TempDir::new().expect("a temp dir");
    let phone_dir = TempDir::new().expect("a temp dir");
    let desktop = store(&desktop_dir);
    let phone = store(&phone_dir);
    let ready = ready_key();

    let told = desktop.agent_notice(&ready, &notice(&ready), true, false, false);
    assert!(
        told.unread.is_some(),
        "unread on the desktop, so it notifies"
    );
    let removed = desktop.viewed_agent("m1", "codex", &SessionId::from("s1"));
    let shared: HashSet<Read> = unread_of(removed).collect();
    assert_eq!(shared.len(), 1, "opening the session reads it for the mesh");

    phone.shared_reads(shared);
    let replayed = phone.agent_notice(&ready, &notice(&ready), true, false, false);
    assert_eq!(replayed.unread, None, "the phone does not notify");
    assert!(phone.get(&ready.to_string()).is_some_and(|n| n.read));

    // The next turn of the session is news again.
    let mut next = notice(&ready);
    next.body = "Another turn ended".into();
    assert!(
        phone
            .agent_notice(&ready, &next, true, false, false)
            .unread
            .is_some()
    );
}

#[test]
fn a_turn_on_screen_and_a_dismissed_note_are_read_for_the_other_apps() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    let ready = ready_key();
    let seen = notes.agent_notice(&ready, &notice(&ready), true, true, false);
    assert_eq!(seen.unread, None);
    assert_eq!(
        seen.read.len(),
        1,
        "a turn read on screen is read everywhere"
    );

    let question = AgentNoticeKey {
        question: Some(7),
        ..ready
    };
    notes.agent_notice(&question, &notice(&question), true, false, false);
    let dismissed = notes.dismiss(&[question.to_string()]);
    assert_eq!(unread_of(dismissed).count(), 1);
}

#[test]
fn answered_questions_are_removed_and_other_notifications_are_kept() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    for id in ["answered", "pending"] {
        let mut question = note(id, 1);
        question.kind = NoteKind::Asks;
        notes.put(question, 1);
    }
    let mut conflict = note("conflict", 1);
    conflict.kind = NoteKind::Conflict;
    notes.put(conflict, 1);
    notes.put(note("finished", 1), 1);

    assert!(notes.finish(|n| n.id == "answered" || n.id == "conflict"));
    assert!(!notes.finish(|n| n.id == "answered" || n.id == "conflict"));
    assert!(notes.get("answered").is_none());
    assert!(notes.get("pending").is_some_and(|n| !n.done));
    assert!(notes.get("conflict").is_some_and(|n| n.done));
    assert!(notes.get("finished").is_some_and(|n| !n.done));

    notes.save().expect("the notes are written");
    assert_eq!(store(&dir).list(), notes.list());
}

#[test]
fn loading_old_history_removes_only_answered_questions() {
    let dir = TempDir::new().expect("a temp dir");
    let mut answered = note("answered", 1);
    answered.kind = NoteKind::Asks;
    answered.done = true;
    let mut pending = note("pending", 1);
    pending.kind = NoteKind::Asks;
    let mut settled = note("settled", 1);
    settled.kind = NoteKind::Conflict;
    settled.done = true;
    let finished = note("finished", 1);
    let saved = [answered, pending.clone(), settled.clone(), finished.clone()];
    fs::write(
        dir.path().join("notes.json"),
        serde_json::to_vec(&saved).expect("the notes serialize"),
    )
    .expect("the old history is written");

    assert_eq!(store(&dir).list(), [pending, settled, finished]);
}

#[test]
fn a_new_question_removes_the_previous_question_only_in_its_session() {
    let dir = TempDir::new().expect("a temp dir");
    let notes = store(&dir);
    let question = AgentNoticeKey {
        question: Some(7),
        ..ready_key()
    };
    let other = AgentNoticeKey {
        session: "s2".into(),
        ..question.clone()
    };
    for key in [&question, &other] {
        notes.agent_notice(key, &notice(key), true, false, false);
    }
    let next = AgentNoticeKey {
        question: Some(8),
        ..question.clone()
    };
    notes.agent_notice(&next, &notice(&next), true, false, false);

    assert!(notes.get(&question.to_string()).is_none());
    assert!(notes.get(&other.to_string()).is_some_and(|n| !n.done));
    assert!(notes.get(&next.to_string()).is_some_and(|n| !n.done));
}
