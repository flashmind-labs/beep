use super::*;
use iroh::{Endpoint, SecretKey, endpoint::presets};

fn result(reply: Reply) -> serde_json::Value {
    match reply {
        Reply::Done {
            text,
            failed: false,
        } => serde_json::from_str(&text).unwrap(),
        other => panic!("unexpected reply: {other:?}"),
    }
}

#[tokio::test]
async fn projects_include_single_machine_imports_and_synced_folders() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint.clone(), "build-box")
        .await
        .unwrap();
    let me = mesh.me();

    mesh.set(
        Key::Project("example/app".into()),
        Some(
            ProjectRecord {
                url: "https://example.com/example/app".into(),
                display_name: Some("New project".into()),
                ..ProjectRecord::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    mesh.set(
        Key::Project(format!("example/app/{me}")),
        Some(
            Place {
                folder: "/srv/app".into(),
                imported_ms: 0,
                fork: None,
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    mesh.set(
        Key::Sync("notes".into()),
        Some(
            SyncSpec {
                folders: [(me, "/srv/notes".into())].into(),
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();

    let listed = result(call(Some(&mesh), "{}"));
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(listed[0]["name"], "example/app");
    assert_eq!(listed[0]["display_name"], "New project");
    assert_eq!(folder(&mesh, "new project", me).unwrap(), "/srv/app");
    assert_eq!(listed[0]["checkouts"][0]["cwd"], "/srv/app");
    assert_eq!(listed[1]["name"], "notes");
    assert_eq!(listed[1]["checkouts"][0]["machine"], "build-box");
    assert_eq!(folder(&mesh, " EXAMPLE/App ", me).unwrap(), "/srv/app");
    assert!(folder(&mesh, "missing", me).is_err());
    assert!(folder(&mesh, "example/app", SecretKey::generate().public()).is_err());
    let filtered = result(call(
        Some(&mesh),
        r#"{"project":"NOTES","machine":"Build Box"}"#,
    ));
    assert_eq!(filtered.as_array().unwrap().len(), 1);
    assert_eq!(filtered[0]["name"], "notes");
    assert!(matches!(
        call(Some(&mesh), r#"{"machine":"missing"}"#),
        Reply::Done { failed: true, .. }
    ));
    assert!(matches!(call(None, "{}"), Reply::Done { failed: true, .. }));

    // An agent's folder, a worktree included, names its project.
    let project = |cwd: &str| of_folder(&mesh, cwd, me);
    assert_eq!(
        project("/srv/app/.worktrees/fix").as_deref(),
        Some("example/app")
    );
    assert_eq!(project("/srv/notes").as_deref(), Some("notes"));
    assert_eq!(project("/srv/apple"), None);

    // A sync record takes precedence over an older import location.
    mesh.set(
        Key::Sync("example/app".into()),
        Some(
            SyncSpec {
                folders: [(me, "/srv/new-app".into())].into(),
                ..SyncSpec::default()
            }
            .to_bytes()
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(folder(&mesh, "example/app", me).unwrap(), "/srv/new-app");
    assert_eq!(
        result(call(Some(&mesh), r#"{"project":"example/app"}"#))[0]["checkouts"][0]["cwd"],
        "/srv/new-app"
    );
    endpoint.close().await;
}
