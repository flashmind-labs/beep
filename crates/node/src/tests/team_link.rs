use super::*;
use crate::team::{
    home::{Checkout, Project},
    roster::{Refusal, Rosters, record},
};
use beep_mesh::{JoinSecret, TeamTicket, secret_hash};
use beep_proto::{
    node::tools::TeamRosterArgs,
    team::{ForgeLogin, Member, OpenInvite, TeamMsg, TeamRecord, TeamRepo},
};
use futures::future::BoxFuture;
use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey, address_lookup::memory::MemoryLookup,
    endpoint::presets, protocol::Router,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime},
};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A forge that answers without a network.
struct Forge {
    login: &'static str,
    collaborator: bool,
}

impl TeamForge for Forge {
    fn collaborator<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move { Ok(self.collaborator) })
    }

    fn login<'a>(&'a self, _repo: &'a TeamRepo) -> BoxFuture<'a, Result<ForgeLogin, String>> {
        Box::pin(async move { Ok(self.login.parse().unwrap()) })
    }
}

fn repo() -> TeamRepo {
    "github.com/org/repo".parse().unwrap()
}

fn login(name: &str) -> ForgeLogin {
    name.parse().unwrap()
}

fn mesh_id() -> EndpointId {
    SecretKey::generate().public()
}

