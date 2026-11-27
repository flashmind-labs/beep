//! The team views keep the keys the TypeScript interfaces in
//! `src/stores/project/team-state.ts`, `src/screens/project/team-view.tsx` and `teamjoin.tsx` read.

use super::*;
use beep_proto::team::{Lane, Member, TeamRepo};
use iroh::{EndpointAddr, SecretKey};
use std::time::SystemTime;

fn keys(value: &serde_json::Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

fn json(value: impl Serialize) -> serde_json::Value {
    serde_json::to_value(value).expect("serializes")
}

fn repo() -> TeamRepo {
    "github.com/org/web".parse().expect("a repo")
}

fn card() -> Card {
    Card {
        id: "ask:a1".to_owned(),
        title: "Who owns the fee model?".to_owned(),
        lane: Lane::NeedsYou {},
        login: "kim".parse().expect("a login"),
        machine: String::new(),
        issue: None,
        pr: None,
        updated_at: SystemTime::UNIX_EPOCH,
    }
}

/// TeamRecord and TeamMember.
#[test]
fn a_team_view_has_the_records_keys() {
    let own = SecretKey::generate().public();
    let member = Member {
        login: "dana".parse().expect("a login"),
        mesh: own,
        devices: vec![EndpointAddr::new(own)],
        joined_at: SystemTime::UNIX_EPOCH,
        admitted_by: None,
        proof: None,
    };
    let view = json(TeamView::of(TeamRecord::new(repo(), vec![member]), own));

    assert_eq!(
        keys(&view),
        [
            "asks",
            "me",
            "members",
            "pickup",
            "pickup_limit",
            "removed",
            "repo"
        ]
    );
    assert_eq!(keys(&view["members"][0]), ["joined_at", "login", "mesh"]);
}

/// TeamCards and TeamCard.
#[test]
fn team_cards_have_the_cards_keys() {
    let cards = TeamCardsView {
        needs_you: vec![CardView::from(card())],
        teammates: Vec::new(),
        own_queued: Vec::new(),
        online: vec!["kim".to_owned()],
    };
    let view = json(cards);

    assert_eq!(
        keys(&view),
        ["needs_you", "online", "own_queued", "teammates"]
    );
    assert_eq!(
        keys(&view["needs_you"][0]),
        [
            "id",
            "issue",
            "lane",
            "login",
            "machine",
            "pr",
            "title",
            "updated_at"
        ]
    );
    assert_eq!(view["needs_you"][0]["lane"], "needs_you");
}

/// Contact.
#[test]
fn a_contact_view_has_the_contacts_keys() {
    let contact = ContactRecord {
        mesh: SecretKey::generate().public(),
        logins: vec![(repo(), "kim".parse().expect("a login"))],
        devices: Vec::new(),
    };
    let view = json(ContactView::from(contact));

    assert_eq!(keys(&view), ["logins", "mesh"]);
    assert_eq!(keys(&view["logins"][0]), ["login", "repo"]);
}

/// TeamInvite and Ticket.
#[test]
fn invite_and_ticket_views_have_their_keys() {
    let invite = json(InviteView {
        from: "mesh-b".to_owned(),
        login: None,
        repo: repo().to_string(),
        ticket: "beepteam1abc".to_owned(),
    });
    let ticket = json(TicketView {
        host: "github.com".to_owned(),
        repo: "org/web".to_owned(),
        project: None,
    });

    assert_eq!(keys(&invite), ["from", "login", "repo", "ticket"]);
    assert_eq!(keys(&ticket), ["host", "project", "repo"]);
}

#[test]
fn prepared_project_has_its_keys_and_matches_online_found_checkout() {
    let prepared = json(PreparedProject {
        project: "org/web".to_owned(),
        machine_name: Some("build-box".to_owned()),
        folder: Some("/srv/org/web".to_owned()),
        added: true,
        cloned: false,
    });
    assert_eq!(
        keys(&prepared),
        ["added", "cloned", "folder", "machine_name", "project"]
    );

    let found = [FoundProject {
        project: "org/web".to_owned(),
        remote: "git@github.com:Org/Web.git".to_owned(),
        places: vec![
            FoundPlace {
                machine_id: "offline-box".to_owned(),
                machine_name: "offline-box".to_owned(),
                folder: "/srv/offline/web".to_owned(),
            },
            FoundPlace {
                machine_id: "build-box".to_owned(),
                machine_name: "build-box".to_owned(),
                folder: "/srv/org/web".to_owned(),
            },
        ],
        agents: Vec::new(),
        last_activity: SystemTime::UNIX_EPOCH,
    }];
    let online = ["build-box".to_owned()];

    let matched = find_matching_place(&found, &repo(), &online, None).expect("matches online");
    assert_eq!(matched.0, "git@github.com:Org/Web.git");
    assert_eq!(matched.1.machine_id, "build-box");
    assert_eq!(matched.1.folder, "/srv/org/web");

    assert!(find_matching_place(&found, &repo(), &online, Some("other-box")).is_none());
    assert!(find_matching_place(&found, &repo(), &[], None).is_none());
}
