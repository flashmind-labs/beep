use super::*;
use crate::{
    memory::watchers::testing::{Harness, Scripted},
    store::test_chat,
};
use std::slice;

/// What a stub watcher does with each review.
#[derive(Clone, Copy)]
enum Act {
    Read,
    Fail,
    Panic,
}

/// A watcher that keeps the stretches it was handed.
struct Stub {
    kind: WatcherKind,
    act: Act,
    read: Mutex<Vec<(i64, i64)>>,
    /// Each chat it read, with the user it was read for.
    chats: Mutex<Vec<(Uuid, String)>>,
}

impl Stub {
    fn new(kind: WatcherKind, act: Act) -> Arc<Self> {
        Arc::new(Self {
            kind,
            act,
            read: Mutex::default(),
            chats: Mutex::default(),
        })
    }

    fn read(&self) -> Vec<(i64, i64)> {
        lock(&self.read).clone()
    }

    fn chats(&self) -> Vec<(Uuid, String)> {
        let mut chats = lock(&self.chats).clone();
        chats.sort();
        chats
    }
}

#[async_trait]
impl Watcher for Stub {
    fn kind(&self) -> WatcherKind {
        self.kind
    }

    async fn review(&self, review: &Review<'_>) -> Result<()> {
        match self.act {
            Act::Read => {
                let transcript = review.transcript;
                lock(&self.read).push((transcript.first_seq, transcript.last_seq));
                lock(&self.chats).push((review.chat, review.user.id().to_owned()));
                Ok(())
            }
            Act::Fail => anyhow::bail!("the model said nothing useful"),
            Act::Panic => panic!("a bug in a watcher"),
        }
    }
}

fn settings(concurrency: usize) -> Settings {
    Settings {
        timing: TimingSettings {
            quiet: Duration::from_secs(30 * 60),
            retry: Duration::from_secs(10 * 60),
        },
        sweep_every: Duration::from_secs(60),
        lookback: Duration::from_secs(24 * 60 * 60),
        concurrency,
        watchers: Vec::new(),
    }
}

fn stub_runner(h: &Harness, concurrency: usize, stubs: &[Arc<Stub>]) -> Runner {
    let watchers = stubs
        .iter()
        .map(|stub| {
            let slot = WatcherSettings {
                kind: stub.kind,
                llm: AgentLlmConfig::new("openai:test".parse().expect("a model name")),
                timeout: Duration::from_secs(30),
            };
            (stub.clone() as Arc<dyn Watcher>, slot)
        })
        .collect();
    Runner::new(&h.app, settings(concurrency), watchers)
}

fn after_quiet() -> DateTime<Utc> {
    Utc::now() + TimeDelta::minutes(31)
}

#[tokio::test]
async fn one_failing_watcher_leaves_the_others_alone() {
    let h = Harness::new(Scripted::default()).await;
    let chat = test_chat("fan-out");
    h.turn(&h.alice, chat, "book padel for tomorrow").await;
    let last = h.turn(&h.alice, chat, "at seven please").await;
    let memory = Stub::new(WatcherKind::Memory, Act::Read);
    let skills = Stub::new(WatcherKind::Skills, Act::Fail);
    let improvement = Stub::new(WatcherKind::Improvement, Act::Panic);
    let mut runner = stub_runner(
        &h,
        3,
        &[memory.clone(), skills.clone(), improvement.clone()],
    );

    // Not quiet yet.
    runner.tick(Utc::now()).await;
    assert!(runner.drain().await.is_empty());
    assert!(memory.read().is_empty());

    let now = after_quiet();
    runner.tick(now).await;
    assert!(
        runner.drain().await.is_empty(),
        "a panic stays in its watcher"
    );
    assert_eq!(memory.read().len(), 1);
    assert_eq!(memory.read()[0].1, last);

    let states = h.app.store.watcher_states(&[chat]).await.unwrap();
    assert_eq!(states[&(chat, WatcherKind::Memory)].reviewed_seq, last);
    let failed = &states[&(chat, WatcherKind::Skills)];
    assert_eq!((failed.reviewed_seq, failed.failures), (0, 1));
    // A panic counts as a failure, and frees its slot.
    let panicked = &states[&(chat, WatcherKind::Improvement)];
    assert_eq!((panicked.reviewed_seq, panicked.failures), (0, 1));
    assert!(lock(&runner.busy).is_empty());
    assert_eq!(runner.limit.available_permits(), 3);

    // The memory watcher has read everything; the failed ones wait.
    runner.tick(now).await;
    assert!(runner.running.is_empty());
    assert_eq!(memory.read().len(), 1);
}

