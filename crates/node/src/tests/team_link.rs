use super::*;
use crate::team::{
    home::{Checkout, Invites, Project},
    roster::{Applied, Refusal, Rosters},
};
use beep_mesh::{JoinSecret, TeamTicket, secret_hash};
use beep_proto::{
    node::tools::{TeamInviteArgs, TeamRosterArgs},
    team::{ForgeLogin, Member, OpenInvite, TeamMsg, TeamRecord, TeamRepo},
};
use futures::future::BoxFuture;
use iroh::{
    Endpoint, EndpointAddr, EndpointId, SecretKey, address_lookup::memory::MemoryLookup,
    endpoint::presets, protocol::Router,
};
use iroh_tickets::Ticket;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime},
};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// What the fake forge says of a collaborator.
#[derive(Clone, Copy)]
enum Answer {
    Yes,
    No,
    /// Fails with local detail that must not reach the joiner.
    Fails,
    /// Never answers.
    Hangs,
}

/// A forge that answers without a network.
struct Forge {
    login: &'static str,
    collaborator: Answer,
}

const LOCAL_DETAIL: &str = "gh: token for /home/someone expired";

impl TeamForge for Forge {
    fn collaborator<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            match self.collaborator {
                Answer::Yes => Ok(true),
                Answer::No => Ok(false),
                Answer::Fails => Err(LOCAL_DETAIL.to_owned()),
                Answer::Hangs => std::future::pending().await,
            }
        })
    }

    fn login<'a>(&'a self, _repo: &'a TeamRepo) -> BoxFuture<'a, Result<ForgeLogin, String>> {
        Box::pin(async move { Ok(self.login.parse().unwrap()) })
    }

    fn open_issues<'a>(
        &'a self,
        _repo: &'a TeamRepo,
    ) -> BoxFuture<'a, Result<Vec<beep_projects::commands::OpenIssue>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn assigned_issues<'a>(
        &'a self,
        _repo: &'a TeamRepo,
        _login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<Vec<beep_projects::commands::OpenIssue>, String>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn pull_heads<'a>(&'a self, _repo: &'a TeamRepo) -> BoxFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async { Ok(Vec::new()) })
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
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// An inviter and a joiner on two endpoints, each with a checkout of one bare remote.
struct Pair {
    _root: tempfile::TempDir,
    remote: String,
    inviter: TeamLink,
    inviter_id: EndpointId,
    alice_mesh: EndpointId,
    joiner: TeamLink,
    joiner_checkout: PathBuf,
    secret: JoinSecret,
    _router: Router,
}

impl Pair {
    async fn new(expires_at: SystemTime) -> Self {
        Self::with(expires_at, Answer::Yes, "bob").await
    }

    /// An inviter whose forge gives `collaborator`, and a joiner signed in as `joiner`.
    async fn with(expires_at: SystemTime, collaborator: Answer, joiner: &'static str) -> Self {
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
        let mut team = TeamRecord::new(repo(), vec![member("alice", alice_mesh, inviter_id)]);
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
                    name: "org/repo".to_owned(),
                    checkout: Some(Checkout {
                        folder,
                        remote: place,
                    }),
                    devices: vec![EndpointAddr::new(inviter_id)],
                },
            );
        });

        let joiner = joiner_link(&inviter_addr, joiner).await;

        Self {
            _root: root,
            remote,
            inviter,
            inviter_id,
            alice_mesh,
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

    /// Sends a `Join` by hand from the joiner and reads the first answer.
    async fn send_join(&self, mesh: EndpointId, devices: Vec<EndpointAddr>) -> Option<TeamMsg> {
        let (mut tx, mut rx) = self
            .joiner
            .dial(&[EndpointAddr::new(self.inviter_id)])
            .await
            .unwrap();
        tx.send(&TeamMsg::Join {
            secret: *self.secret.as_bytes(),
            repo: repo(),
            login: login("bob"),
            mesh,
            devices,
        })
        .await
        .unwrap();

        rx.recv().await.ok().flatten()
    }

    /// Hands `msg` to the inviter as `peer`'s device would.
    async fn tell(&self, peer: EndpointId, msg: TeamMsg) -> Option<TeamMsg> {
        self.inviter.answer(peer, &msg).await
    }
}

