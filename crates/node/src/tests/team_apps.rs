use super::*;
use crate::agents::Parts;
use crate::tools::Caller;
use crate::{
    run::tasks::Table,
    team::board::tests::{Forge, member, repo},
};
use beep_proto::{
    SessionId,
    team::{Card, Lane, TeamRecord},
};
use iroh::{Endpoint, SecretKey, endpoint::presets};
use std::sync::Arc;

async fn link() -> TeamLink {
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("an endpoint");

    TeamLink::new(endpoint, Arc::new(Forge))
}

async fn answer(caller: &Caller, request: &Request) -> Reply {
    super::answer(
        caller.team.as_ref(),
        caller.mesh.as_ref(),
        caller.desktop_here,
        request,
    )
    .await
}

fn caller(link: &TeamLink, table: Table, desktop_here: bool) -> Caller {
    Caller {
        table,
        from: None,
        desktop_here,
        commands_off: None,
        transfers: None,
        agents: None,
        project_tools: None,
        team: Some(link.clone()),
        mesh: None,
    }
}

fn request(tool: Tool, args: serde_json::Value) -> Request {
    Request {
        session: SessionId::default(),
        tool,
        args: args.to_string(),
        approved: Default::default(),
    }
}

fn cards(forwarded: bool) -> Request {
    let repo = serde_json::to_value(repo()).expect("a repo");
    let args = serde_json::json!({ "action": "cards", "repo": repo, "forwarded": forwarded });

    request(Tool::TeamApp, args)
}

fn ask_card() -> Card {
    Card {
        id: "ask:a1".into(),
        title: "Which branch?".into(),
        lane: Lane::NeedsYou {},
        login: "bob".parse().expect("a login"),
        machine: String::new(),
        issue: None,
        pr: None,
        updated_at: SystemTime::now(),
    }
}

fn done(reply: Reply) -> (String, bool) {
    match reply {
        Reply::Done { text, failed } => (text, failed),
        other => panic!("not done: {other:?}"),
    }
}

#[tokio::test]
async fn a_forwarded_call_is_answered_on_the_gateway() {
    let link = link().await;
    let me = link.endpoint().id();
    let card = ask_card();
    link.publish(|served| {
        served.gateways.insert(repo(), me);
    });
    link.asks(|asks| asks.cards.insert(repo(), vec![card.clone()]));

    let (text, failed) = done(answer(&caller(&link, Table::Shared, false), &cards(true)).await);

    assert!(!failed, "{text}");
    let read: TeamCards = serde_json::from_str(&text).expect("cards");
    assert_eq!(read.needs_you, vec![card]);
}

#[tokio::test]
async fn cards_carry_this_meshs_own_queued_issues() {
    let link = link().await;
    let me = link.endpoint().id();
    let queued = Card {
        id: "queued:51".into(),
        lane: Lane::Queued {},
        issue: Some(51),
        ..ask_card()
    };
    let working = Card {
        id: "s1@build-box".into(),
        lane: Lane::Working {},
        ..ask_card()
    };
    link.publish(|served| {
        served.gateways.insert(repo(), me);
        served.boards.set(&repo(), vec![working, queued.clone()]);
    });

    let (text, failed) = done(answer(&caller(&link, Table::Shared, false), &cards(false)).await);

    assert!(!failed, "{text}");
    let read: TeamCards = serde_json::from_str(&text).expect("cards");
    assert_eq!(read.own_queued, vec![queued]);
}

#[tokio::test]
async fn a_followed_teammate_with_no_cards_is_online() {
    let link = link().await;
    let me = link.endpoint().id();
    let device = || SecretKey::generate().public();
    let (bob, carol) = (member("bob", device()), member("carol", device()));
    let team = TeamRecord::new(repo(), vec![bob.clone(), carol]);
    link.with(|teams| teams.rosters.refresh([(repo(), team)].into()));
    link.publish(|served| {
        served.gateways.insert(repo(), me);
        let teammates = served.teammates.entry(repo()).or_default();
        teammates.board(bob.mesh, &bob.login, Vec::new());
    });

    let (text, failed) = done(answer(&caller(&link, Table::Shared, false), &cards(false)).await);

    assert!(!failed, "{text}");
    let read: TeamCards = serde_json::from_str(&text).expect("cards");
    assert!(read.teammates.is_empty());
    assert_eq!(read.online, vec![bob.login]);
}

