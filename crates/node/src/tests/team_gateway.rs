use super::*;
use crate::membership::team_repo_of;
use crate::{
    team::{
        board::tests::{URL, member, repo},
        home::{Home, Project, picks_up, read},
        link::Teams,
    },
    upkeep::ci::projects::pinned,
};
use beep_mesh::Key;
use beep_proto::node::{
    mesh_state::{Place, ProjectRecord, StateValue},
    tools::HostReach,
};
use beep_proto::team::{TeamMsg, TeamRecord};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};

#[tokio::test]
async fn gateway_and_ci_watcher_elect_the_same_machine() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let me = mesh.me();
    // A machine that is not in the mesh, and that would win if it counted.
    let stranger = SecretKey::generate().public();
    let record = ProjectRecord {
        url: URL.to_owned(),
        ..ProjectRecord::default()
    };
    mesh.set(
        Key::Project("org/repo".into()),
        Some(record.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    for machine in [me, stranger] {
        let place = Place {
            folder: "/srv/repo".into(),
            imported_ms: 0,
            fork: None,
        };
        mesh.set(
            Key::Project(format!("org/repo/{machine}")),
            Some(place.to_bytes().unwrap()),
        )
        .await
        .unwrap();
    }
    mesh.set(Key::Pin("org/repo".into()), Some(Vec::new()))
        .await
        .unwrap();
    let ci = Ci::default();
    let mut said = ForgeReach::default();
    let reached = HostReach {
        reached: true,
        failures: 0,
    };
    said.hosts.insert(repo().host, reached);
    let peers = [(stranger, said)].into();

    let watched = pinned(&mesh).into_iter().next().unwrap();
    let home = read(&mesh);
    let project = &home.projects[&repo()];
    let machines: BTreeSet<EndpointId> = project.devices.iter().map(|device| device.id).collect();

    assert_eq!(machines, watched.machines);
    assert_eq!(watched.target.host_name(), repo().host);
    assert_eq!(project.name, watched.name);
    // This mesh's entry names every machine with the project: both agree.
    let mine = member("alice", me);
    assert_eq!(watched.elected(me, &ci, &peers), Some(me));
    assert_eq!(elected(project, &mine, &repo(), me, &ci, &peers), Some(me));

    // When the entry names no machine with the project, as when the last
    // checkout moved before its new machine was announced, one with it still
    // acts for the team, so that it announces itself.
    let elsewhere = member("alice", SecretKey::generate().public());
    assert_eq!(
        elected(project, &elsewhere, &repo(), me, &ci, &peers),
        Some(me)
    );
}

#[test]
fn a_machine_not_on_the_entry_is_never_elected() {
    let mut ids = [
        SecretKey::generate().public(),
        SecretKey::generate().public(),
    ];
    ids.sort();
    let [first, second] = ids;
    let project = Project {
        name: "org/repo".to_owned(),
        checkout: None,
        devices: vec![EndpointAddr::new(first), EndpointAddr::new(second)],
    };
    let peers = [(second, ForgeReach::default())].into();
    let ci = Ci::default();

    // By id the first would win; only the second is on the entry.
    assert_eq!(
        elected(
            &project,
            &member("alice", first),
            &repo(),
            first,
            &ci,
            &peers
        ),
        Some(first)
    );
    assert_eq!(
        elected(
            &project,
            &member("alice", second),
            &repo(),
            first,
            &ci,
            &peers
        ),
        Some(second)
    );
}

#[test]
fn a_pending_announcement_moves_no_election() {
    let mut ids = [
        SecretKey::generate().public(),
        SecretKey::generate().public(),
    ];
    ids.sort();
    // The added machine would win by id once the written entry names it.
    let [added, old] = ids;
    let alice = member("alice", old);
    let bob = member("bob", SecretKey::generate().public());
    let written = TeamRecord::new(repo(), vec![alice.clone(), bob]);
    let machine = |me: EndpointId| {
        let mut teams = Teams::default();
        teams.rosters.refresh([(repo(), written.clone())].into());
        let mut home = Home {
            mesh: Some(alice.mesh),
            ..Home::default()
        };
        let project = Project {
            name: "org/repo".to_owned(),
            checkout: None,
            devices: vec![EndpointAddr::new(added), EndpointAddr::new(old)],
        };
        home.projects.insert(repo(), project);
        (me, teams, home)
    };
    let (mut on_old, on_added) = (machine(old), machine(added));
    // The old gateway announced the added machine; no desktop wrote it yet.
    let mut entry = alice.clone();
    entry.devices.push(EndpointAddr::new(added));
    let roster = TeamMsg::Roster {
        repo: repo(),
        members: vec![entry],
    };
    let applied = on_old
        .1
        .rosters
        .apply(old, Some(alice.mesh), &roster)
        .unwrap();
    assert!(applied.changed.is_some());

    let ci = Ci::default();
    let chosen: Vec<Option<EndpointId>> = [&on_old, &on_added]
        .into_iter()
        .map(|(me, teams, home)| {
            let other = if *me == old { added } else { old };
            let peers = [(other, ForgeReach::default())].into();
            let team = &membership::teams(home, teams)[0];
            elected(
                &home.projects[&repo()],
                &team.mine,
                &repo(),
                *me,
                &ci,
                &peers,
            )
        })
        .collect();

    // Both pick the old gateway: exactly one machine acts.
    assert_eq!(chosen, [Some(old), Some(old)]);
}

#[tokio::test]
async fn a_folder_is_in_a_team_once_its_project_has_one() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let project = ProjectRecord {
        url: URL.to_owned(),
        ..ProjectRecord::default()
    };
    mesh.set(
        Key::Project("org/repo".into()),
        Some(project.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    let place = Place {
        folder: "/srv/repo".into(),
        imported_ms: 0,
        fork: None,
    };
    mesh.set(
        Key::Project(format!("org/repo/{}", mesh.me())),
        Some(place.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    let worktree = std::path::Path::new("/srv/repo/.worktrees/52-fees");

    assert_eq!(read(&mesh).team_of(worktree), Some(repo()));
    assert!(!in_team(&mesh, worktree));

    // A team a teammate removed this mesh from is as good as left.
    let mut team = TeamRecord::new(repo(), vec![member("bob", SecretKey::generate().public())]);
    let write = |team: &TeamRecord| Some(rmp_serde::to_vec(team).unwrap());
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(!in_team(&mesh, worktree));

    let mut mine = member("alice", mesh.me());
    mine.mesh = mesh.id();
    team.members.push(mine);
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();

    assert!(in_team(&mesh, worktree));
    assert!(!in_team(&mesh, std::path::Path::new("/srv/other")));
}

#[tokio::test]
async fn of_two_projects_on_one_repository_the_last_by_name_holds_the_team() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let project = ProjectRecord {
        url: URL.to_owned(),
        ..ProjectRecord::default()
    };
    for name in ["org/first", "org/second"] {
        mesh.set(Key::Project(name.into()), Some(project.to_bytes().unwrap()))
            .await
            .unwrap();
    }
    let place = Place {
        folder: "/srv/first".into(),
        imported_ms: 0,
        fork: None,
    };
    mesh.set(
        Key::Project(format!("org/first/{}", mesh.me())),
        Some(place.to_bytes().unwrap()),
    )
    .await
    .unwrap();
    let mut team = TeamRecord::new(repo(), vec![]);
    let mut mine = member("alice", mesh.me());
    mine.mesh = mesh.id();
    team.members.push(mine);
    mesh.set(Key::Team(repo()), Some(rmp_serde::to_vec(&team).unwrap()))
        .await
        .unwrap();
    let folder = std::path::Path::new("/srv/first");

    // The later project has no checkout here and still takes the repository.
    assert!(!in_team(&mesh, folder));

    let place = Place {
        folder: "/srv/second".into(),
        ..place
    };
    mesh.set(
        Key::Project(format!("org/second/{}", mesh.me())),
        Some(place.to_bytes().unwrap()),
    )
    .await
    .unwrap();

    assert!(in_team(&mesh, std::path::Path::new("/srv/second")));
    assert!(!in_team(&mesh, folder));
}

#[tokio::test]
async fn only_a_team_this_mesh_is_on_with_pickup_on_picks_up() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let write = |team: &TeamRecord| Some(rmp_serde::to_vec(team).unwrap());
    assert!(!picks_up(&mesh, &repo()));

    let mut team = TeamRecord::new(repo(), vec![member("bob", SecretKey::generate().public())]);
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(!picks_up(&mesh, &repo()));

    let mut mine = member("alice", mesh.me());
    mine.mesh = mesh.id();
    team.members.push(mine);
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(picks_up(&mesh, &repo()));

    team.pickup = false;
    mesh.set(Key::Team(repo()), write(&team)).await.unwrap();
    assert!(!picks_up(&mesh, &repo()));
}

/// A desktop-only mesh has no other place to start an issue's session.
#[test]
fn issue_sessions_start_on_any_online_machine_with_the_checkout() {
    let (desktop, node, offline) = (
        SecretKey::generate().public(),
        SecretKey::generate().public(),
        SecretKey::generate().public(),
    );
    let project = Project {
        name: "org/repo".to_owned(),
        checkout: None,
        devices: [desktop, node, offline].map(EndpointAddr::new).to_vec(),
    };
    let elsewhere = SecretKey::generate().public();
    let online = [desktop, node, elsewhere].into();

    assert_eq!(starters(&project, &online), [desktop, node].into());
}

#[tokio::test]
async fn gh_runs_as_the_account_for_the_repositorys_owner() {
    use crate::{process::spawn_env, team::home::forge_command};
    use beep_projects::{
        model::{IssueHost, RepoName},
        team_forge::user_call,
    };
    use beep_proto::{GITHUB_ENV_VAR, SecretText, node::EnvSecret};
    use std::ffi::OsStr;

    let _env = spawn_env::TEST_LOCK.lock().await;
    let secret = |name: &str, value: &str| EnvSecret {
        name: name.to_owned(),
        value: SecretText::new(value),
    };
    spawn_env::set_vars(vec![
        secret(GITHUB_ENV_VAR, "default-token"),
        secret("BEEP_GH_TOKEN_ACME_LABS", "acme-token"),
    ]);
    let token = |repo: &str| {
        let repo: RepoName = repo.parse().unwrap();
        let command = forge_command(&user_call(&IssueHost::Github), &repo);
        let envs: Vec<_> = command.as_std().get_envs().collect();
        let gh = envs
            .iter()
            .rev()
            .find(|(name, _)| *name == OsStr::new(GITHUB_ENV_VAR));
        gh.and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
    };

    let owners = (token("acme-labs/widget"), token("someone/else"));
    spawn_env::set_vars(Vec::new());

    assert_eq!(owners.0.as_deref(), Some("acme-token"));
    assert_eq!(owners.1.as_deref(), Some("default-token"));
}

#[test]
fn a_forgejo_repository_with_a_port_is_asked_on_its_host() {
    use crate::team::home::forge_of;
    use beep_projects::model::IssueHost;

    let (host, name) = forge_of(&repo()).unwrap();

    assert_eq!(host, IssueHost::of_hostname("git.example.com").unwrap());
    assert_eq!(name.to_string(), "org/repo");
}

/// A team made on two machines already online asks them at once, though
/// the machines were asked moments before, when there was no team.
#[test]
fn a_new_team_asks_its_machines_at_once() {
    let other = SecretKey::generate().public();
    let none = (Instant::now(), BTreeSet::new());

    assert!(ask_due(None, &BTreeSet::new()));
    assert!(!ask_due(Some(&none), &BTreeSet::new()));
    assert!(ask_due(Some(&none), &[other].into()));
}

/// A machine that acts for no team still asks again by itself while a team has machines to ask:
/// the acting machine stepping down tells it nothing.
#[test]
fn machines_with_a_team_are_asked_again_when_quiet_has_passed() {
    let other = SecretKey::generate().public();
    let at = Instant::now();

    assert_eq!(ask_again_at(None), None);
    assert_eq!(ask_again_at(Some(&(at, BTreeSet::new()))), None);
    assert_eq!(ask_again_at(Some(&(at, [other].into()))), Some(at + QUIET));
}

/// A gather that waits on a machine that does not answer holds nothing up:
/// what a teammate's gateway says is taken meanwhile.
#[tokio::test]
async fn teammates_are_heard_while_the_machines_are_asked() {
    use crate::team::{
        board::tests::{device, login},
        cards::Teammates,
    };
    use beep_proto::team::Card;
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let me = mesh.me();
    let (link, _router) = device().await;
    let (heard, hearing) = mpsc::channel(8);
    let mut gateway = Gateway::new(
        mesh.clone(),
        me,
        link.clone(),
        Arc::default(),
        heard.clone(),
        dir.path().join("node"),
        Reaches::new(mesh.clone()),
    );
    let bob = member("bob", SecretKey::generate().public());
    let follow = AbortOnDropHandle::new(tokio::spawn(std::future::pending()));
    gateway
        .follows
        .insert((repo(), bob.mesh), (bob.clone(), follow));
    // The machines were just asked and one does not answer.
    gateway.asked = Some((Instant::now(), BTreeSet::new()));
    gateway.asking.spawn(std::future::pending());
    let card = Card {
        id: "s1@build-box".to_owned(),
        title: "Fix the fee model".to_owned(),
        lane: Lane::Working {},
        login: login("bob"),
        machine: "Build box".to_owned(),
        issue: None,
        pr: None,
        updated_at: SystemTime::now(),
    };
    // The first tick is due at once, once the timer has run.
    tokio::time::pause();
    let mut running = Box::pin(gateway.drive(hearing));
    assert!(futures::poll!(&mut running).is_pending());
    tokio::time::advance(Duration::from_millis(1)).await;
    assert!(futures::poll!(&mut running).is_pending());
    let board = Followed {
        repo: repo(),
        mesh: bob.mesh,
        heard: Heard::Board(vec![card.clone()]),
    };
    heard.send(board).await.unwrap();
    assert!(futures::poll!(&mut running).is_pending());

    let cards = link.served(|served| served.teammates.get(&repo()).map(Teammates::cards));
    assert_eq!(cards, Some(vec![card]));
}

/// The gateway acts on the mesh's new teams and projects when the home task
/// publishes them, not at its next tick.
#[tokio::test]
async fn the_gateway_elects_when_the_home_changes() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let me = mesh.me();
    let (link, _router) = crate::team::board::tests::device().await;
    let (heard, hearing) = mpsc::channel(8);
    let mut gateway = Gateway::new(
        mesh.clone(),
        me,
        link.clone(),
        Arc::default(),
        heard,
        dir.path().join("node"),
        Reaches::new(mesh.clone()),
    );
    gateway.answered = true;
    let alice = member("alice", me);
    let team = TeamRecord::new(repo(), vec![alice.clone()]);
    link.with(|teams| teams.rosters.refresh([(repo(), team)].into()));
    tokio::time::pause();
    let mut running = Box::pin(gateway.drive(hearing));
    assert!(futures::poll!(&mut running).is_pending());
    assert!(link.served(|served| served.gateways.is_empty()));

    link.edit_home(|home| {
        home.mesh = Some(alice.mesh);
        let project = Project {
            name: "org/repo".to_owned(),
            checkout: None,
            devices: vec![EndpointAddr::new(me)],
        };
        home.projects.insert(repo(), project);
    });
    assert!(futures::poll!(&mut running).is_pending());

    assert_eq!(
        link.served(|served| served.gateways.get(&repo()).copied()),
        Some(me)
    );
}

/// A roster edit elects at once, and a machine that acts for no team keeps
/// no timer.
#[tokio::test]
async fn the_gateway_elects_when_a_roster_is_edited() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let me = mesh.me();
    let (link, _router) = crate::team::board::tests::device().await;
    let (heard, hearing) = mpsc::channel(8);
    let mut gateway = Gateway::new(
        mesh.clone(),
        me,
        link.clone(),
        Arc::default(),
        heard,
        dir.path().join("node"),
        Reaches::new(mesh.clone()),
    );
    gateway.answered = true;
    let alice = member("alice", me);
    link.edit_home(|home| {
        home.mesh = Some(alice.mesh);
        let project = Project {
            name: "org/repo".to_owned(),
            checkout: None,
            devices: vec![EndpointAddr::new(me)],
        };
        home.projects.insert(repo(), project);
    });
    tokio::time::pause();
    let mut running = Box::pin(gateway.drive(hearing));
    assert!(futures::poll!(&mut running).is_pending());
    assert!(
        link.served(|served| served.gateways.is_empty()),
        "no team yet"
    );

    // No timer runs for a machine that holds no team: time passing elects nothing.
    tokio::time::advance(Duration::from_secs(600)).await;
    assert!(futures::poll!(&mut running).is_pending());
    assert!(link.served(|served| served.gateways.is_empty()));

    let team = TeamRecord::new(repo(), vec![alice.clone()]);
    link.with(|teams| teams.rosters.refresh([(repo(), team)].into()));
    link.roster_edited();
    assert!(futures::poll!(&mut running).is_pending());

    assert_eq!(
        link.served(|served| served.gateways.get(&repo()).copied()),
        Some(me)
    );
}

