//! Moving an account from a server into a mesh. The server is a fake that answers the reads;
//! the mesh is real, on loopback.

use async_trait::async_trait;
use beep_fleet::{
    FleetError, Result,
    migrate::{self, Report, Source},
};
use beep_mesh::{Key, Mesh, transport_config};
use beep_proto::{
    Credential, CredentialKind, FlshSettings, HarnessEnv, HarnessSettings, ImportedProject,
    InstructionsVersion, MachineHarnessSettings, MachineSync, ProjectGroup, ProjectSync,
    SyncStatus,
    node::mesh_state::{StateValue, SyncSpec},
};
use iroh::{Endpoint, EndpointId, SecretKey, endpoint::presets};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};
use tempfile::TempDir;
use uuid::Uuid;

const ACCOUNT: &str = "server-one";

async fn mesh(dir: &TempDir) -> Mesh {
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("endpoint binds");
    Mesh::create(dir.path(), endpoint, "Studio")
        .await
        .expect("mesh is created")
}

fn machine() -> EndpointId {
    SecretKey::generate().public()
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn harness_settings(machine: EndpointId, program: &str, args: &[&str]) -> MachineHarnessSettings {
    MachineHarnessSettings {
        machine_id: machine.to_string(),
        machine_name: "box".into(),
        settings: HarnessSettings {
            enabled: true,
            program: program.into(),
            home: String::new(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: vec![HarnessEnv {
                name: "OPENAI_API_KEY".into(),
                value: None,
            }],
        },
    }
}

struct Server {
    account: &'static str,
    box_id: EndpointId,
    group: Uuid,
    down: AtomicBool,
}

impl Server {
    fn new() -> Self {
        Self {
            account: ACCOUNT,
            box_id: machine(),
            group: Uuid::new_v4(),
            down: AtomicBool::new(false),
        }
    }

    fn reach(&self) -> Result<()> {
        if self.down.load(Ordering::Relaxed) {
            return Err(FleetError::NeedsServer);
        }
        Ok(())
    }
}

#[async_trait]
impl Source for Server {
    async fn account(&self) -> Result<String> {
        self.reach()?;
        Ok(self.account.to_owned())
    }

    async fn imports(&self) -> Result<Vec<ImportedProject>> {
        self.reach()?;
        Ok(vec![ImportedProject {
            project: "acme/widget".into(),
            url: "https://github.com/acme/widget".into(),
            machine_id: self.box_id.to_string(),
            machine_name: "build-box".into(),
            folder: "/srv/projects/acme/widget".into(),
            cloned: false,
            imported_at: at(1_700_000_000),
        }])
    }

    async fn sync(&self) -> Result<Vec<ProjectSync>> {
        self.reach()?;
        Ok(vec![ProjectSync {
            project: "acme/widget".into(),
            pinned: false,
            machines: vec![MachineSync {
                machine_id: self.box_id.to_string(),
                machine_name: "build-box".into(),
                folder: "/srv/projects/acme/widget".into(),
                status: SyncStatus::InSync {},
                files: 0,
                bytes: 0,
                pending: 0,
                conflicts: Vec::new(),
                rescan_secs: None,
                hint: None,
                error: None,
            }],
        }])
    }

    async fn groups(&self) -> Result<Vec<ProjectGroup>> {
        self.reach()?;
        Ok(vec![ProjectGroup {
            id: self.group,
            name: "work".into(),
            paths: vec!["work".into()],
        }])
    }

    async fn on_top(&self) -> Result<Vec<String>> {
        self.reach()?;
        Ok(vec!["acme/widget".into()])
    }

    async fn credentials(&self) -> Result<Vec<Credential>> {
        self.reach()?;
        Ok(vec![Credential {
            id: "7".into(),
            kind: CredentialKind::GitHub {},
            last4: "9f3a".into(),
            account: Some("octo".into()),
            env_var: "GH_TOKEN".into(),
            to_machines: true,
            created_at: at(1_700_000_100),
            unreadable: false,
        }])
    }

    async fn harness(&self, agent: &str) -> Result<Vec<MachineHarnessSettings>> {
        self.reach()?;
        Ok(match agent {
            "codex" => vec![harness_settings(self.box_id, "/opt/bin/codex", &["--fast"])],
            _ => Vec::new(),
        })
    }

    async fn flsh(&self) -> Result<FlshSettings> {
        self.reach()?;
        Ok(FlshSettings {
            zdr: Some(true),
            ..FlshSettings::default()
        })
    }

    async fn instructions(&self) -> Result<Vec<InstructionsVersion>> {
        self.reach()?;
        Ok(vec![
            InstructionsVersion {
                version: 1,
                text: "Be brief.".into(),
                note: String::new(),
                saved_at: at(1_700_000_200),
            },
            InstructionsVersion {
                version: 2,
                text: "Be brief and plain.".into(),
                note: String::new(),
                saved_at: at(1_700_000_300),
            },
        ])
    }
}

fn snapshot(mesh: &Mesh) -> Vec<(String, Vec<u8>)> {
    let mut all: Vec<_> = mesh
        .scan("")
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
    all.sort();
    all
}

#[tokio::test]
async fn moving_an_account_keeps_its_projects_and_credentials() {
    let dir = TempDir::new().expect("temp dir");
    let mesh = mesh(&dir).await;
    let server = Server::new();
    let report = migrate::run(&mesh, &server).await.expect("moves");
    assert!(!report.already);
    assert_eq!(report.unmoved, 1);

    let place = format!("project/acme/widget/{}", server.box_id);
    let keys: Vec<String> = snapshot(&mesh).into_iter().map(|(key, _)| key).collect();
    for want in [
        "project/acme/widget".to_owned(),
        place,
        "sync/acme/widget".to_owned(),
        format!("group/{}", server.group),
        "pin/acme/widget".to_owned(),
        "flsh".to_owned(),
        "harness/codex".to_owned(),
        format!("harness/codex/{}", server.box_id),
        "unmoved/credential/7".to_owned(),
        "unmoved/harness/codex/OPENAI_API_KEY".to_owned(),
    ] {
        assert!(keys.contains(&want), "{want} is in the mesh: {keys:?}");
    }
    assert_eq!(
        keys.iter()
            .filter(|key| key.starts_with("instructions/"))
            .count(),
        2
    );
    let spec = SyncSpec::from_bytes(&mesh.get(&Key::Sync("acme/widget".into())).expect("sync"))
        .expect("reads");
    assert_eq!(
        spec.folders.get(&server.box_id).map(String::as_str),
        Some("/srv/projects/acme/widget")
    );

    // No secret travels: the mesh holds no credential value, only the record with its last4.
    assert!(mesh.scan("credential/").is_empty());
    // The harness record names the variable and holds no value.
    let var = mesh
        .get(&Key::Unmoved("harness/codex/OPENAI_API_KEY".into()))
        .expect("the variable's record");
    assert!(!String::from_utf8_lossy(&var).contains("sk-"));
    let record = mesh
        .get(&Key::Unmoved("credential/7".into()))
        .expect("the record");
    assert!(String::from_utf8_lossy(&record).contains("9f3a"));
}

#[tokio::test]
async fn running_the_move_twice_changes_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let mesh = mesh(&dir).await;
    let server = Server::new();
    let first = migrate::run(&mesh, &server).await.expect("moves");
    assert!(first.written > 0);
    let after_first = snapshot(&mesh);
    let second = migrate::run(&mesh, &server).await.expect("moves");
    assert_eq!(
        second,
        Report {
            already: true,
            written: 0,
            unmoved: 0
        }
    );
    assert_eq!(snapshot(&mesh), after_first);
}

#[tokio::test]
async fn a_newer_mesh_value_is_never_overwritten() {
    let dir = TempDir::new().expect("temp dir");
    let mesh = mesh(&dir).await;
    let server = Server::new();
    // The user changed these in the mesh before the move ran.
    let newer = SyncSpec {
        folders: [(server.box_id, "/elsewhere".to_owned())].into(),
        paused: true,
    }
    .to_bytes()
    .expect("encodes");
    mesh.set(Key::Sync("acme/widget".into()), Some(newer.clone()))
        .await
        .expect("written");
    let flsh = FlshSettings {
        zdr: Some(false),
        ..FlshSettings::default()
    }
    .to_bytes()
    .expect("encodes");
    mesh.set(Key::Flsh, Some(flsh.clone()))
        .await
        .expect("written");
    mesh.set(Key::InstructionVersion("1.local".into()), Some(vec![1]))
        .await
        .expect("written");

    migrate::run(&mesh, &server).await.expect("moves");

    assert_eq!(mesh.get(&Key::Sync("acme/widget".into())), Some(newer));
    assert_eq!(mesh.get(&Key::Flsh), Some(flsh));
    // Instructions saved in the mesh first stay the only ones.
    assert_eq!(mesh.scan("instructions/").len(), 1);
}

#[tokio::test]
async fn a_server_that_cannot_be_reached_leaves_the_mesh_alone_and_the_next_try_moves() {
    let dir = TempDir::new().expect("temp dir");
    let mesh = mesh(&dir).await;
    let server = Server::new();
    let before = snapshot(&mesh);
    server.down.store(true, Ordering::Relaxed);
    migrate::run(&mesh, &server)
        .await
        .expect_err("the server is down");
    assert_eq!(snapshot(&mesh), before);

    server.down.store(false, Ordering::Relaxed);
    let report = migrate::run(&mesh, &server).await.expect("moves");
    assert!(!report.already);
    assert!(report.written > 0);
}

#[tokio::test]
async fn two_logins_on_one_server_are_two_moves() {
    let dir = TempDir::new().expect("temp dir");
    let mesh = mesh(&dir).await;
    let first = Server::new();
    migrate::run(&mesh, &first).await.expect("moves");
    let second = Server {
        account: "account-two",
        ..Server::new()
    };
    let other = migrate::run(&mesh, &second).await.expect("moves");
    assert!(!other.already);
    // The same login again is done.
    assert!(migrate::run(&mesh, &first).await.expect("moves").already);
}

#[tokio::test]
async fn a_join_carrying_a_moved_account_keeps_the_meshes_newer_values() {
    // The lone mesh moved an account; the mesh being joined has its own newer values.
    let (lone_dir, joined_dir) = (TempDir::new().expect("dir"), TempDir::new().expect("dir"));
    let (lone, joined) = (mesh(&lone_dir).await, mesh(&joined_dir).await);
    let server = Server::new();
    migrate::run(&lone, &server).await.expect("moves");

    let flsh = FlshSettings {
        zdr: Some(false),
        ..FlshSettings::default()
    }
    .to_bytes()
    .expect("encodes");
    joined
        .set(Key::Flsh, Some(flsh.clone()))
        .await
        .expect("written");
    // A pin the user removed in the joined mesh stays removed.
    joined
        .set(Key::Pin("acme/widget".into()), Some(vec![1]))
        .await
        .expect("written");
    joined
        .set(Key::Pin("acme/widget".into()), None)
        .await
        .expect("deleted");

    for (key, value) in lone.scan("") {
        joined.set_if_new(key, value).await.expect("carried");
    }

    assert_eq!(joined.get(&Key::Flsh), Some(flsh));
    assert_eq!(joined.get(&Key::Pin("acme/widget".into())), None);
    // What the joined mesh lacked came along.
    assert!(joined.get(&Key::Sync("acme/widget".into())).is_some());
    assert!(joined.get(&Key::Project("acme/widget".into())).is_some());
}