#[tokio::test]
async fn a_forwarded_call_is_refused_off_the_gateway() {
    let link = link().await;
    let elsewhere = SecretKey::generate().public();
    link.publish(|served| {
        served.gateways.insert(repo(), elsewhere);
    });
    link.asks(|asks| asks.cards.insert(repo(), vec![ask_card()]));

    for table in [Table::Shared, Table::Peer(elsewhere)] {
        let (text, failed) = done(answer(&caller(&link, table, false), &cards(true)).await);

        assert!(failed, "{text}");
        assert_eq!(text, "The team's gateway moved; try again");
    }
}

/// Only the node this machine elects for the team, or a desktop, starts
/// sessions on its issues here.
#[tokio::test]
async fn pickup_is_taken_only_from_the_gateway() {
    let link = link().await;
    let dir = tempfile::tempdir().expect("a folder");
    let offers = crate::link::Offers::local(Parts::default(), crate::machine::provision::Provision::new(dir.path()));
    let gateway = SecretKey::generate().public();
    link.publish(|served| {
        served.gateways.insert(repo(), gateway);
    });
    let order = serde_json::json!({
        "repo": serde_json::to_value(repo()).expect("a repo"),
        "number": 52,
        "title": "Fix it",
        "url": "https://github.com/org/repo/issues/52",
    });
    let pickup = |table| {
        let caller = Caller {
            agents: Some(offers.agents.clone()),
            ..caller(&link, table, true)
        };
        crate::tools::peer_tools::answer(request(Tool::TeamPickup, order.clone()), caller)
    };

    let (text, failed) = done(pickup(Table::Peer(SecretKey::generate().public())).await);
    assert!(failed && text.contains("does not act for the team"), "{text}");

    // Past the gate: this test machine is in no mesh.
    for table in [Table::Peer(gateway), Table::Shared] {
        let (text, failed) = done(pickup(table).await);
        assert!(failed && text == "This machine is in no mesh.", "{text}");
    }
}

#[tokio::test]
async fn rosters_and_invites_are_taken_only_on_a_desktop() {
    let link = link().await;
    let peer = Table::Peer(SecretKey::generate().public());

    for tool in [Tool::TeamRoster, Tool::TeamInvite] {
        let (text, failed) = done(
            answer(
                &caller(&link, peer, false),
                &request(tool, serde_json::json!({})),
            )
            .await,
        );

        assert!(
            failed && text == "Only a desktop records teams.",
            "{tool}: {text}"
        );
    }
}

#[tokio::test]
async fn roster_from_node_is_written_once() {
    use beep_mesh::{Key, Mesh};
    use beep_proto::{
        node::mesh_state::StateValue,
        team::{Member, TeamRecord},
    };

    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::builder(presets::Minimal)
        .bind()
        .await
        .expect("an endpoint");
    let mesh = Mesh::create(dir.path(), endpoint, "desktop")
        .await
        .expect("a mesh");
    let member = |login: &str| Member {
        login: login.parse().expect("a login"),
        mesh: SecretKey::generate().public(),
        devices: vec![iroh::EndpointAddr::new(SecretKey::generate().public())],
        joined_at: SystemTime::UNIX_EPOCH,
        admitted_by: None,
        proof: None,
    };
    // This mesh's own entry: a record without it is a team that removed it.
    let mut dana = member("dana");
    dana.mesh = mesh.id();
    let mut team = TeamRecord::new(repo(), vec![dana]);
    team.pickup = false;
    let key = Key::Team(repo());
    mesh.set(key.clone(), Some(team.to_bytes().unwrap()))
        .await
        .expect("written");
    let mut changed = TeamRecord::new(repo(), team.members.clone());
    changed.members.push(member("alice"));
    let args = serde_json::to_value(TeamRosterArgs { record: changed }).unwrap();
    let link = link().await;
    let on_desktop = Caller {
        mesh: Some(mesh.clone()),
        ..caller(&link, Table::Peer(SecretKey::generate().public()), true)
    };

    let mut changes = mesh.changes();

    for _ in 0..2 {
        let (text, failed) =
            done(answer(&on_desktop, &request(Tool::TeamRoster, args.clone())).await);
        assert!(!failed, "{text}");
    }

    assert_eq!(changes.try_recv().expect("one change"), key);
    assert!(changes.try_recv().is_err(), "written once");
    let written = TeamRecord::from_bytes(&mesh.get(&key).expect("a team")).unwrap();
    assert_eq!(written.members.len(), 2);
    assert!(!written.pickup, "the desktop's settings stay");
}

