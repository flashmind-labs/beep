use super::*;
use std::{fs, path::Path, sync::Mutex};
use tokio::sync::{Notify, Semaphore};

#[test]
fn a_recent_write_with_nothing_holding_it_is_stopped() {
    assert_eq!(
        settle(Presence::Recent, None, Holding::Closed),
        Presence::Stopped
    );
    assert_eq!(
        settle(Presence::Recent, None, Holding::Open(7)),
        Presence::Recent
    );
    assert_eq!(
        settle(Presence::Stopped, None, Holding::Open(7)),
        Presence::Running
    );
    // When nothing can say, a recent write still counts.
    assert_eq!(
        settle(Presence::Recent, None, Holding::Unknown),
        Presence::Recent
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_file_this_process_holds_is_held() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(&path, "{}\n").unwrap();
    let other = dir.path().join("other.jsonl");
    fs::write(&other, "{}\n").unwrap();
    let _open = fs::File::open(&path).unwrap();
    // A path nothing opened beside it. (A file closed again may still be
    // held for a moment by a child another test forks before it execs.)
    let held = Holders::default()
        .held_open(vec![path.clone(), other.clone()])
        .await;
    assert_eq!(
        held,
        HashMap::from([
            (path, Holding::Open(std::process::id())),
            (other, Holding::Closed),
        ])
    );
}

/// A scan that records what it was asked, answers from `open` (failing
/// while it is `None`) and, when gated, waits for a permit.
struct Fake {
    batches: Mutex<Vec<Vec<PathBuf>>>,
    open: Mutex<Option<HashMap<PathBuf, u32>>>,
    started: Notify,
    gate: Option<Semaphore>,
}

impl Fake {
    fn new(gated: bool) -> Arc<Self> {
        Arc::new(Self {
            batches: Mutex::default(),
            open: Mutex::new(Some(HashMap::new())),
            started: Notify::new(),
            gate: gated.then(|| Semaphore::new(0)),
        })
    }

    fn holders(self: &Arc<Self>) -> Arc<Holders> {
        let fake = self.clone();
        Arc::new(Holders::with_scan(Arc::new(move |mut real| {
            let fake = fake.clone();
            Box::pin(async move {
                real.sort();
                fake.batches.lock().unwrap().push(real.clone());
                fake.started.notify_one();

                if let Some(gate) = &fake.gate {
                    gate.acquire().await.unwrap().forget();
                }

                let open = fake.open.lock().unwrap().clone()?;
                Some(
                    open.into_iter()
                        .filter(|(path, _)| real.contains(path))
                        .collect(),
                )
            })
        })))
    }

    fn scans(&self) -> usize {
        self.batches.lock().unwrap().len()
    }

    fn hold(&self, file: &Path, pid: u32) {
        let real = fs::canonicalize(file).unwrap();
        self.open
            .lock()
            .unwrap()
            .get_or_insert_default()
            .insert(real, pid);
    }

    fn release(&self, file: &Path) {
        let real = fs::canonicalize(file).unwrap();

        if let Some(open) = self.open.lock().unwrap().as_mut() {
            open.remove(&real);
        }
    }
}

fn files(dir: &tempfile::TempDir, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| {
            let path = dir.path().join(name);
            fs::write(&path, "{}\n").unwrap();
            path
        })
        .collect()
}

async fn ask(holders: &Holders, file: &Path) -> Holding {
    holders.held_open(vec![file.to_owned()]).await[file]
}