/// A joiner signed in as `name` in a mesh of its own, which finds the inviter by
/// `inviter`'s address.
async fn joiner_link(inviter: &EndpointAddr, name: &'static str) -> TeamLink {
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
            login: name,
            collaborator: Answer::Yes,
        }),
    );
    link.with(|teams| {
        teams.home.mesh = Some(mesh_id());
        teams.home.projects.insert(
            repo(),
            Project {
                name: "org/repo".to_owned(),
                checkout: None,
                devices: vec![EndpointAddr::new(id)],
            },
        );
    });

    link
}

/// Waits until `remote` holds no proof ref: the inviter deletes it after answering.
async fn no_proof_refs(remote: &str) {
    for _ in 0..50 {
        let out = Command::new("git")
            .args(["ls-remote", remote, "refs/beep/proof/*"])
            .output()
            .unwrap();

        if out.stdout.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    panic!("a proof ref stays on the remote");
}

fn refused(result: Result<TeamRecord, TeamError>) -> String {
    match result {
        Err(TeamError::Refused(reason)) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn reason(answer: Option<TeamMsg>) -> String {
    match answer {
        Some(TeamMsg::Refused { reason }) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A `beepteam1` ticket to `repo`.
fn ticket_to(repo: TeamRepo) -> String {
    TeamTicket {
        inviter: mesh_id(),
        repo,
        secret: JoinSecret::generate(),
    }
    .encode_string()
}

fn invite(repo: TeamRepo) -> TeamMsg {
    TeamMsg::Invite {
        ticket: ticket_to(repo.clone()),
        repo,
    }
}

/// The inviter's record with `carol` as a member on `device`.
fn with_carol(pair: &Pair, carol: EndpointId, device: EndpointId) {
    pair.inviter.with(|teams| {
        let mut team = teams.rosters.get(&repo()).unwrap().clone();
        team.members.push(member("carol", carol, device));
        teams.rosters.refresh([(repo(), team)].into());
    });
}

fn invites(pair: &Pair) -> Vec<TeamInviteArgs> {
    pair.inviter
        .with(|teams| teams.invites.current(SystemTime::now()))
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
    no_proof_refs(&pair.remote).await;
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
    let pair = Pair::with(SystemTime::now() + DAY, Answer::No, "bob").await;

    let reason = refused(pair.join(pair.secret.clone()).await);

    assert!(reason.contains("not a collaborator"), "{reason}");
    assert!(pair.inviter_pending().is_empty());
    no_proof_refs(&pair.remote).await;
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

    assert!(reason(answer).contains("push access"));
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
    tx.send(&invite(repo())).await.unwrap();

    assert!(matches!(
        rx.recv().await.unwrap(),
        Some(TeamMsg::Refused { .. })
    ));
    assert!(invites(&pair).is_empty());
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
    tx.send(&invite(repo())).await.unwrap();
    tx.finish().await.unwrap();
    assert!(rx.recv::<TeamMsg>().await.unwrap().is_none());

    let invites = invites(&pair);
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0].from, contact);
}

#[tokio::test]
async fn member_invite_to_another_team_is_kept() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let (contact, device) = (mesh_id(), mesh_id());
    with_carol(&pair, contact, device);
    pair.inviter.with(|teams| {
        teams.home.contacts.insert(device, contact);
    });
    let other: TeamRepo = "github.com/org/other".parse().unwrap();

    let answer = pair.tell(device, invite(other.clone())).await;

    assert_eq!(answer, None);
    let invites = invites(&pair);
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0].repo, other);
}

