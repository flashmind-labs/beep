use super::*;
use crate::files::sync::Syncs;
use crate::{files::Folders, files::syncthing::Syncthing, fs::paths};
use futures::FutureExt;
use iroh::{SecretKey, endpoint::presets};
use std::{path::Path, time::Duration};
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
    let mut seen = rig.syncs.inner.projects.subscribe();

    // One poll sends the change; the caller is gone before the reply.
    assert!(rig.syncs.start_over("proj").now_or_never().is_none());

    let waiting = async {
        loop {
            assert!(
                rig.syncs
                    .status()
                    .iter()
                    .any(|status| status.name == "proj"),
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
    let actor = &rig.syncs.inner.projects;
    let running = actor.take("proj").expect("it runs");

    assert!(
        rig.syncs
            .status()
            .iter()
            .any(|status| status.name == "proj")
    );
    assert!(rig.syncs.project("proj").is_none());
    assert!(matches!(
        actor.seen().get("proj").map(|entry| &entry.phase),
        Some(Phase::Stopping(_))
    ));

    actor.start_run(&rig.project, Source::Direct, running);
    assert!(rig.syncs.project("proj").is_some());
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_change_after_the_shutdown_is_refused() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.projects;
    let held = Project {
        name: "held".to_owned(),
        ..rig.project.clone()
    };
    let stopped = Project {
        name: "stopped".to_owned(),
        ..rig.project.clone()
    };
    actor.put(
        &held,
        Source::Direct,
        Phase::Held("by Syncthing".to_owned()),
    );
    actor.put(&stopped, Source::Direct, Phase::Stopped("gone".to_owned()));
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
    let actor = &rig.syncs.inner.projects;
    // The steps of a configure that replaces the run, with the shutdown
    // arriving after the halt.
    let old = actor.take("proj").expect("it runs");
    let shared = halt(old).await;
    let file = shared.file().to_owned();
    assert!(file.exists(), "a halted run saved its state");

    rig.syncs.shutdown().await;

    actor.put(&rig.project, Source::Direct, Phase::Starting);
    let started = rig.syncs.run(&rig.project, Source::Direct).await;
    assert!(started.is_err(), "nothing loads once the node stops");
    rig.syncs.forget(&shared).await;

    assert!(file.exists(), "the state a run saves stays");
    assert!(rig.syncs.status().is_empty());
    assert!(actor.take("proj").is_none());
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_put_over_a_running_project_is_refused() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.projects;
    let shared = rig.syncs.project("proj").expect("it runs");

    actor.put(&rig.project, Source::Direct, Phase::Stopped("x".to_owned()));

    let now = rig.syncs.project("proj").expect("it still runs");
    assert!(Arc::ptr_eq(&shared, &now));
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn a_change_left_half_way_is_stopped_not_lost() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.projects;
    let shared = rig.syncs.project("proj").expect("it runs");
    let named = |name: &str| Project {
        name: name.to_owned(),
        ..rig.project.clone()
    };
    actor.put(&named("starting"), Source::Direct, Phase::Starting);
    actor.put(&named("stopping"), Source::Direct, Phase::Stopping(shared));
    actor.put(
        &named("held"),
        Source::Direct,
        Phase::Held("by Syncthing".to_owned()),
    );

    // A change that ends early, as by a panic, drops its gates.
    for name in ["starting", "stopping", "held"] {
        drop(actor.gate(name).await);
    }

    let phase = |name: &str| actor.seen()[name].phase.clone();
    assert!(matches!(phase("starting"), Phase::Stopped(_)));
    assert!(matches!(phase("stopping"), Phase::Stopped(_)));
    assert!(matches!(phase("held"), Phase::Held(_)));
    rig.transfers.shutdown().await.expect("the node stops");
}

#[tokio::test]
async fn changes_to_one_project_never_overlap_but_other_projects_do_not_wait() {
    let rig = rig().await;
    let actor = &rig.syncs.inner.projects;
    let first = actor.gate("proj").await;

    assert!(
        actor.gate("proj").now_or_never().is_none(),
        "a second change on the project waits"
    );
    assert!(
        actor.gate("other").now_or_never().is_some(),
        "a change on another project does not"
    );

    // The same through the API: a start over of a project that is held up.
    let mut waiting = std::pin::pin!(rig.syncs.start_over("proj"));
    assert!(waiting.as_mut().now_or_never().is_none());
    drop(first);
    timeout(Duration::from_secs(60), waiting)
        .await
        .expect("it ends once the first change did")
        .expect("it starts over");

    rig.transfers.shutdown().await.expect("the node stops");
}

// Syncthing takes a folder while a configure waits for the project's gate:
// the configure asks it again under the gate and does not start the project.
#[tokio::test]
async fn a_configure_does_not_start_a_project_syncthing_took_while_it_waited() {
    let rig = rig().await;
    let gate = rig.syncs.inner.projects.gate("proj").await;
    let changed = Project {
        rules: "*.log\n".to_owned(),
        ..rig.project.clone()
    };
    let syncs = rig.syncs.clone();
    let configure = tokio::spawn(async move { syncs.configure(vec![changed]).await });

    // Let it reach the gate; the marker comes after.
    for _ in 0..100 {
        tokio::task::spawn_blocking(|| ())
            .await
            .expect("the pool runs");
        tokio::task::yield_now().await;
    }

    std::fs::write(Path::new(&rig.project.folder).join(".stfolder"), "").expect("a marker");
    drop(gate);
    configure.await.expect("the configure ends");

    assert!(
        rig.syncs.project("proj").is_none(),
        "Syncthing keeps the folder"
    );
    rig.transfers.shutdown().await.expect("the node stops");
}
