use super::*;
use beep_mesh::transport_config;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use tempfile::TempDir;

#[tokio::test]
async fn selected_ignored_folders_are_independent_datasets_with_the_same_peers_and_pause() {
    use beep_proto::node::sync::IgnoredSync;
    let (mesh, _dir) = lone_mesh().await;
    let me = mesh.me();
    let spec = SyncSpec {
        folders: [(me, "/srv/project".to_owned())].into(),
        paused: true,
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
    let (configured, _) = projects(&mesh, me, &mut BTreeMap::new());
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
async fn an_api_key_in_the_mesh_is_not_taken() {
    let (mesh, _dir) = lone_mesh().await;
    let env = |name: &str| {
        Credential::Env(beep_proto::node::EnvSecret {
            name: name.to_owned(),
            value: beep_proto::SecretText::new("placeholder"),
        })
    };
    for (id, value) in [
        ("api.1.openrouter", env(ApiProvider::OpenRouter.env_var())),
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
    assert_eq!(names, [beep_proto::GITHUB_ENV_VAR]);
}

#[tokio::test]
async fn a_project_naming_a_machine_that_has_not_joined_is_pending() {
    let (mesh, _dir) = lone_mesh().await;
    let (me, other) = (mesh.me(), SecretKey::generate().public());
    let spec = |ids: &[EndpointId]| SyncSpec {
        ignored: Default::default(),
        folders: ids.iter().map(|id| (*id, "/srv/app".to_owned())).collect(),
        paused: false,
    };

    for (name, ids) in [("alone", vec![me]), ("waiting", vec![me, other])] {
        let bytes = spec(&ids).to_bytes().expect("a spec encodes");
        mesh.set(Key::Sync(name.to_owned()), Some(bytes))
            .await
            .expect("written");
    }

    let (projects, pending) = projects(&mesh, me, &mut BTreeMap::new());
    assert_eq!(projects.len(), 2);
    assert_eq!(pending, BTreeSet::from(["waiting".to_owned()]));
    assert!(projects.iter().all(|project| project.peers.is_empty()));
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