#[tokio::test]
async fn contact_invites_are_checked_and_bounded() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let (contact, device) = (mesh_id(), mesh_id());
    pair.inviter.with(|teams| {
        teams.home.contacts.insert(device, contact);
    });
    let other = |n: usize| -> TeamRepo { format!("github.com/org/r{n}").parse().unwrap() };

    let unreadable = TeamMsg::Invite {
        repo: repo(),
        ticket: "beepteam1x".to_owned(),
    };
    assert!(reason(pair.tell(device, unreadable).await).contains("does not read"));
    let elsewhere = TeamMsg::Invite {
        repo: repo(),
        ticket: ticket_to(other(0)),
    };
    assert!(reason(pair.tell(device, elsewhere).await).contains("another repository"));

    // The same team twice keeps one.
    assert_eq!(pair.tell(device, invite(other(1))).await, None);
    assert_eq!(pair.tell(device, invite(other(1))).await, None);
    assert_eq!(invites(&pair).len(), 1);

    for n in 2..=8 {
        assert_eq!(pair.tell(device, invite(other(n))).await, None);
    }
    assert!(reason(pair.tell(device, invite(other(9))).await).contains("Too many"));
    assert_eq!(invites(&pair).len(), 8);
}

#[test]
fn contact_invites_expire_after_a_day() {
    let mut invites = Invites::default();
    let args = TeamInviteArgs {
        from: mesh_id(),
        repo: repo(),
        ticket: ticket_to(repo()),
    };
    let now = SystemTime::now();

    invites
        .keep(args, now - DAY - Duration::from_secs(1))
        .unwrap();

    assert!(invites.current(now).is_empty());
}

#[tokio::test]
async fn join_cannot_claim_a_member() {
    // Bob signs in as alice, who is already on the team from another mesh.
    let pair = Pair::with(SystemTime::now() + DAY, Answer::Yes, "alice").await;

    let reason = refused(pair.join(pair.secret.clone()).await);

    assert_eq!(reason, Refusal::Taken.to_string());
    assert!(pair.inviter_pending().is_empty());
}

#[tokio::test]
async fn join_from_own_mesh_or_for_other_devices_is_refused() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let joiner = EndpointAddr::new(pair.joiner.endpoint().id());

    let own = pair.send_join(pair.alice_mesh, vec![joiner.clone()]).await;
    assert_eq!(reason(own), Refusal::OwnMesh.to_string());

    let not_the_peer = pair
        .send_join(mesh_id(), vec![EndpointAddr::new(mesh_id())])
        .await;
    assert_eq!(reason(not_the_peer), Refusal::NotThePeer.to_string());

    let alice_device = pair
        .send_join(mesh_id(), vec![joiner, EndpointAddr::new(pair.inviter_id)])
        .await;
    assert_eq!(reason(alice_device), Refusal::Taken.to_string());
}

#[tokio::test]
async fn join_with_too_many_devices_is_refused() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let mut devices = vec![EndpointAddr::new(pair.joiner.endpoint().id())];
    devices.extend((0..32).map(|_| EndpointAddr::new(mesh_id())));

    let answer = pair.send_join(mesh_id(), devices).await;

    assert_eq!(reason(answer), Refusal::Devices.to_string());
}

#[tokio::test]
async fn oversized_first_message_is_dropped() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let mut devices = vec![EndpointAddr::new(pair.joiner.endpoint().id())];
    devices.extend((0..1000).map(|_| EndpointAddr::new(mesh_id())));

    let answer = pair.send_join(mesh_id(), devices).await;

    assert_eq!(answer, None, "read past the first frame's limit");
}

#[tokio::test]
async fn forge_failure_says_nothing_of_the_inviter() {
    let pair = Pair::with(SystemTime::now() + DAY, Answer::Fails, "bob").await;

    let reason = refused(pair.join(pair.secret.clone()).await);

    assert_eq!(reason, "Could not check access on the forge");
    assert!(pair.inviter_pending().is_empty());
}

#[tokio::test]
async fn git_failure_says_nothing_of_the_inviter() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    pair.inviter.with(|teams| {
        let project = teams.home.projects.get_mut(&repo()).unwrap();
        project.checkout.as_mut().unwrap().remote = "/no/such/remote.git".to_owned();
    });

    // The joiner pushes to the real remote; the inviter cannot read its own.
    let reason = refused(pair.join(pair.secret.clone()).await);

    assert_eq!(reason, "Could not check the proof");
}

