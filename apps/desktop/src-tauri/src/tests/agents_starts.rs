use super::*;
use crate::agents::kept_folder::{IMAGES, image_file};
use std::collections::VecDeque;

fn starts() -> (tempfile::TempDir, PathBuf, Arc<Starts>) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let kept = dir.path().join("starts");
    let starts = Arc::new(Starts::load(kept.clone(), dir.path().join("cache")));
    (dir, kept, starts)
}

fn start(stage: Stage) -> Start {
    Start {
        id: StartId::new(),
        machine_id: "build-box-id".into(),
        machine_name: "build-box".into(),
        agent: "claude".into(),
        cwd: None,
        prompt: "Fix the build".into(),
        model: Some("opus".into()),
        effort: None,
        fast: None,
        checkout: None,
        images: Vec::new(),
        assistant: false,
        account: None,
        continues: None,
        why: String::new(),
        at: SystemTime::UNIX_EPOCH,
        stage,
    }
}

fn keep(starts: &Starts, start: Start) -> StartId {
    let id = start.id;
    starts.with(|held| held.keep(start));
    id
}

fn started(start: &Start, session: &str) -> Tried {
    Tried::Started(Box::new(StartedAgent {
        session: MachineSession {
            id: session.into(),
            ..start.row()
        },
        why: String::new(),
    }))
}

/// How a start stands, as the lists show it.
fn shown(starts: &Starts, id: StartId) -> String {
    starts.read(|held| match held.get(id) {
        None => "gone".into(),
        Some(start) => match (&start.stage, start.row().doing) {
            (Stage::Failed { reason }, _) => format!("failed: {reason}"),
            (_, Some(doing)) => doing,
            (_, None) => "nothing".into(),
        },
    })
}

/// A machine that answers each try in turn, and what the runner did, with
/// how the start stood each time.
struct Fake {
    starts: Arc<Starts>,
    tries: VecDeque<Tried>,
    /// The user cancels the start while its machine has it.
    cancelled: bool,
    log: Vec<String>,
}

impl Fake {
    fn new(starts: &Arc<Starts>, tries: impl IntoIterator<Item = Tried>) -> Self {
        Self {
            starts: starts.clone(),
            tries: tries.into_iter().collect(),
            cancelled: false,
            log: Vec::new(),
        }
    }
}

impl Link for Fake {
    async fn online(&mut self, start: &Start) {
        self.log
            .push(format!("online: {}", shown(&self.starts, start.id)));
    }

    async fn open(&mut self, start: &Start) -> Tried {
        self.log
            .push(format!("open: {}", shown(&self.starts, start.id)));

        if self.cancelled {
            self.starts.with(|held| held.remove(start.id));
        }

        self.tries
            .pop_front()
            .unwrap_or_else(|| Tried::Refused("no more tries".into()))
    }

    async fn pause(&mut self) {
        self.log.push("pause".into());
    }

    fn changed(&mut self, _: StartId) {
        self.log.push("changed".into());
    }

    async fn started(&mut self, start: Start, started: StartedAgent) {
        self.log.push(format!(
            "started {} ({})",
            started.session.id,
            shown(&self.starts, start.id)
        ));
    }

    async fn close(&mut self, started: StartedAgent) {
        self.log.push(format!("closed {}", started.session.id));
    }
}

#[tokio::test]
async fn a_start_for_an_offline_machine_waits_for_it_then_starts() {
    let (_dir, _, starts) = starts();
    let kept = start(Stage::Waiting);
    let mut link = Fake::new(&starts, [started(&kept, "s1")]);
    let id = keep(&starts, kept);

    run(&starts, id, &mut link).await;

    assert_eq!(
        link.log,
        [
            "online: Waiting for build-box",
            "changed",
            "open: Starting Claude Code on build-box",
            "started s1 (gone)",
        ]
    );
    assert!(starts.rows().is_empty());
}

#[tokio::test]
async fn a_try_that_did_not_reach_the_machine_waits_and_goes_again() {
    let (_dir, _, starts) = starts();
    let kept = start(Stage::Starting);
    let mut link = Fake::new(
        &starts,
        [Tried::Unreached, Tried::Unreached, started(&kept, "s1")],
    );
    let id = keep(&starts, kept);

    run(&starts, id, &mut link).await;

    let opens = link.log.iter().filter(|l| l.starts_with("open")).count();
    let pauses = link.log.iter().filter(|l| *l == "pause").count();
    assert_eq!((opens, pauses), (3, 2));
    assert!(
        link.log
            .contains(&"online: Waiting for build-box".to_string())
    );
    assert_eq!(
        link.log.last().map(String::as_str),
        Some("started s1 (gone)")
    );
}

#[tokio::test]
async fn a_link_dropped_after_the_start_went_fails_it_without_trying_again() {
    let (_dir, _, starts) = starts();
    let mut link = Fake::new(&starts, [Tried::Dropped]);
    let id = keep(&starts, start(Stage::Starting));

    run(&starts, id, &mut link).await;

    let opens = link.log.iter().filter(|l| l.starts_with("open")).count();
    assert_eq!(opens, 1);
    assert_eq!(
        shown(&starts, id),
        "failed: The link to build-box dropped while it started. It may be running: check the list, or retry."
    );
}

