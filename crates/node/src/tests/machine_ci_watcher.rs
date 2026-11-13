use super::*;
use crate::machine::ci::check::Answer;
use std::future::pending;
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

    async fn ask(&self, _: EndpointId) -> Result<ForgeReach> {
        bail!("nobody")
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
    watcher.waited(cancelled).await;

    assert_eq!(watcher.waiting.get(&key).map(AbortHandle::id), Some(second));
    assert_eq!(watcher.tasks.get(&second), Some(&key));
    assert_eq!(watcher.tasks.len(), 1);
}