#[tokio::test]
async fn slow_forge_does_not_admit() {
    let pair = Pair::with(SystemTime::now() + DAY, Answer::Hangs, "bob").await;

    let reason = refused(pair.join(pair.secret.clone()).await);

    assert!(reason.contains("took too long"), "{reason}");
    assert!(pair.inviter_pending().is_empty());
    no_proof_refs(&pair.remote).await;
}

#[tokio::test]
async fn roster_newcomer_needs_the_forge() {
    let (carol, device) = (mesh_id(), mesh_id());
    let dave = mesh_id();
    let add_dave = || TeamMsg::Roster {
        repo: repo(),
        members: vec![member("dave", mesh_id(), dave)],
    };

    let refusing = Pair::with(SystemTime::now() + DAY, Answer::No, "bob").await;
    with_carol(&refusing, carol, device);
    let answer = refusing.tell(device, add_dave()).await;
    assert!(reason(answer).contains("not a collaborator"));
    assert!(
        !refusing
            .inviter
            .with(|teams| teams.rosters.allows(dave, &repo()))
    );

    let vouching = Pair::new(SystemTime::now() + DAY).await;
    with_carol(&vouching, carol, device);
    assert_eq!(vouching.tell(device, add_dave()).await, None);
    assert!(
        vouching
            .inviter
            .with(|teams| teams.rosters.allows(dave, &repo()))
    );
}

#[test]
fn member_cannot_take_another_entry() {
    let (alice, bob, own) = (mesh_id(), mesh_id(), mesh_id());
    let (alice_device, bob_device, bob_laptop) = (mesh_id(), mesh_id(), mesh_id());
    let mut rosters = Rosters::default();
    rosters.refresh(
        [(
            repo(),
            TeamRecord::new(
                repo(),
                vec![
                    member("alice", alice, alice_device),
                    member("bob", bob, bob_device),
                ],
            ),
        )]
        .into(),
    );
    let mut from_bob = |members: Vec<Member>| {
        let msg = TeamMsg::Roster {
            repo: repo(),
            members,
        };
        rosters.apply(bob_device, Some(own), &msg)
    };

    // Alice's mesh under a new login, then Alice's login under a new mesh.
    assert_eq!(
        from_bob(vec![member("x", alice, bob_laptop)]),
        Err(Refusal::Taken)
    );
    assert_eq!(
        from_bob(vec![member("alice", mesh_id(), bob_laptop)]),
        Err(Refusal::Taken)
    );
    // Alice's device, as a newcomer or in Bob's own entry.
    assert_eq!(
        from_bob(vec![member("x", mesh_id(), alice_device)]),
        Err(Refusal::Taken)
    );
    let mut grab = member("bob", bob, bob_device);
    grab.devices.push(EndpointAddr::new(alice_device));
    assert_eq!(from_bob(vec![grab]), Err(Refusal::Taken));
    // This mesh, from outside.
    assert_eq!(
        from_bob(vec![member("x", own, bob_laptop)]),
        Err(Refusal::OwnMesh)
    );

    assert!(rosters.pending().is_empty());
    assert!(!rosters.allows(bob_laptop, &repo()));
}