/// A link whose home names `org/repo` and whose roster holds the team, with
/// this device acting for it.
async fn acting_link(project: &str) -> TeamLink {
    let link = link().await;
    let me = link.endpoint().id();
    let team = TeamRecord::new(repo(), vec![member("bob", SecretKey::generate().public())]);
    link.edit_home(|home| {
        home.projects.insert(
            repo(),
            crate::team::home::Project {
                name: project.to_owned(),
                checkout: None,
                devices: Vec::new(),
            },
        );
    });
    link.with(|teams| teams.rosters.refresh([(repo(), team)].into()));
    link.publish(|served| {
        served.gateways.insert(repo(), me);
    });

    link
}

#[tokio::test]
async fn a_projects_cards_come_from_the_machine_that_acts_for_its_team() {
    let link = acting_link("org/repo").await;
    let card = ask_card();
    link.asks(|asks| asks.cards.insert(repo(), vec![card.clone()]));

    let read = cards_of(&link, "org/repo").expect("the gateway has cards");
    assert_eq!(read.needs_you, vec![card]);

    // Another machine, or a project that is not here, has none.
    assert!(cards_of(&link, "org/other").is_none());
    let other = SecretKey::generate().public();
    link.publish(|served| {
        served.gateways.insert(repo(), other);
    });
    assert!(cards_of(&link, "org/repo").is_none());
}

#[tokio::test]
async fn a_change_of_the_cards_rings_the_topic_of_its_project() {
    use beep_proto::node::events::{TEAM_CARDS_TOPIC, Topic};

    // A project of its own: other tests ring the shared hub too.
    let link = acting_link("org/rung").await;
    let ours = Topic::keyed(TEAM_CARDS_TOPIC, "org/rung");
    let mut changes = crate::events::system().subscribe();

    let rung = |changes: &mut tokio::sync::broadcast::Receiver<crate::events::Change>| {
        std::iter::from_fn(|| changes.try_recv().ok()).any(|change| change.topic == ours)
    };
    // The gateway elected itself above; what is rung since is what follows.
    while changes.try_recv().is_ok() {}

    link.dismiss_card(&repo(), "ask:a1");
    assert!(rung(&mut changes), "a dismissed card");

    link.publish(|served| {
        served.boards.set(&repo(), Vec::new());
    });
    assert!(rung(&mut changes), "a changed board");
}

#[tokio::test]
async fn a_team_the_device_stops_acting_for_rings_its_topic_once() {
    use beep_proto::node::events::{TEAM_CARDS_TOPIC, Topic};

    let link = acting_link("org/moved").await;
    let ours = Topic::keyed(TEAM_CARDS_TOPIC, "org/moved");
    let mut changes = crate::events::system().subscribe();
    while changes.try_recv().is_ok() {}

    let other = SecretKey::generate().public();
    link.publish(|served| {
        served.gateways.insert(repo(), other);
    });

    assert!(
        std::iter::from_fn(|| changes.try_recv().ok()).any(|change| change.topic == ours),
        "readers of the old gateway hear it has nothing now"
    );
}
