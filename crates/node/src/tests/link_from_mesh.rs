use super::*;
use beep_proto::node::mesh_state::Credential;
use beep_mesh::transport_config;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use crate::link::peers::Peers;
use tempfile::TempDir;

#[tokio::test]
async fn the_master_pause_applies_to_all_projects_and_ignored_datasets() {
    use beep_proto::node::sync::IgnoredSync;
    let (mesh, _dir) = lone_mesh().await;
    let me = mesh.me();
    let spec = SyncSpec {
        folders: [(me, "/srv/project".into())].into(),
        ignored: IgnoredSync {
            enabled: true,
            folders: vec!["assets".into()],
        },
        ..SyncSpec::default()
    };
    mesh.set(
        Key::Sync("acme/widget".into()),
        Some(spec.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    let mut specs = BTreeMap::new();
    assert!(projects(&mesh, me, &mut specs).iter().all(|p| !p.paused));
    mesh.set(
        Key::ProjectSync,
        Some(
            ProjectSyncSettings {
                enabled: false,
                ..ProjectSyncSettings::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    let configured = projects(&mesh, me, &mut specs);
    assert_eq!(configured.len(), 2);
    assert!(configured.iter().all(|p| p.paused));
    mesh.set(
        Key::ProjectSync,
        Some(ProjectSyncSettings::default().to_bytes().unwrap()),
    )
    .await
    .unwrap();
    assert!(projects(&mesh, me, &mut specs).iter().all(|p| !p.paused));
}

#[tokio::test]
async fn selected_ignored_folders_are_independent_datasets_with_the_same_peers_and_pause() {
    use beep_proto::node::sync::IgnoredSync;
    let (mesh, _dir) = lone_mesh().await;
    let me = mesh.me();
    let spec = SyncSpec {
        folders: [(me, "/srv/project".to_owned())].into(),
        paused: true,
        rules: String::new(),
        ignored: IgnoredSync {
            enabled: true,
            folders: vec!["assets/models".into()],
        },
    };
    mesh.set(
        Key::Sync("acme/widget".into()),
        Some(spec.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    let configured = projects(&mesh, me, &mut BTreeMap::new());
    assert_eq!(configured.len(), 2);
    assert_eq!(configured[0].name, "acme/widget");
    assert_eq!(
        configured[1].name,
        ignored_project("acme/widget", "assets/models")
    );
    assert_eq!(configured[1].folder, "/srv/project/assets/models");
    assert!(configured.iter().all(|p| p.paused));
}

async fn lone_mesh() -> (Mesh, TempDir) {
    let dir = TempDir::new().expect("a temp folder");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds");
    let mesh = Mesh::create(dir.path(), endpoint, "desktop")
        .await
        .expect("the mesh is made");
    (mesh, dir)
}

#[tokio::test]
async fn an_api_key_in_the_mesh_reaches_the_processes() {
    let (mesh, _dir) = lone_mesh().await;
    let env = |name: &str| {
        Credential::Env(beep_proto::node::EnvSecret {
            name: name.to_owned(),
            value: beep_proto::SecretText::new("placeholder"),
        })
    };
    for (id, value) in [
        (
            "key.1.openrouter",
            env(beep_proto::ApiKey::OpenRouter.env_var()),
        ),
        ("gh.2.octo", env(beep_proto::GITHUB_ENV_VAR)),
    ] {
        let bytes = value.to_bytes().expect("a credential encodes");
        mesh.set(Key::Credential(id.to_owned()), Some(bytes))
            .await
            .expect("the credential is set");
    }
    let names: Vec<String> = credentials(&mesh)
        .env
        .into_iter()
        .map(|secret| secret.name)
        .collect();
    assert_eq!(names, [beep_proto::GITHUB_ENV_VAR, "OPENROUTER_API_KEY"]);
}

fn config(agent: &str, program: Option<&str>) -> HarnessConfig {
    HarnessConfig {
        program: program.map(ToOwned::to_owned),
        ..HarnessConfig::new(agent)
    }
}

#[test]
fn a_program_path_reaches_only_the_device_it_is_for() {
    let named = || {
        vec![
            ("codex".to_owned(), config("codex", None)),
            (
                "codex/mac".to_owned(),
                config("codex", Some("/Users/a/bin/codex")),
            ),
            (
                "codex/linux".to_owned(),
                config("codex", Some("/usr/local/bin/codex")),
            ),
        ]
    };
    let on_linux = harnesses_for("linux", named());
    assert_eq!(on_linux.len(), 1);
    assert_eq!(on_linux[0].program.as_deref(), Some("/usr/local/bin/codex"));
    let elsewhere = harnesses_for("windows", named());
    assert_eq!(elsewhere.len(), 1);
    assert_eq!(elsewhere[0].program, None);
    // A path left in the shared value is nobody's.
    let stray = vec![(
        "codex".to_owned(),
        config("codex", Some("/Users/a/bin/codex")),
    )];
    assert_eq!(harnesses_for("linux", stray)[0].program, None);
}

#[tokio::test]
async fn instructions_use_the_latest_saved_version_including_an_empty_one() {
    let (mesh, _dir) = lone_mesh().await;
    assert_eq!(instructions(&mesh), None);
    for (key, saved_ms, text) in [
        ("1.a", 1, "Old text"),
        ("2.a", 2, "New text"),
        ("2.b", 2, ""),
    ] {
        let saved = SavedVersion {
            text: text.into(),
            note: String::new(),
            saved_ms,
        };
        mesh.set(
            Key::InstructionVersion(key.into()),
            Some(saved.to_bytes().unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(instructions(&mesh).as_deref(), Some(text));
    }
}

/// What `run` holds of the follower set: its task, once it is started.
fn following(peers: &Peers) -> Option<tokio::task::Id> {
    crate::lock(&peers.inner.follower)
        .as_ref()
        .map(|follower| follower.abort_handle().id())
}

/// Waits, in the test's own time, for the follower set to change from `was`.
async fn changed_from(peers: &Peers, was: Option<tokio::task::Id>) -> Option<tokio::task::Id> {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let now = following(peers);

            if now != was {
                return now;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the follower set changed")
}

#[tokio::test]
async fn the_follower_set_starts_on_install_restarts_on_transfers_and_ends_on_leave() {
    use crate::files::{Folders, syncthing::Syncthing};
    use crate::link::Offers;

    let dir = TempDir::new().expect("a temp folder");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds");
    let mesh = Mesh::create(dir.path(), endpoint.clone(), "desktop")
        .await
        .expect("the mesh is made");
    let peers = Peers::new(Offers::default().bound(&endpoint, dir.path(), "box")).await;
    assert!(following(&peers).is_none());

    peers.install(mesh);
    let started = changed_from(&peers, None).await;
    assert!(started.is_some());

    let transfers = crate::files::transfer::Transfers::start_with(
        &endpoint,
        &dir.path().join("blobs"),
        Folders::new(&[]),
        Syncthing::at(vec![dir.path().join("syncthing")], None),
    )
    .await
    .expect("the transfers start");
    peers.serve_transfers(transfers.clone());
    let restarted = changed_from(&peers, started).await;
    assert!(restarted.is_some());

    peers.leave().await;
    assert!(following(&peers).is_none());
    transfers.shutdown().await.expect("the transfers stop");
}
