use super::*;
use crate::files::sync::STOPPING;
use crate::{files::Folders, files::paths, files::syncthing::Syncthing};
use futures::FutureExt;
use iroh::{SecretKey, endpoint::presets};
use tempfile::TempDir;

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
            break;
        }

        seen.changed().await.expect("the actor runs");
    }

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
    rig.syncs.shutdown().await;

    assert!(rig.syncs.status().is_empty());
    let refused = rig.syncs.configure(vec![rig.project.clone()]).await;
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].message, STOPPING);
    rig.transfers.shutdown().await.expect("the node stops");
}
