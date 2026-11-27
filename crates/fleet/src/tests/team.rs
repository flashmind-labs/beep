use super::*;
use crate::{
    Machines,
    records::{Place, ProjectRecord, place_key, project_key},
};
use beep_mesh::{Mesh, TeamWrites, take_roster, transport_config};
use beep_proto::team::Removed;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use tempfile::TempDir;

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
    let desk = Desk::open(Machines::for_tests(mesh.clone()))
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
        admitted_by: None,
        proof: None,
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
async fn ticket_names_its_repo_and_the_project_here() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, _mesh) = desk(&dir).await;
    let code = |name: &str| {
        TeamTicket {
            inviter: SecretKey::generate().public(),
            repo: repo(name),
            secret: JoinSecret::generate(),
        }
        .encode_string()
    };

    let here = desk.team_ticket(&code("org/web")).expect("read");
    let elsewhere = desk.team_ticket(&code("org/api")).expect("read");

    assert_eq!(here, (repo("org/web"), Some("org/web".to_owned())));
    assert_eq!(elsewhere, (repo("org/api"), None));
    assert!(desk.team_ticket("beepteam1nonsense").is_err());
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
        joined_at: Some(alice.joined_at),
    });
    seed(&mesh, &team).await;
    // A change from before alice was removed.
    let stale = TeamRecord::new(repo("org/web"), vec![team.members[0].clone(), alice]);
    let elsewhere = TeamRecord::new(repo("org/api"), stale.members.clone());

    assert!(
        !take_roster(&mesh, stale, &TeamWrites::lock().await)
            .await
            .expect("taken")
    );
    assert!(
        !take_roster(&mesh, elsewhere, &TeamWrites::lock().await)
            .await
            .expect("taken")
    );

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert_eq!(record, team);
    assert!(mesh.get(&Key::Team(repo("org/api"))).is_none());
}

/// A teammate let alice back in and admitted her new join: writing it
/// ends this mesh's removal of her earlier one.
#[tokio::test]
async fn a_later_join_ends_an_earlier_removal() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let alice = member("alice", SecretKey::generate().public());
    let mut team = TeamRecord::new(
        repo("org/web"),
        vec![member("dana", mesh.id()), alice.clone()],
    );
    team.remove(&alice.login);
    seed(&mesh, &team).await;
    let again = Member {
        joined_at: alice.joined_at + Duration::from_secs(60),
        ..alice
    };

    let change = TeamRecord::new(repo("org/web"), vec![again.clone()]);
    assert!(
        take_roster(&mesh, change, &TeamWrites::lock().await)
            .await
            .expect("taken")
    );

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert_eq!(record.members, [team.members[0].clone(), again]);
    assert!(record.removed.is_empty());
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

    keep_contacts(&mesh, &TeamWrites::lock().await)
        .await
        .expect("kept");

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
    keep_contacts(&mesh, &TeamWrites::lock().await)
        .await
        .expect("kept");
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

/// Two machines admit different people from the same record: the second
/// write keeps the first one's newcomer.
#[tokio::test]
async fn concurrent_admissions_both_stay() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id())]);
    seed(&mesh, &team).await;
    let (alice, bob) = (
        member("alice", SecretKey::generate().public()),
        member("bob", SecretKey::generate().public()),
    );
    let mut with_alice = team.clone();
    with_alice.members.push(alice.clone());
    let mut with_bob = team.clone();
    with_bob.members.push(bob.clone());

    for pending in [with_alice, with_bob] {
        assert!(
            take_roster(&mesh, pending, &TeamWrites::lock().await)
                .await
                .expect("taken")
        );
    }

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert_eq!(record.members, [team.members[0].clone(), alice, bob]);
}

/// A roster a node merges while the user changes a setting: neither undoes
/// the other.
#[tokio::test]
async fn settings_wait_for_a_roster_merge_in_progress() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id())]);
    seed(&mesh, &team).await;
    let mut joined = team.clone();
    joined
        .members
        .push(member("alice", SecretKey::generate().public()));

    let held = TeamWrites::lock().await;
    let mut settings = std::pin::pin!(desk.team_settings("org/web", false, None, true));
    assert!(futures::poll!(&mut settings).is_pending());
    assert!(take_roster(&mesh, joined, &held).await.expect("taken"));
    drop(held);
    settings.await.expect("set");

    let record = desk.team_record("org/web").expect("read").expect("a team");
    assert!(!record.pickup, "the setting stays");
    assert_eq!(record.members.len(), 2, "the newcomer stays");
}