#[test]
fn removed_member_stays_out() {
    let (alice, carol) = (mesh_id(), mesh_id());
    let (alice_device, carol_device) = (mesh_id(), mesh_id());
    let team = TeamRecord::new(
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
            None,
            &TeamMsg::Remove {
                repo: repo(),
                login: login("carol"),
            },
        )
        .unwrap();
    assert_eq!(removed.changed.unwrap().members.len(), 1);

    let stale = rosters.apply(
        alice_device,
        None,
        &TeamMsg::Roster {
            repo: repo(),
            members: team.members.clone(),
        },
    );
    assert_eq!(stale, Ok(Applied::default()));
    let rejoin = rosters.add(
        &repo(),
        alice_device,
        None,
        member("carol", carol, carol_device),
    );
    assert_eq!(rejoin, Err(Refusal::Removed(login("carol"))));
    let from_carol = rosters.apply(
        carol_device,
        None,
        &TeamMsg::Roster {
            repo: repo(),
            members: team.members.clone(),
        },
    );
    assert_eq!(from_carol, Err(Refusal::NotMember(repo())));
    let subscribe = rosters.apply(carol_device, None, &TeamMsg::Subscribe { repo: repo() });
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
            TeamRecord::new(
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
    let by_alice = rosters.apply(alice_device, None, &roster(vec![moved.clone()]));
    assert_eq!(by_alice, Ok(Applied::default()));
    rosters
        .apply(bob_device, None, &roster(vec![moved]))
        .unwrap();
    assert!(rosters.allows(laptop, &repo()));

    // A newcomer waits for the forge, then any member adds them.
    let dave = member("dave", mesh_id(), mesh_id());
    let applied = rosters
        .apply(alice_device, None, &roster(vec![dave.clone()]))
        .unwrap();
    assert_eq!(applied.newcomers, std::slice::from_ref(&dave));
    assert!(!rosters.allows(dave.devices[0].id, &repo()));
    rosters
        .add(&repo(), alice_device, None, dave.clone())
        .unwrap();
    assert!(rosters.allows(dave.devices[0].id, &repo()));

    // Leave only from the leaver.
    let leave = TeamMsg::Leave {
        repo: repo(),
        login: login("bob"),
    };
    assert_eq!(
        rosters.apply(alice_device, None, &leave),
        Err(Refusal::NotLeaver(login("bob")))
    );
    rosters.apply(laptop, None, &leave).unwrap();
    assert!(!rosters.allows(bob_device, &repo()));

    // A message about another repo is refused.
    let other: TeamRepo = "github.com/org/other".parse().unwrap();
    assert_eq!(
        rosters.apply(
            alice_device,
            None,
            &TeamMsg::Subscribe {
                repo: other.clone()
            }
        ),
        Err(Refusal::NotMember(other))
    );
}

#[test]
fn pending_gives_way_to_the_mesh() {
    let alice_device = mesh_id();
    let team = TeamRecord::new(repo(), vec![member("alice", mesh_id(), alice_device)]);
    let mut rosters = Rosters::default();
    rosters.refresh([(repo(), team.clone())].into());
    rosters
        .add(
            &repo(),
            alice_device,
            None,
            member("dave", mesh_id(), mesh_id()),
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
    let mut team = TeamRecord::new(repo(), vec![member("alice", mesh_id(), mesh_id())]);
    team.invites.push(OpenInvite {
        secret_hash: [7; 32],
        expires_at: SystemTime::now(),
    });
    let args = TeamRosterArgs { record: team };

    let text = serde_json::to_string(&args).unwrap();

    assert_eq!(serde_json::from_str::<TeamRosterArgs>(&text).unwrap(), args);
}

#[tokio::test]
async fn member_roster_past_the_first_frame_limit_is_taken() {
    let pair = Pair::new(SystemTime::now() + DAY).await;
    let (carol, device) = (mesh_id(), pair.joiner.endpoint().id());
    let others: Vec<Member> = (0..300)
        .map(|n| member(&format!("member{n}"), mesh_id(), mesh_id()))
        .collect();
    with_carol(&pair, carol, device);
    pair.inviter.with(|teams| {
        let mut team = teams.rosters.get(&repo()).unwrap().clone();
        team.members.extend(others.iter().cloned());
        teams.rosters.refresh([(repo(), team)].into());
    });
    let laptop = mesh_id();
    let mut moved = member("carol", carol, device);
    moved.devices.push(EndpointAddr::new(laptop));
    let mut members = others;
    members.push(moved);
    let roster = TeamMsg::Roster {
        repo: repo(),
        members,
    };
    assert!(rmp_serde::to_vec(&roster).unwrap().len() > 16 * 1024);

    let (mut tx, mut rx) = pair
        .joiner
        .dial(&[EndpointAddr::new(pair.inviter_id)])
        .await
        .unwrap();
    tx.send(&roster).await.unwrap();
    tx.finish().await.unwrap();

    assert_eq!(rx.recv::<TeamMsg>().await.unwrap(), None);
    assert!(
        pair.inviter
            .with(|teams| teams.rosters.allows(laptop, &repo()))
    );
}
