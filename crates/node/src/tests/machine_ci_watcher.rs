use super::*;
use crate::upkeep::ci::check::Answer;
use std::{future::pending, sync::atomic::Ordering::SeqCst};
use tempfile::TempDir;

struct Idle;

impl Forge for Idle {
    async fn list(&self, _: &Target) -> Result<Vec<WorkflowRun>> {
        Ok(Vec::new())
    }

    async fn one(&self, _: &Target, id: u64) -> Result<WorkflowRun> {
        bail!("no run {id}")
    }

    async fn login(&self, _: &Target) -> Result<String> {
        Ok("someone".into())
    }

    async fn wait(&self, _: &Target, _: u64) -> Result<bool> {
        pending().await
    }

    async fn check(&self, _: &Target, _: Option<&str>) -> Result<Option<Answer>> {
        Ok(None)
    }
}

struct Nobody;

impl World for Nobody {
    fn me(&self) -> EndpointId {
        iroh::SecretKey::from_bytes(&[1; 32]).public()
    }

    fn projects(&self) -> Vec<Project> {
        Vec::new()
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        BTreeSet::new()
    }

    #[allow(clippy::manual_async_fn, reason = "the future must not borrow the world")]
    fn ask(&self, _: EndpointId) -> impl Future<Output = Result<ForgeReach>> + Send + 'static {
        async { bail!("nobody") }
    }

    async fn changed(&mut self) -> bool {
        pending().await
    }

    async fn pushed(&self, _: &Project) -> Option<SystemTime> {
        None
    }
}

fn target() -> Target {
    Target {
        host: Host::GitHub,
        repo: "owner/repo".into(),
    }
}

#[tokio::test]
async fn a_cancelled_wait_does_not_clear_the_wait_that_replaced_it() {
    let dir = TempDir::new().expect("a temp dir");
    let ci = Arc::new(Ci::default());
    let mut watcher = Watcher::start(Idle, Nobody, ci, dir.path().join("runs.json")).await;
    let key: RunKey = ("project".into(), 7);

    watcher.start_wait(key.clone(), target());
    watcher.drop_waits("project");
    watcher.start_wait(key.clone(), target());
    let second = watcher.waiting[&key].id();

    // The first wait's cancellation arrives after the second one started.
    let cancelled = watcher
        .waits
        .join_next_with_id()
        .await
        .expect("a finished task");
    assert!(cancelled.is_err());
    watcher.waited(cancelled);

    assert_eq!(watcher.waiting.get(&key).map(AbortHandle::id), Some(second));
    assert_eq!(watcher.tasks.get(&second), Some(&key));
    assert_eq!(watcher.tasks.len(), 1);
}

/// Lists of the `slow` repository wait for a permit; others answer at once.
#[derive(Clone)]
struct Slow {
    permits: Arc<tokio::sync::Semaphore>,
    active: Arc<std::sync::atomic::AtomicUsize>,
    most: Arc<std::sync::atomic::AtomicUsize>,
    slow_lists: Arc<std::sync::atomic::AtomicUsize>,
}

impl Default for Slow {
    fn default() -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(0)),
            active: Arc::default(),
            most: Arc::default(),
            slow_lists: Arc::default(),
        }
    }
}

impl Forge for Slow {
    async fn list(&self, at: &Target) -> Result<Vec<WorkflowRun>> {
        use std::sync::atomic::Ordering::SeqCst;

        if at.repo == "slow/repo" {
            self.slow_lists.fetch_add(1, SeqCst);
            let now = self.active.fetch_add(1, SeqCst) + 1;
            self.most.fetch_max(now, SeqCst);
            self.permits.acquire().await.expect("open").forget();
            self.active.fetch_sub(1, SeqCst);
        }

        if at.repo == "fast/repo" {
            return Ok(vec![WorkflowRun {
                id: 1,
                number: 1,
                workflow: "build.yml".into(),
                title: "Build".into(),
                branch: "main".into(),
                event: "push".into(),
                actor: "someone".into(),
                state: RunState::Running {},
                url: String::new(),
                started_at: None,
                finished_at: None,
                progress: None,
                pull_requests: Vec::new(),
            }]);
        }

        Ok(Vec::new())
    }