/// A roster a node merges while the user leaves: the team does not come back.
#[tokio::test]
async fn a_roster_merge_does_not_bring_a_left_team_back() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id())]);
    seed(&mesh, &team).await;
    let mut joined = team.clone();
    joined
        .members
        .push(member("alice", SecretKey::generate().public()));

    let held = TeamWrites::lock().await;
    let mut leave = std::pin::pin!(desk.team_leave("org/web"));
    assert!(futures::poll!(&mut leave).is_pending());
    assert!(
        take_roster(&mesh, joined.clone(), &held)
            .await
            .expect("taken")
    );
    drop(held);
    leave.await.expect("left");

    assert_eq!(desk.team_record("org/web").expect("read"), None);
    let held = TeamWrites::lock().await;
    assert!(!take_roster(&mesh, joined, &held).await.expect("taken"));
    assert_eq!(desk.team_record("org/web").expect("read"), None);
}

#[tokio::test]
async fn a_project_without_a_team_has_no_team_cards() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, _mesh) = desk(&dir).await;

    assert_eq!(
        desk.team_cards("org/web").await.expect("cards"),
        TeamCards::default()
    );
    assert!(matches!(
        desk.team_dismiss("org/web", "ask:a1").await,
        Err(FleetError::NotFound)
    ));
}

/// A teammate removed this mesh: the record goes with the contacts only it
/// brought, and no invite is minted on it.
#[tokio::test]
async fn a_team_that_removed_this_mesh_is_left() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let alice = member("alice", SecretKey::generate().public());
    let mut team = TeamRecord::new(repo("org/web"), vec![member("dana", mesh.id()), alice]);
    seed(&mesh, &team).await;
    keep_contacts(&mesh, &TeamWrites::lock().await)
        .await
        .expect("kept");
    assert_eq!(desk.contacts().len(), 1);

    let mut removed = team.clone();
    removed.remove(&"dana".parse().expect("a login"));
    // An older desktop wrote it as it came: Invite starts nothing on it.
    seed(&mesh, &removed).await;
    assert!(matches!(
        desk.team_invite("org/web").await,
        Err(FleetError::Refused(_))
    ));
    seed(&mesh, &team).await;

    assert!(
        take_roster(&mesh, removed, &TeamWrites::lock().await)
            .await
            .expect("taken")
    );

    assert_eq!(desk.team_record("org/web").expect("read"), None);
    assert!(desk.contacts().is_empty());
    team.members.clear();
    assert!(
        !take_roster(&mesh, team, &TeamWrites::lock().await)
            .await
            .expect("taken")
    );
}

/// Removing a contact takes them off every team shared with them, and
/// forgets them, even when no node is there to tell the teams.
#[tokio::test]
async fn removing_a_contact_takes_them_off_every_shared_team() {
    let dir = TempDir::new().expect("a temp dir");
    let (desk, mesh) = desk(&dir).await;
    let me = member("dana", mesh.id());
    let alice = member("alice", SecretKey::generate().public());
    let mut alice_on_api = alice.clone();
    alice_on_api.login = "alice-work".parse().expect("a login");
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
        &TeamRecord::new(repo("org/api"), vec![me, alice_on_api.clone()]),
    )
    .await;
    keep_contacts(&mesh, &TeamWrites::lock().await)
        .await
        .expect("kept");

    desk.remove_contact(alice.mesh).await.expect("removed");

    let [contact] = &desk.contacts()[..] else {
        panic!("bob alone: {:?}", desk.contacts());
    };
    assert_eq!(contact.mesh, bob.mesh);
    for (name, login) in [("org/web", &alice.login), ("org/api", &alice_on_api.login)] {
        let record: TeamRecord = read(&mesh, &Key::Team(repo(name))).expect("a team");
        assert!(
            record
                .members
                .iter()
                .all(|member| member.mesh != alice.mesh)
        );
        assert!(record.removed.iter().any(|gone| gone.login == *login));
    }
    assert!(matches!(
        desk.remove_contact(alice.mesh).await,
        Err(FleetError::NotFound)
    ));
}