#[tokio::test]
async fn a_busy_desktop_write_does_not_hold_up_the_home() {
    use crate::team::home::ForgeCli;
    use crate::team::link::home_io::follow;
    use beep_mesh::{DeviceKind, TeamWrites};
    use std::sync::Arc;
    use tokio::time::timeout;
    use tokio_util::task::AbortOnDropHandle;

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
    assert!(
        mesh.devices()
            .iter()
            .any(|status| status.device.id == me && status.device.kind == DeviceKind::Desktop),
        "the first device of a mesh is a desktop"
    );
    let link = TeamLink::new(endpoint, Arc::new(ForgeCli));
    let mut home = link.home_changes();

    // Keeping the contacts waits on this lock for as long as it is held.
    let writing = TeamWrites::lock().await;
    let _follow = AbortOnDropHandle::new(tokio::spawn(follow(mesh.clone(), link.clone())));
    let record = ProjectRecord {
        url: URL.to_owned(),
        ..ProjectRecord::default()
    };
    mesh.set(
        Key::Project("org/repo".into()),
        Some(record.to_bytes().unwrap()),
    )
    .await
    .unwrap();

    let seen = timeout(
        Duration::from_secs(20),
        beep_proto::watched::wait_for(&mut home, |home| home.projects.contains_key(&repo())),
    )
    .await;
    drop(writing);

    assert!(
        seen.is_ok(),
        "the project reached the home while a write was held"
    );
}