fn member(name: &str, mesh: EndpointId, device: EndpointId) -> Member {
    Member {
        login: login(name),
        mesh,
        devices: vec![EndpointAddr::new(device)],
        joined_at: SystemTime::now(),
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// An inviter and a joiner on two endpoints, each with a checkout of one bare remote.
struct Pair {
    _root: tempfile::TempDir,
    remote: String,
    inviter: TeamLink,
    inviter_id: EndpointId,
    joiner: TeamLink,
    joiner_checkout: PathBuf,
    secret: JoinSecret,
    _router: Router,
}

impl Pair {
    async fn new(expires_at: SystemTime) -> Self {
        Self::with(expires_at, true).await
    }

    /// An inviter whose forge says whether the joiner is a collaborator.
    async fn with(expires_at: SystemTime, collaborator: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("remote.git");
        std::fs::create_dir(&bare).unwrap();
        git(&bare, &["init", "--bare"]);
        let remote = bare.to_string_lossy().into_owned();
        let checkout = |name: &str| {
            let dir = root.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            git(&dir, &["init"]);
            dir
        };
        let (inviter_checkout, joiner_checkout) = (checkout("alice"), checkout("bob"));

        let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
        let inviter_id = endpoint.id();
        let inviter_addr = endpoint.addr();
        let inviter = TeamLink::new(
            endpoint.clone(),
            Arc::new(Forge {
                login: "alice",
                collaborator,
            }),
        );
        let router = Router::builder(endpoint)
            .accept(beep_proto::team::ALPN, inviter.handler())
            .spawn();
        let secret = JoinSecret::generate();
        let alice_mesh = mesh_id();
        let mut team = record(repo(), vec![member("alice", alice_mesh, inviter_id)]);
        team.invites.push(OpenInvite {
            secret_hash: secret_hash(&secret),
            expires_at,
        });
        let folder = inviter_checkout.clone();
        let place = remote.clone();
        inviter.with(|teams| {
            teams.rosters.refresh([(repo(), team)].into());
            teams.home.mesh = Some(alice_mesh);
            teams.home.projects.insert(
                repo(),
                Project {
                    checkout: Some(Checkout {
                        folder,
                        remote: place,
                    }),
                    devices: vec![EndpointAddr::new(inviter_id)],
                },
            );
        });

        let joiner = joiner_link(&inviter_addr).await;

        Self {
            _root: root,
            remote,
            inviter,
            inviter_id,
            joiner,
            joiner_checkout,
            secret,
            _router: router,
        }
    }

    fn ticket(&self, secret: JoinSecret) -> TeamTicket {
        TeamTicket {
            inviter: self.inviter_id,
            repo: repo(),
            secret,
        }
    }

    async fn join(&self, secret: JoinSecret) -> Result<TeamRecord, TeamError> {
        self.joiner
            .join(self.ticket(secret), &self.joiner_checkout, &self.remote)
            .await
    }

    fn inviter_pending(&self) -> Vec<TeamRecord> {
        self.inviter.with(|teams| teams.rosters.pending())
    }
}

/// A joiner as `bob` in a mesh of its own, which finds the inviter by `inviter`'s address.
async fn joiner_link(inviter: &EndpointAddr) -> TeamLink {
    let lookup = MemoryLookup::new();
    lookup.add_endpoint_info(inviter.clone());
    let endpoint = Endpoint::builder(presets::Minimal)
        .address_lookup(lookup)
        .bind()
        .await
        .unwrap();
    let id = endpoint.id();
    let link = TeamLink::new(
        endpoint,
        Arc::new(Forge {
            login: "bob",
            collaborator: true,
        }),
    );
    link.with(|teams| {
        teams.home.mesh = Some(mesh_id());
        teams.home.projects.insert(
            repo(),
            Project {
                checkout: None,
                devices: vec![EndpointAddr::new(id)],
            },
        );
    });

    link
}

/// The proof refs `remote` holds.
fn proof_refs(remote: &str) -> Vec<u8> {
    Command::new("git")
        .args(["ls-remote", remote, "refs/beep/proof/*"])
        .output()
        .unwrap()
        .stdout
}

fn refused(result: Result<TeamRecord, TeamError>) -> String {
    match result {
        Err(TeamError::Refused(reason)) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn join_admits_with_proof() {
    let pair = Pair::new(SystemTime::now() + DAY).await;

    let joined = pair.join(pair.secret.clone()).await.unwrap();

    let logins: Vec<String> = joined.members.iter().map(|m| m.login.to_string()).collect();
    assert_eq!(logins, ["alice", "bob"]);
    let pending = pair.inviter_pending();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].members.iter().any(|m| m.login == login("bob")));
    assert!(proof_refs(&pair.remote).is_empty(), "the proof ref is deleted");
}

#[tokio::test]
async fn wrong_secret_is_refused() {
    let pair = Pair::new(SystemTime::now() + DAY).await;

    let reason = refused(pair.join(JoinSecret::generate()).await);

    assert!(reason.contains("not valid"), "{reason}");
    assert!(pair.inviter_pending().is_empty());
}

#[tokio::test]
async fn expired_invite_is_refused() {
    let pair = Pair::new(SystemTime::now() - Duration::from_secs(60)).await;

    let reason = refused(pair.join(pair.secret.clone()).await);

    assert!(reason.contains("expired"), "{reason}");
    assert!(pair.inviter_pending().is_empty());
}

#[tokio::test]
async fn non_collaborator_is_refused() {
    let pair = Pair::with(SystemTime::now() + DAY, false).await;

    let reason = refused(pair.join(pair.secret.clone()).await);

    assert!(reason.contains("not a collaborator"), "{reason}");
    assert!(proof_refs(&pair.remote).is_empty(), "the proof ref is deleted");
    assert!(pair.inviter_pending().is_empty());
}

#[tokio::test]
async fn missing_proof_is_refused() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let devices = vec![EndpointAddr::new(pair.joiner.endpoint().id())];

    let (mut tx, mut rx) = pair
        .joiner
        .dial(&[EndpointAddr::new(pair.inviter_id)])
        .await
        .unwrap();
    tx.send(&TeamMsg::Join {
        secret: *pair.secret.as_bytes(),
        repo: repo(),
        login: login("bob"),
        mesh: mesh_id(),
        devices,
    })
    .await
    .unwrap();
    let Some(TeamMsg::Challenge { nonce }) = rx.recv().await.unwrap() else {
        panic!("no challenge");
    };
    // Proved, without pushing the proof ref.
    tx.send(&TeamMsg::Proved { nonce }).await.unwrap();
    let answer: Option<TeamMsg> = rx.recv().await.unwrap();

    match answer {
        Some(TeamMsg::Refused { reason }) => {
            assert!(reason.contains("push access"), "{reason}")
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(pair.inviter_pending().is_empty());
}

#[tokio::test]
async fn stranger_connection_is_closed() {
    let pair = Pair::new(SystemTime::now() + DAY).await;

    let (mut tx, mut rx) = pair
        .joiner
        .dial(&[EndpointAddr::new(pair.inviter_id)])
        .await
        .unwrap();
    tx.send(&TeamMsg::Subscribe { repo: repo() }).await.unwrap();
    let answer = rx.recv::<TeamMsg>().await;

    assert!(
        !matches!(answer, Ok(Some(_))),
        "a stranger gets nothing: {answer:?}"
    );
}

#[tokio::test]
async fn invite_from_stranger_is_refused() {
    let pair = Pair::new(SystemTime::now() + DAY).await;

    let (mut tx, mut rx) = pair
        .joiner
        .dial(&[EndpointAddr::new(pair.inviter_id)])
        .await
        .unwrap();
    tx.send(&TeamMsg::Invite {
        repo: repo(),
        ticket: "beepteam1x".to_owned(),
    })
    .await
    .unwrap();

    assert!(matches!(
        rx.recv().await.unwrap(),
        Some(TeamMsg::Refused { .. })
    ));
    assert!(pair.inviter.with(|teams| teams.invites.is_empty()));
}

#[tokio::test]
async fn invite_from_contact_is_kept() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let contact = mesh_id();
    let device = pair.joiner.endpoint().id();
    pair.inviter.with(|teams| {
        teams.home.contacts.insert(device, contact);
    });

    let (mut tx, mut rx) = pair
        .joiner
        .dial(&[EndpointAddr::new(pair.inviter_id)])
        .await
        .unwrap();
    tx.send(&TeamMsg::Invite {
        repo: repo(),
        ticket: "beepteam1x".to_owned(),
    })
    .await
    .unwrap();
    tx.finish().await.unwrap();
    assert!(rx.recv::<TeamMsg>().await.unwrap().is_none());

    let invites = pair.inviter.with(|teams| teams.invites.clone());
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0].from, contact);
}