    async fn one(&self, _: &Target, id: u64) -> Result<WorkflowRun> {
        bail!("no run {id}")
    }

    async fn login(&self, _: &Target) -> Result<String> {
        Ok("someone".into())
    }

    async fn wait(&self, _: &Target, _: u64) -> Result<bool> {
        pending().await
    }

    async fn check(&self, _: &Target, _: Option<&str>) -> Result<Option<Answer>> {
        Ok(None)
    }
}

fn project(name: &str, repo: &str) -> Project {
    Project {
        name: name.into(),
        target: Target {
            host: Host::GitHub,
            repo: repo.into(),
        },
        machines: BTreeSet::new(),
    }
}

// A host that does not answer holds its own project only: the other lists
// are applied as they end, and the slow project never has two calls at once.
#[tokio::test]
async fn a_slow_host_holds_only_its_own_project() {
    use std::sync::atomic::Ordering::SeqCst;

    let dir = TempDir::new().expect("a temp dir");
    let forge = Slow::default();
    let mut watcher = Watcher::start(
        forge.clone(),
        Nobody,
        Arc::new(Ci::default()),
        dir.path().join("runs.json"),
    )
    .await;
    let slow = project("slow", "slow/repo");
    let fast = project("fast", "fast/repo");
    watcher.projects = vec![slow.clone(), fast.clone()];
    watcher.held = ["slow".to_owned(), "fast".to_owned()].into();

    watcher.begin(slow.clone(), Todo::List);
    watcher.begin(fast.clone(), Todo::List);
    // Asked for again while the first is in flight: kept once, for after it.
    watcher.begin(slow.clone(), Todo::List);
    watcher.begin(slow.clone(), Todo::List);

    let done = watcher.calls.join_next_with_id().await.expect("a call");
    watcher.applied(done).await;
    assert!(watcher.next.contains_key("fast"), "the fast list was applied");
    assert!(!watcher.next.contains_key("slow"));

    forge.permits.add_permits(10);
    let done = watcher.calls.join_next_with_id().await.expect("a call");
    watcher.applied(done).await;
    assert!(watcher.next.contains_key("slow"));
    let done = watcher.calls.join_next_with_id().await.expect("the kept list");
    watcher.applied(done).await;

    assert!(watcher.calls.is_empty());
    assert_eq!(forge.slow_lists.load(SeqCst), 2, "the two asks became one");
    assert_eq!(forge.most.load(SeqCst), 1, "never two lists at once");
}

// A host that does not answer keeps a call in flight; what the other
// projects' calls changed is saved all the same.
#[tokio::test]
async fn the_books_are_saved_while_another_projects_call_is_in_flight() {
    let dir = TempDir::new().expect("a temp dir");
    let file = dir.path().join("runs.json");
    let mut watcher = Watcher::start(
        Slow::default(),
        Nobody,
        Arc::new(Ci::default()),
        file.clone(),
    )
    .await;
    let slow = project("slow", "slow/repo");
    let fast = project("fast", "fast/repo");
    watcher.projects = vec![slow.clone(), fast.clone()];
    watcher.held = ["slow".to_owned(), "fast".to_owned()].into();

    watcher.begin(slow, Todo::List);
    watcher.begin(fast, Todo::List);
    let done = watcher.calls.join_next_with_id().await.expect("a call");
    watcher.applied(done).await;

    assert!(!watcher.calls.is_empty(), "the slow list is still in flight");
    assert!(file.exists(), "the file holds the running run");
}

/// One other machine is online and never says how it reaches the hosts.
struct Silent(EndpointId);