#[tokio::test]
async fn each_watcher_reads_only_what_came_after_its_place() {
    let h = Harness::new(Scripted::default()).await;
    let chat = test_chat("places");
    let first = h.turn(&h.alice, chat, "my sister is called Ana").await;
    let memory = Stub::new(WatcherKind::Memory, Act::Read);
    let mut runner = stub_runner(&h, 2, slice::from_ref(&memory));
    runner.tick(after_quiet()).await;
    runner.drain().await;

    let second = h.turn(&h.alice, chat, "she lives in Porto").await;
    // A restart starts from the saved place.
    let mut runner = stub_runner(&h, 2, slice::from_ref(&memory));
    runner.tick(after_quiet()).await;
    runner.drain().await;
    let read = memory.read();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].1, first);
    assert!(read[1].0 > first, "nothing is read twice");
    assert_eq!(read[1].1, second);
}

#[tokio::test]
async fn a_task_ending_reads_its_chat_now_for_every_user() {
    let h = Harness::new(Scripted::default()).await;
    let (ours, theirs) = (test_chat("alice-task"), test_chat("bob-task"));
    h.turn(&h.alice, ours, "check the build").await;
    h.turn(&h.bob, theirs, "check the build").await;
    let memory = Stub::new(WatcherKind::Memory, Act::Read);
    let mut runner = stub_runner(&h, 2, slice::from_ref(&memory));
    runner.wake("alice".into(), ours);
    runner.wake("bob".into(), theirs);
    // A wake for a chat that is not the user's own reads nothing.
    runner.wake("bob".into(), ours);
    runner.tick(Utc::now()).await;
    runner.drain().await;
    let mut expected = vec![(ours, "alice".to_owned()), (theirs, "bob".to_owned())];
    expected.sort();
    assert_eq!(memory.chats(), expected);
    assert!(runner.woken.is_empty());

    // Read once, so a later sweep leaves both alone.
    runner.tick(after_quiet() + TimeDelta::hours(1)).await;
    runner.drain().await;
    assert_eq!(memory.chats(), expected);
}

#[tokio::test]
async fn reviews_past_the_limit_wait_for_the_next_tick() {
    let h = Harness::new(Scripted::default()).await;
    let chat = test_chat("limit");
    h.turn(&h.alice, chat, "hello there friend").await;
    let memory = Stub::new(WatcherKind::Memory, Act::Read);
    let skills = Stub::new(WatcherKind::Skills, Act::Read);
    let mut runner = stub_runner(&h, 1, &[memory.clone(), skills.clone()]);
    runner.wake("alice".into(), chat);
    runner.tick(Utc::now()).await;
    assert_eq!(runner.running.len(), 1);
    // Kept, since one of its reviews could not start.
    assert!(runner.woken.contains_key(&chat));
    runner.drain().await;
    runner.tick(Utc::now()).await;
    runner.drain().await;
    assert_eq!(memory.read().len() + skills.read().len(), 2);
    assert!(runner.woken.is_empty());
}

#[tokio::test]
async fn gives_up_after_failing_three_times() {
    let h = Harness::new(Scripted::default()).await;
    let chat = test_chat("give-up");
    let last = h.turn(&h.alice, chat, "this one breaks the watcher").await;
    let skills = Stub::new(WatcherKind::Skills, Act::Fail);
    let mut runner = stub_runner(&h, 1, &[skills]);
    let mut now = after_quiet();
    for wait in [0, 10, 20] {
        now += TimeDelta::minutes(wait);
        runner.tick(now).await;
        runner.drain().await;
    }
    let states = h.app.store.watcher_states(&[chat]).await.unwrap();
    let state = &states[&(chat, WatcherKind::Skills)];
    assert_eq!((state.reviewed_seq, state.failures), (last, 0));
}