fn in_team(mesh: &Mesh, folder: &std::path::Path) -> bool {
    team_repo_of(mesh, folder).is_some()
}

/// A machine's pushed session list makes the cards again after `SETTLE`, not
/// at the next round, and a machine that refuses to push brings the round
/// back to `ROUND`.
#[tokio::test(start_paused = true)]
async fn a_pushed_session_change_makes_the_cards_again_without_a_round() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal).bind().await.unwrap();
    let mesh = Mesh::create(&dir.path().join("mesh"), endpoint, "build-box")
        .await
        .unwrap();
    let me = mesh.me();
    let (link, _router) = crate::team::board::tests::device().await;
    let (heard, _hearing) = mpsc::channel(8);
    let mut gateway = Gateway::new(
        mesh.clone(),
        me,
        link,
        Arc::default(),
        heard,
        dir.path().join("node"),
        Reaches::new(mesh.clone()),
    );
    let peer = SecretKey::generate().public();
    gateway.held.insert(repo());
    gateway.sightings.insert(
        peer,
        AbortOnDropHandle::new(tokio::spawn(std::future::pending())),
    );
    // The last round ended long ago and the next is due in a minute.
    gateway.round_due = Some(Instant::now() + BUSY);

    // A machine not heard yet is asked, so its sessions count meanwhile.
    assert!(gateway.unheard().contains(&peer));

    gateway.sighted(Sighted {
        peer,
        sight: Sight::Rows(Vec::new()),
    });

    assert!(gateway.unheard().is_empty());
    assert_eq!(gateway.round_due, Some(Instant::now() + SETTLE));
    assert!(gateway.rows.contains_key(&peer));

    // The machine that refuses is asked for its sessions, on a round.
    gateway.sighted(Sighted {
        peer,
        sight: Sight::Refused,
    });
    gateway.served(Ok((Cards::new(dir.path().join("node")), Vec::new())));

    assert!(gateway.refused.contains(&peer));
    assert!(gateway.rows.is_empty());
    assert_eq!(gateway.round_due, Some(Instant::now() + ROUND));
}