#[test]
fn removed_member_stays_out() {
    let (alice, carol) = (mesh_id(), mesh_id());
    let (alice_device, carol_device) = (mesh_id(), mesh_id());
    let team = record(
        repo(),
        vec![
            member("alice", alice, alice_device),
            member("carol", carol, carol_device),
        ],
    );
    let mut rosters = Rosters::default();
    rosters.refresh([(repo(), team.clone())].into());

    let removed = rosters
        .apply(
            alice_device,
            &TeamMsg::Remove {
                repo: repo(),
                login: login("carol"),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(removed.members.len(), 1);

    let stale = rosters.apply(
        alice_device,
        &TeamMsg::Roster {
            repo: repo(),
            members: team.members.clone(),
        },
    );
    assert!(matches!(stale, Ok(None)), "{stale:?}");
    let from_carol = rosters.apply(
        carol_device,
        &TeamMsg::Roster {
            repo: repo(),
            members: team.members.clone(),
        },
    );
    assert_eq!(from_carol, Err(Refusal::NotMember(repo())));
    let subscribe = rosters.apply(carol_device, &TeamMsg::Subscribe { repo: repo() });
    assert_eq!(subscribe, Err(Refusal::NotMember(repo())));
    assert!(!rosters.allows(carol_device, &repo()));
}

#[test]
fn roster_rules() {
    let (alice, bob) = (mesh_id(), mesh_id());
    let (alice_device, bob_device) = (mesh_id(), mesh_id());
    let mut rosters = Rosters::default();
    rosters.refresh(
        [(
            repo(),
            record(
                repo(),
                vec![
                    member("alice", alice, alice_device),
                    member("bob", bob, bob_device),
                ],
            ),
        )]
        .into(),
    );
    let roster = |members: Vec<Member>| TeamMsg::Roster {
        repo: repo(),
        members,
    };
    let laptop = mesh_id();
    let mut moved = member("bob", bob, bob_device);
    moved.devices.push(EndpointAddr::new(laptop));

    // Alice cannot change Bob's devices; Bob can.
    let by_alice = rosters.apply(alice_device, &roster(vec![moved.clone()]));
    assert!(matches!(by_alice, Ok(None)), "{by_alice:?}");
    rosters.apply(bob_device, &roster(vec![moved])).unwrap();
    assert!(rosters.allows(laptop, &repo()));

    // Any member adds one they admitted.
    let dave_device = mesh_id();
    rosters
        .apply(
            alice_device,
            &roster(vec![member("dave", mesh_id(), dave_device)]),
        )
        .unwrap();
    assert!(rosters.allows(dave_device, &repo()));

    // Leave only from the leaver.
    let leave = TeamMsg::Leave {
        repo: repo(),
        login: login("bob"),
    };
    assert_eq!(
        rosters.apply(alice_device, &leave),
        Err(Refusal::NotLeaver(login("bob")))
    );
    rosters.apply(laptop, &leave).unwrap();
    assert!(!rosters.allows(bob_device, &repo()));

    // A message about another repo is refused.
    let other: TeamRepo = "github.com/org/other".parse().unwrap();
    assert_eq!(
        rosters.apply(alice_device, &TeamMsg::Subscribe { repo: other.clone() }),
        Err(Refusal::NotMember(other))
    );
}

#[test]
fn pending_gives_way_to_the_mesh() {
    let alice_device = mesh_id();
    let team = record(repo(), vec![member("alice", mesh_id(), alice_device)]);
    let mut rosters = Rosters::default();
    rosters.refresh([(repo(), team.clone())].into());
    rosters
        .apply(
            alice_device,
            &TeamMsg::Roster {
                repo: repo(),
                members: vec![member("dave", mesh_id(), mesh_id())],
            },
        )
        .unwrap();
    assert_eq!(rosters.pending().len(), 1);

    // Unchanged in the mesh: still pending.
    rosters.refresh([(repo(), team)].into());
    assert_eq!(rosters.pending().len(), 1);

    let written = rosters.pending().remove(0);
    rosters.refresh([(repo(), written)].into());
    assert!(rosters.pending().is_empty());
}

#[test]
fn roster_args_round_trip_as_json() {
    let mut team = record(repo(), vec![member("alice", mesh_id(), mesh_id())]);
    team.invites.push(OpenInvite {
        secret_hash: [7; 32],
        expires_at: SystemTime::now(),
    });
    let args = TeamRosterArgs { record: team };

    let text = serde_json::to_string(&args).unwrap();

    assert_eq!(serde_json::from_str::<TeamRosterArgs>(&text).unwrap(), args);
}

#[tokio::test]
async fn member_invite_to_another_team_is_kept() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let (contact, device) = (mesh_id(), mesh_id());
    pair.inviter.with(|teams| {
        teams.home.contacts.insert(device, contact);
        let roster = TeamMsg::Roster {
            repo: repo(),
            members: vec![member("carol", contact, device)],
        };
        let alice = teams.rosters.get(&repo()).unwrap().members[0].devices[0].id;
        teams.rosters.apply(alice, &roster).unwrap();
    });
    let other: TeamRepo = "github.com/org/other".parse().unwrap();

    let answer = pair.inviter.answer(
        device,
        &TeamMsg::Invite {
            repo: other.clone(),
            ticket: "beepteam1x".to_owned(),
        },
    );

    assert_eq!(answer, None);
    let invites = pair.inviter.with(|teams| teams.invites.clone());
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0].repo, other);
}
