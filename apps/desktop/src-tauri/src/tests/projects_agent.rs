use super::*;
use beep_proto::FoundPlace;
use std::time::SystemTime;

fn found(project: &str, machine: &str) -> FoundProject {
    FoundProject {
        project: project.to_owned(),
        remote: format!("https://example.com/{project}"),
        places: vec![FoundPlace {
            machine_id: machine.to_owned(),
            machine_name: "build-box".to_owned(),
            folder: format!("/srv/{project}"),
        }],
        agents: Vec::new(),
        last_activity: SystemTime::UNIX_EPOCH,
    }
}

#[test]
fn create_defaults_private() {
    let action: Action = serde_json::from_str(
        r#"{"action": "create", "name": "notes", "machines": ["build-box"],
            "repository": {"host": "github.com", "owner": "someone"}}"#,
    )
    .unwrap();
    let Action::Create {
        repository: Some(repository),
        ..
    } = action
    else {
        panic!("not a create with a repository");
    };

    assert_eq!(
        NewRepository::from(repository),
        NewRepository {
            host: "github.com".to_owned(),
            owner: "someone".to_owned(),
            private: true,
        }
    );
}

#[tokio::test]
async fn import_reports_imported_and_skipped() {
    let list = [found("org/app", "m1"), found("org/lib", "m1")];
    let names = ["org/app".to_owned(), "org/gone".to_owned()];

    let reply = import_found(&list, &names, &[], |repo, place| async move {
        Ok(ImportedProject {
            project: repo.trim_start_matches("https://example.com/").to_owned(),
            url: repo,
            machine_id: place.machine_id,
            machine_name: place.machine_name,
            folder: place.folder,
            cloned: false,
            imported_at: SystemTime::UNIX_EPOCH,
            display_name: None,
        })
    })
    .await;

    assert_eq!(
        reply,
        serde_json::json!({
            "imported": [{"project": "org/app", "machine": "build-box", "folder": "/srv/org/app", "cloned": false}],
            "skipped": [{"project": "org/gone", "why": NOT_FOUND}],
            "failed": [],
        })
    );
}