#[tokio::test]
async fn a_refused_start_fails_with_the_machines_reason() {
    let (_dir, _, starts) = starts();
    let mut link = Fake::new(
        &starts,
        [Tried::Refused(
            "Could not start Claude Code: no folder".into(),
        )],
    );
    let id = keep(&starts, start(Stage::Starting));

    run(&starts, id, &mut link).await;

    assert_eq!(
        shown(&starts, id),
        "failed: Could not start Claude Code: no folder"
    );
    assert_eq!(link.log.last().map(String::as_str), Some("changed"));
}

#[tokio::test]
async fn a_retried_start_runs_again() {
    let (_dir, _, starts) = starts();
    let kept = start(Stage::Starting);
    let mut link = Fake::new(&starts, [Tried::Dropped, started(&kept, "s1")]);
    let id = keep(&starts, kept);
    run(&starts, id, &mut link).await;

    // Only a failed start is retried, and it needs a runner once.
    assert!(starts.with(|held| held.retry(id)).expect("a failed start"));
    assert!(starts.with(|held| held.retry(id)).is_err());
    assert_eq!(shown(&starts, id), "Waiting for build-box");
    run(&starts, id, &mut link).await;

    assert_eq!(
        link.log.last().map(String::as_str),
        Some("started s1 (gone)")
    );
}

#[tokio::test]
async fn a_start_cancelled_while_its_machine_starts_it_has_its_session_closed() {
    let (_dir, _, starts) = starts();
    let kept = start(Stage::Waiting);
    let mut link = Fake {
        cancelled: true,
        ..Fake::new(&starts, [started(&kept, "s1")])
    };
    let id = keep(&starts, kept);

    run(&starts, id, &mut link).await;

    assert_eq!(link.log.last().map(String::as_str), Some("closed s1"));
    assert!(!link.log.iter().any(|l| l.starts_with("started")));
    assert_eq!(shown(&starts, id), "gone");
}

#[tokio::test]
async fn a_removed_start_forgets_its_images_and_is_not_sent() {
    let (dir, kept, starts) = starts();
    let with_image = Start {
        images: vec![KeptImage {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: "aGVsbG8=".into(),
        }],
        ..start(Stage::Waiting)
    };
    let id = keep(&starts, with_image);
    starts.sync();
    let file = dir
        .path()
        .join("cache")
        .join(IMAGES)
        .join(image_file(id.images(), 0));
    assert!(file.exists());
    assert!(!kept.join(IMAGES).exists());

    assert!(starts.with(|held| held.remove(id)).is_some());
    let mut link = Fake::new(&starts, []);
    run(&starts, id, &mut link).await;
    starts.sync();

    assert!(link.log.is_empty());
    assert!(!file.exists());
}

#[test]
fn a_start_sent_when_the_app_quit_comes_back_failed_and_a_waiting_one_runs() {
    let (dir, kept, starts) = starts();
    let sent = keep(&starts, start(Stage::Starting));
    let waiting = keep(&starts, start(Stage::Waiting));
    starts.sync();
    drop(starts);

    let again = Starts::load(kept, dir.path().join("cache"));

    assert_eq!(
        shown(&again, sent),
        "failed: The link to build-box dropped while it started. It may be running: check the list, or retry."
    );
    assert_eq!(shown(&again, waiting), "Waiting for build-box");
    assert_eq!(again.with(Held::waiting), [waiting]);
    // Its runner runs already.
    assert!(again.with(Held::waiting).is_empty());
}

#[test]
fn a_start_shows_as_a_starting_session_with_its_prompt_as_the_first_entry() {
    let kept = Start {
        images: vec![KeptImage {
            name: "shot.png".into(),
            mime: "image/png".into(),
            data: "".into(),
        }],
        ..start(Stage::Waiting)
    };

    let row = kept.row();
    assert_eq!(row.id.as_ref().parse::<StartId>().ok(), Some(kept.id));
    assert!(row.id.as_ref().starts_with("start:"));
    assert_eq!(row.status, AgentStatus::Starting {});
    assert_eq!(row.doing.as_deref(), Some("Waiting for build-box"));
    assert_eq!(row.title.as_deref(), Some("Fix the build"));
    assert_eq!((row.cwd.as_str(), row.external), ("", false));
    assert_eq!(row.model, Some(ModelId::new("opus")));

    let page = kept.page();
    let entries: Vec<(&AgentEntryKind, &str)> = page
        .entries
        .iter()
        .map(|entry| (&entry.kind, entry.text.as_str()))
        .collect();
    assert_eq!(
        entries,
        [(&AgentEntryKind::User {}, "Fix the build\n[image: shot.png]")]
    );
    assert_eq!(page.error, None);

    let failed = Start {
        stage: Stage::Failed {
            reason: "refused".into(),
        },
        ..kept
    };
    let page = failed.page();
    assert_eq!(page.session.status, AgentStatus::Idle {});
    assert_eq!(page.session.doing, None);
    assert_eq!(page.error.as_deref(), Some("refused"));
    // A machine's own session id is not a start's.
    assert!(
        "0b8c4c5e-1d1f-4c51-9a55-0d1e2f3a4b5c"
            .parse::<StartId>()
            .is_err()
    );
}