/// Waits until `count` asks wait for the checker's next pass.
async fn queued(holders: &Holders, count: usize) {
    while holders.waiting() < count {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn asks_during_a_check_share_the_next_pass() {
    let dir = tempfile::TempDir::new().unwrap();
    let [a, b, c] = files(&dir, &["a", "b", "c"]).try_into().unwrap();
    let fake = Fake::new(true);
    fake.hold(&a, std::process::id());
    let holders = fake.holders();

    let first = tokio::spawn({
        let (holders, a) = (holders.clone(), a.clone());
        async move { ask(&holders, &a).await }
    });
    fake.started.notified().await;
    // Three more while the first scan runs: two for the same file, which
    // it answers, and two others, which one more scan looks up together.
    let more: Vec<_> = [a.clone(), a.clone(), b.clone(), c.clone()]
        .into_iter()
        .map(|file| {
            let holders = holders.clone();
            tokio::spawn(async move { ask(&holders, &file).await })
        })
        .collect();
    queued(&holders, 4).await;
    fake.gate.as_ref().unwrap().add_permits(2);

    assert_eq!(first.await.unwrap(), Holding::Open(std::process::id()));
    let mut answers = Vec::new();

    for ask in more {
        answers.push(ask.await.unwrap());
    }

    let open = Holding::Open(std::process::id());
    assert_eq!(answers, [open, open, Holding::Closed, Holding::Closed]);
    let batches = fake.batches.lock().unwrap().clone();
    let real = |file: &Path| fs::canonicalize(file).unwrap();
    assert_eq!(batches, [vec![real(&a)], vec![real(&b), real(&c)]]);
}

#[tokio::test]
async fn an_unwritten_file_is_not_checked_again() {
    let dir = tempfile::TempDir::new().unwrap();
    let [a] = files(&dir, &["a"]).try_into().unwrap();
    let fake = Fake::new(false);
    let holders = fake.holders();

    for _ in 0..5 {
        assert_eq!(ask(&holders, &a).await, Holding::Closed);
    }

    assert_eq!(fake.scans(), 1);
}

/// A session starts, writes while its process holds the file, ends and is
/// resumed: each change is seen on the next ask, and writes by the holder
/// cost no check.
#[cfg(unix)]
#[tokio::test]
async fn start_exit_and_resume_are_seen_at_once() {
    use crate::events::exits::Exit;
    use std::io::Write;

    let dir = tempfile::TempDir::new().unwrap();
    let [rollout] = files(&dir, &["rollout.jsonl"]).try_into().unwrap();
    let fake = Fake::new(false);
    let holders = fake.holders();
    assert_eq!(ask(&holders, &rollout).await, Holding::Closed);

    // It starts: a process opens the file and writes to it.
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();
    let append = |line: &str| {
        let mut file = fs::OpenOptions::new().append(true).open(&rollout).unwrap();
        file.write_all(line.as_bytes()).unwrap();
    };
    fake.hold(&rollout, pid);
    append("{\"start\":1}\n");
    assert_eq!(ask(&holders, &rollout).await, Holding::Open(pid));
    assert_eq!(fake.scans(), 2);

    for turn in 0..5 {
        append(&format!("{{\"turn\":{turn}}}\n"));
        assert_eq!(ask(&holders, &rollout).await, Holding::Open(pid));
    }

    assert_eq!(fake.scans(), 2, "its own writes need no check");

    // It exits.
    let exit = Exit::watch(pid).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    exit.wait().await;
    fake.release(&rollout);
    assert_eq!(ask(&holders, &rollout).await, Holding::Closed);
    assert_eq!(fake.scans(), 3);

    // It is resumed by this process, which writes again.
    fake.hold(&rollout, std::process::id());
    append("{\"resumed\":1}\n");
    assert_eq!(
        ask(&holders, &rollout).await,
        Holding::Open(std::process::id())
    );
    assert_eq!(fake.scans(), 4);
}

#[tokio::test(start_paused = true)]
async fn a_failed_check_is_unknown_not_closed() {
    let dir = tempfile::TempDir::new().unwrap();
    let [a] = files(&dir, &["a"]).try_into().unwrap();
    let fake = Fake::new(false);
    *fake.open.lock().unwrap() = None;
    let holders = fake.holders();

    assert_eq!(ask(&holders, &a).await, Holding::Unknown);
    // Within the retry gap it is not tried again, and still not known.
    assert_eq!(ask(&holders, &a).await, Holding::Unknown);
    assert_eq!(fake.scans(), 1);

    *fake.open.lock().unwrap() = Some(HashMap::new());
    tokio::time::advance(RETRY_AFTER).await;
    assert_eq!(ask(&holders, &a).await, Holding::Closed);
    assert_eq!(fake.scans(), 2);
}

#[tokio::test]
async fn a_missing_file_is_unknown() {
    let dir = tempfile::TempDir::new().unwrap();
    let fake = Fake::new(false);
    let holders = fake.holders();

    assert_eq!(
        ask(&holders, &dir.path().join("gone")).await,
        Holding::Unknown
    );
    assert_eq!(fake.scans(), 0);
}

/// An answer is checked again once old, and the other files due soon go
/// along in the same scan.
#[tokio::test(start_paused = true)]
async fn old_answers_are_checked_again_together() {
    let dir = tempfile::TempDir::new().unwrap();
    let [a, b] = files(&dir, &["a", "b"]).try_into().unwrap();
    let fake = Fake::new(false);
    let holders = fake.holders();

    assert_eq!(ask(&holders, &a).await, Holding::Closed);
    // `a`, due within half the time, goes along with `b`.
    tokio::time::advance(KEPT_FOR / 2).await;
    assert_eq!(ask(&holders, &b).await, Holding::Closed);
    tokio::time::advance(KEPT_FOR).await;
    // A process opened `a` without writing to it.
    fake.hold(&a, std::process::id());
    assert_eq!(ask(&holders, &a).await, Holding::Open(std::process::id()));
    assert_eq!(ask(&holders, &b).await, Holding::Closed);

    let real = |file: &Path| fs::canonicalize(file).unwrap();
    let batches = fake.batches.lock().unwrap().clone();
    assert_eq!(
        batches,
        [
            vec![real(&a)],
            vec![real(&a), real(&b)],
            vec![real(&a), real(&b)]
        ]
    );
}
