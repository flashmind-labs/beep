use super::*;
use crate::files::sync::STOPPING;
use crate::{files::Folders, files::paths, files::syncthing::Syncthing};
use futures::FutureExt;
use std::time::Duration;
use iroh::{SecretKey, endpoint::presets};
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    syncs: Syncs,
    transfers: crate::files::transfer::Transfers,
    project: Project,
    _dirs: (TempDir, TempDir),
}

async fn rig() -> Rig {
    let shared = TempDir::new().expect("a temp folder");
    let base = paths::canonicalize(shared.path()).expect("it is there");
    let folder = base.join("proj");
    std::fs::create_dir(&folder).expect("a project folder");
    std::fs::write(folder.join("a.txt"), "one").expect("a file");
    let state = TempDir::new().expect("a temp folder");
    let endpoint = iroh::Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("an endpoint binds");
    let transfers = crate::files::transfer::Transfers::start_with(
        &endpoint,
        state.path(),
        Folders::new(std::slice::from_ref(&base)),
        Syncthing::at(vec![state.path().join("syncthing")], None),
    )
    .await
    .expect("the transfers start");
    let syncs = transfers.syncs();
    let project = Project {
        name: "proj".to_owned(),
        folder: folder.display().to_string(),
        peers: Vec::new(),
        paused: false,
        rules: String::new(),
    };
    let refused = syncs.configure(vec![project.clone()]).await;
    assert!(refused.is_empty(), "{refused:?}");
    Rig {
        syncs,
        transfers,
        project,
        _dirs: (shared, state),
    }
}

#[tokio::test]
async fn a_start_over_whose_caller_left_still_ends_running() {
    let rig = rig().await;
    let before = rig.syncs.project("proj").expect("it runs");
    let mut seen = rig.syncs.inner.actor.seen.clone();

    // One poll sends the change; the caller is gone before the reply.
    assert!(rig.syncs.start_over("proj").now_or_never().is_none());

    let waiting = async {
        loop {
            assert!(
                rig.syncs.status().iter().any(|status| status.name == "proj"),
                "a project in a change is still listed"
            );

            if rig
                .syncs
                .project("proj")
                .is_some_and(|now| !Arc::ptr_eq(&now, &before))
            {
                return;
            }

            seen.changed().await.expect("the actor runs");
        }
    };
    timeout(Duration::from_secs(60), waiting)
        .await
        .expect("the start over ends without its caller");

    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_project_being_stopped_is_listed_but_does_not_serve() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.actor;
    let running = actor.take("proj").await.expect("it runs");

    assert!(rig.syncs.status().iter().any(|status| status.name == "proj"));
    assert!(rig.syncs.project("proj").is_none());
    assert!(matches!(
        actor.seen().get("proj").map(|entry| &entry.phase),
        Some(Phase::Stopping(_))
    ));

    actor.start_run(&rig.project, Source::Direct, running).await;
    assert!(rig.syncs.project("proj").is_some());
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_change_after_the_shutdown_is_refused() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.actor;
    let held = Project {
        name: "held".to_owned(),
        ..rig.project.clone()
    };
    let stopped = Project {
        name: "stopped".to_owned(),
        ..rig.project.clone()
    };
    actor
        .put(&held, Source::Direct, Phase::Held("by Syncthing".to_owned()))
        .await;
    actor
        .put(&stopped, Source::Direct, Phase::Stopped("gone".to_owned()))
        .await;
    assert_eq!(rig.syncs.status().len(), 3);
    rig.syncs.shutdown().await;

    assert!(rig.syncs.status().is_empty());
    let refused = rig.syncs.configure(vec![rig.project.clone()]).await;
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].message, STOPPING);
    assert!(rig.syncs.status().is_empty());
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_shutdown_in_the_middle_of_a_change_keeps_the_state_and_starts_nothing() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.actor;
    // The steps of a configure that replaces the run, with the shutdown
    // arriving after the halt.
    let old = actor.take("proj").await.expect("it runs");
    let shared = halt(old).await;
    let file = shared.file().to_owned();
    assert!(file.exists(), "a halted run saved its state");

    rig.syncs.shutdown().await;

    actor.put(&rig.project, Source::Direct, Phase::Starting).await;
    let started = rig.syncs.run(&rig.project, Source::Direct).await;
    assert!(started.is_err(), "nothing loads once the node stops");
    rig.syncs.forget(&shared).await;

    assert!(file.exists(), "the state a run saves stays");
    assert!(rig.syncs.status().is_empty());
    assert!(actor.take("proj").await.is_none());
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_put_over_a_running_project_is_refused() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.actor;
    let shared = rig.syncs.project("proj").expect("it runs");

    actor
        .put(&rig.project, Source::Direct, Phase::Stopped("x".to_owned()))
        .await;

    let now = rig.syncs.project("proj").expect("it still runs");
    assert!(Arc::ptr_eq(&shared, &now));
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_change_left_half_way_is_stopped_not_lost() {
    let rig = rig().await;
    let shared = rig.syncs.project("proj").expect("it runs");
    let entry = |name: &str, phase| Entry {
        project: Project {
            name: name.to_owned(),
            ..rig.project.clone()
        },
        from: Source::Direct,
        phase,
    };
    let (seen, _) = watch::channel(Seen::default());
    let mut actor = Actor {
        map: BTreeMap::new(),
        tasks: BTreeMap::new(),
        seen,
        inner: Weak::new(),
        queue: VecDeque::new(),
        ops: JoinSet::new(),
        halts: JoinSet::new(),
        closing: Arc::default(),
    };
    actor
        .map
        .insert("starting".to_owned(), entry("starting", Phase::Starting));
    actor.map.insert(
        "stopping".to_owned(),
        entry("stopping", Phase::Stopping(shared)),
    );
    actor.map.insert(
        "held".to_owned(),
        entry("held", Phase::Held("by Syncthing".to_owned())),
    );

    actor.settle();

    assert!(matches!(actor.map["starting"].phase, Phase::Stopped(_)));
    assert!(matches!(actor.map["stopping"].phase, Phase::Stopped(_)));
    assert!(matches!(actor.map["held"].phase, Phase::Held(_)));
    rig.transfers.shutdown().await.expect("the node stops");
}
