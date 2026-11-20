use super::*;
use crate::{
    Peers,
    records::{Place, ProjectRecord, place_key, project_key},
};
use async_trait::async_trait;
use beep_mesh::{Mesh, PeerClient, take_roster, transport_config};
use beep_proto::team::Removed;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use std::sync::Arc;
use tempfile::TempDir;

/// A desktop alone in its mesh: no node answers it.
struct Alone(Mesh);

#[async_trait]
impl Peers for Alone {
    async fn mesh(&self) -> Result<Mesh> {
        Ok(self.0.clone())
    }

    fn me(&self) -> EndpointId {
        self.0.me()
    }

    async fn client(&self, _machine: EndpointId) -> Result<PeerClient> {
        Err(FleetError::Offline("the node".into()))
    }
}

/// A desk over a new mesh with `org/web` imported on this desktop.
async fn desk(dir: &TempDir) -> (Desk, Mesh) {
    let endpoint = Endpoint::builder(presets::Minimal)
        .transport_config(transport_config())
        .bind()
        .await
        .expect("an endpoint");
    let mesh = Mesh::create(dir.path(), endpoint, "desktop")
        .await
        .expect("a mesh");
    let record = ProjectRecord {
        url: "https://github.com/org/web".into(),
        ..ProjectRecord::default()
    };
    let place = Place {
        folder: "/srv/web".into(),
        imported_ms: 0,
        fork: None,
    };
    write(&mesh, project_key("org/web"), Some(&record))
        .await
        .expect("written");
    write(&mesh, place_key("org/web", mesh.me()), Some(&place))
        .await
        .expect("written");
    let desk = Desk::open(Arc::new(Alone(mesh.clone())))
        .await
        .expect("a desk");

    (desk, mesh)
}

fn repo(name: &str) -> TeamRepo {
    format!("github.com/{name}").parse().expect("a repo")
}

fn member(login: &str, mesh: EndpointId) -> Member {
    Member {
        login: login.parse().expect("a login"),
        mesh,
        devices: vec![EndpointAddr::new(SecretKey::generate().public())],
        joined_at: SystemTime::UNIX_EPOCH,
    }
}

async fn seed(mesh: &Mesh, record: &TeamRecord) {
    write(mesh, Key::Team(record.repo.clone()), Some(record))
        .await
        .expect("written");
}

#[tokio::test]
async fn invite_writes_open_invite_valid_a_day() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    seed(
        &mesh,
        &TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id())]),
    )
    .await;

    let code = desk.team_invite("org/web").await.expect("an invite");

    let ticket = TeamTicket::parse(&code).expect("a beepteam1 code");
    assert!(code.starts_with("beepteam1"));
    assert_eq!(ticket.repo, repo("org/web"));
    assert_eq!(ticket.inviter, mesh.me(), "the machine with the checkout");
    let record = desk.team_record("org/web").expect("read").expect("a team");
    let [invite] = &record.invites[..] else {
        panic!("one invite: {:?}", record.invites);
    };
    assert_eq!(invite.secret_hash, secret_hash(&ticket.secret));
    let left = invite
        .expires_at
        .duration_since(SystemTime::now())
        .expect("in the future");
    assert!(left > INVITE_FOR - Duration::from_secs(60) && left <= INVITE_FOR);
}

#[tokio::test]
async fn invite_drops_expired_invites() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let mut team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id())]);
    team.invites.push(OpenInvite {
        secret_hash: [7; 32],
        expires_at: SystemTime::now() - Duration::from_secs(1),
    });
    seed(&mesh, &team).await;

    desk.team_invite("org/web").await.expect("an invite");

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert_eq!(record.invites.len(), 1);
    assert_ne!(record.invites[0].secret_hash, [7; 32]);
}

#[tokio::test]
async fn join_without_import_says_import_first() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, _mesh) = desk(&dir).await;
    let code = TeamTicket {
        inviter: SecretKey::generate().public(),
        repo: repo("org/api"),
        secret: JoinSecret::generate(),
    }
    .encode_string();

    let refused = desk.team_join(&code).await.expect_err("not imported");

    assert!(matches!(&refused, FleetError::NotImported(of) if *of == repo("org/api")));
    assert_eq!(refused.to_string(), "Import github.com/org/api first");
}