impl World for Silent {
    fn me(&self) -> EndpointId {
        iroh::SecretKey::from_bytes(&[1; 32]).public()
    }

    fn projects(&self) -> Vec<Project> {
        let mut project = project("quiet", "quiet/repo");
        project.machines = [self.me(), self.0].into();
        vec![project]
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        [self.0].into()
    }

    #[allow(clippy::manual_async_fn, reason = "the future must not borrow the world")]
    fn ask(&self, _: EndpointId) -> impl Future<Output = Result<ForgeReach>> + Send + 'static {
        pending()
    }

    async fn changed(&mut self) -> bool {
        pending().await
    }

    async fn pushed(&self, _: &Project) -> Option<SystemTime> {
        None
    }
}

// A machine that does not answer delays no poll: the held projects are
// listed with what was known, and the election waits for the answer only
// the first time.
#[tokio::test]
async fn a_silent_machine_does_not_hold_up_a_poll() {
    let dir = TempDir::new().expect("a temp dir");
    let peer = iroh::SecretKey::from_bytes(&[2; 32]).public();
    let mut watcher = Watcher::start(
        Slow::default(),
        Silent(peer),
        Arc::new(Ci::default()),
        dir.path().join("runs.json"),
    )
    .await;

    watcher.poll().await;
    assert!(watcher.held.is_empty(), "the first election waits for the answers");
    assert_eq!(watcher.asking.len(), 1);

    watcher.answered = true;
    watcher.poll().await;
    assert!(watcher.held.contains("quiet"), "later polls go on with what is known");
    assert!(!watcher.calls.is_empty(), "and list the held project");
    assert_eq!(watcher.asking.len(), 1, "one gather at a time");
}

/// One other machine, which holds the only checkout, is online. It answers
/// each ask, and `pushes` says whether it also pushes its reach.
struct Pusher {
    peer: EndpointId,
    pushes: bool,
    asks: Arc<std::sync::atomic::AtomicUsize>,
}

impl World for Pusher {
    fn me(&self) -> EndpointId {
        iroh::SecretKey::from_bytes(&[1; 32]).public()
    }

    fn projects(&self) -> Vec<Project> {
        let mut project = project("far", "far/repo");
        project.machines = [self.peer].into();
        vec![project]
    }

    fn online(&self) -> BTreeSet<EndpointId> {
        [self.peer].into()
    }

    fn ask(&self, _: EndpointId) -> impl Future<Output = Result<ForgeReach>> + Send + 'static {
        self.asks.fetch_add(1, SeqCst);
        async { Ok(ForgeReach::default()) }
    }

    fn pushes(&self, _: EndpointId) -> bool {
        self.pushes
    }

    async fn changed(&mut self) -> bool {
        pending().await
    }

    async fn pushed(&self, _: &Project) -> Option<SystemTime> {
        None
    }
}

/// How many times the peer is asked over three `QUIET` spans with nothing held.
async fn asks_while_parked(pushes: bool) -> usize {
    let dir = TempDir::new().expect("a temp dir");
    let asks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let world = Pusher {
        peer: iroh::SecretKey::from_bytes(&[2; 32]).public(),
        pushes,
        asks: asks.clone(),
    };
    let watcher = Watcher::start(Idle, world, Arc::new(Ci::default()), dir.path().join("runs.json")).await;
    let task = tokio::spawn(watcher.run());

    tokio::time::sleep(QUIET * 3 + Duration::from_secs(1)).await;
    task.abort();

    asks.load(SeqCst)
}

// A machine that pushes its reach is not asked again while nothing is held;
// one from before 0.8.28 is, every `QUIET`.
#[tokio::test(start_paused = true)]
async fn a_machine_that_pushes_its_reach_is_not_asked_again() {
    assert_eq!(asks_while_parked(true).await, 1, "asked once, then followed");
    assert!(asks_while_parked(false).await >= 3, "an old machine is asked every QUIET");
}