#[tokio::test]
async fn roster_from_node_is_written_once() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let mut team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id())]);
    team.pickup = false;
    seed(&mesh, &team).await;
    // A node took a joiner, from the record it had before pickup was off.
    let mut pending = TeamRecord::new(repo("org/web"), team.members.clone());
    pending
        .members
        .push(member("alice", SecretKey::generate().public()));

    assert!(take_roster(&mesh, pending.clone()).await.expect("taken"));
    assert!(!take_roster(&mesh, pending).await.expect("taken"));

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert_eq!(record.members.len(), 2);
    assert!(!record.pickup, "the desktop's settings stay");
}

#[tokio::test]
async fn roster_keeps_removals_and_never_starts_a_team() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let alice = member("alice", SecretKey::generate().public());
    let mut team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id())]);
    team.removed.push(Removed {
        login: alice.login.clone(),
        mesh: alice.mesh,
    });
    seed(&mesh, &team).await;
    // A change from before alice was removed.
    let stale = TeamRecord::new(repo("org/web"), vec![team.members[0].clone(), alice]);
    let elsewhere = TeamRecord::new(repo("org/api"), stale.members.clone());

    assert!(!take_roster(&mesh, stale).await.expect("taken"));
    assert!(!take_roster(&mesh, elsewhere).await.expect("taken"));

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert_eq!(record, team);
    assert!(mesh.get(&Key::Team(repo("org/api"))).is_none());
}

#[tokio::test]
async fn contacts_follow_the_teams() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let me = member("dana", mesh.id());
    let alice = member("alice", SecretKey::generate().public());
    let mut alice_on_api = alice.clone();
    alice_on_api.login = "alice-work".parse().expect("a login");
    alice_on_api.devices = vec![EndpointAddr::new(SecretKey::generate().public())];
    seed(
        &mesh,
        &TeamRecord::new(repo("org/web"), vec![me.clone(), alice.clone()]),
    )
    .await;
    seed(
        &mesh,
        &TeamRecord::new(repo("org/api"), vec![me, alice_on_api.clone()]),
    )
    .await;

    keep_contacts(&mesh).await.expect("kept");

    let [contact] = &desk.contacts()[..] else {
        panic!("alice alone: {:?}", desk.contacts());
    };
    assert_eq!(contact.mesh, alice.mesh);
    let mut logins = contact.logins.clone();
    logins.sort();
    assert_eq!(
        logins,
        vec![
            (repo("org/api"), alice_on_api.login),
            (repo("org/web"), alice.login),
        ]
    );
    assert_eq!(contact.devices.len(), 2);
}

#[tokio::test]
async fn leave_deletes_key_and_contacts_only_for_that_team() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let me = member("dana", mesh.id());
    let alice = member("alice", SecretKey::generate().public());
    let bob = member("bob", SecretKey::generate().public());
    seed(
        &mesh,
        &TeamRecord::new(
            repo("org/web"),
            vec![me.clone(), alice.clone(), bob.clone()],
        ),
    )
    .await;
    seed(
        &mesh,
        &TeamRecord::new(repo("org/api"), vec![me, alice.clone()]),
    )
    .await;
    keep_contacts(&mesh).await.expect("kept");
    assert_eq!(desk.contacts().len(), 2);

    // No node answers: the teammates are not told, and the team is left.
    desk.team_leave("org/web").await.expect("left");

    assert_eq!(desk.team_record("org/web").expect("read"), None);
    let [contact] = &desk.contacts()[..] else {
        panic!("alice alone: {:?}", desk.contacts());
    };
    assert_eq!(contact.mesh, alice.mesh);
    assert_eq!(contact.logins, vec![(repo("org/api"), alice.login)]);
}

#[tokio::test]
async fn readmit_clears_the_removal() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let alice = member("alice", SecretKey::generate().public());
    let mut team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id()), alice]);
    team.remove(&"alice".parse().expect("a login"));
    team.remove(&"bob".parse().expect("a login"));
    seed(&mesh, &team).await;
    assert_eq!(team.removed.len(), 1);

    desk.team_readmit("org/web", &"alice".parse().expect("a login"))
        .await
        .expect("readmitted");

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert!(record.removed.is_empty());
    assert_eq!(record.members.len(), 1);
}
