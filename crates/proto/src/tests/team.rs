use super::*;
use crate::{ProjectIssue, lenient};
use iroh::SecretKey;
use std::time::Duration;

fn id(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

fn login(s: &str) -> ForgeLogin {
    s.parse().unwrap()
}

fn repo() -> TeamRepo {
    "github.com/org/repo".parse().unwrap()
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn member() -> Member {
    Member {
        login: login("ana"),
        mesh: id(1),
        devices: vec![EndpointAddr::new(id(2))],
        joined_at: at(10),
    }
}

fn card() -> Card {
    Card {
        id: "c1".into(),
        title: "Fix it".into(),
        lane: Lane::ReadyToReview {},
        login: login("ana"),
        machine: "build-box".into(),
        issue: Some(7),
        pr: None,
        updated_at: at(20),
    }
}

#[test]
fn repo_forms_are_one() {
    let a: TeamRepo = "git@github.com:Org/Repo.git".parse().unwrap();
    let b: TeamRepo = "https://github.com/org/repo/".parse().unwrap();
    let c: TeamRepo = "ssh://git@github.com/org/repo".parse().unwrap();
    let d: TeamRepo = "GitHub.com/Org/Repo".parse().unwrap();
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(a, d);
    assert_eq!(a.to_string(), "github.com/org/repo");
}

#[test]
fn a_port_stays_with_the_host() {
    let a: TeamRepo = "https://git.example.com:8443/org/repo.git".parse().unwrap();
    assert_eq!(a.host, "git.example.com:8443");
    assert_eq!(a.hostname(), "git.example.com");
    assert_eq!(a.repo, "org/repo");

    let b: TeamRepo = "git@git.example.com:org/repo".parse().unwrap();
    assert_eq!(b.hostname(), "git.example.com");
}

#[test]
fn a_ported_host_round_trips_through_text() {
    let a: TeamRepo = "git.example.com:2222/org/repo".parse().unwrap();
    let b: TeamRepo = "https://git.example.com:2222/org/repo".parse().unwrap();
    let c: TeamRepo = "git@git.example.com:org/repo".parse().unwrap();
    assert_eq!(a, b);
    assert_eq!(a.host, "git.example.com:2222");
    assert_eq!(a.to_string().parse::<TeamRepo>().unwrap(), a);
    assert_eq!(c.host, "git.example.com");
    assert_eq!(c.repo, "org/repo");
}

#[test]
fn an_scp_remote_with_a_numeric_owner_is_not_a_port() {
    let a: TeamRepo = "git@github.com:1234/repo".parse().unwrap();
    let b: TeamRepo = "git@github.com:1234/sub/repo".parse().unwrap();
    assert_eq!(a.host, "github.com");
    assert_eq!(a.repo, "1234/repo");
    assert_eq!(b.host, "github.com");
    assert_eq!(b.repo, "1234/sub/repo");
    assert_eq!(
        "git@host/org/repo".parse::<TeamRepo>().unwrap().host,
        "host"
    );
}

#[test]
fn a_remote_without_an_org_is_refused() {
    for s in [
        "",
        "github.com",
        "github.com/repo",
        "https://github.com/",
        "git@github.com:repo",
    ] {
        assert!(s.parse::<TeamRepo>().is_err(), "{s}");
    }
}

#[test]
fn empty_login_is_refused() {
    assert!("".parse::<ForgeLogin>().is_err());
    assert!("  ".parse::<ForgeLogin>().is_err());
}

#[test]
fn team_msg_round_trips_positionally() {
    let msgs = vec![
        TeamMsg::Join {
            secret: [9; 32],
            repo: repo(),
            login: login("ana"),
            mesh: id(1),
            devices: vec![EndpointAddr::new(id(2))],
        },
        TeamMsg::Challenge { nonce: "n".into() },
        TeamMsg::Proved { nonce: "n".into() },
        TeamMsg::Roster {
            repo: repo(),
            members: vec![member()],
        },
        TeamMsg::Invite {
            repo: repo(),
            ticket: "beepteam1abc".into(),
        },
        TeamMsg::Subscribe { repo: repo() },
        TeamMsg::Board {
            repo: repo(),
            cards: vec![card()],
        },
        TeamMsg::BoardChange {
            repo: repo(),
            card: card(),
            gone: true,
        },
        TeamMsg::Ask {
            id: "a1".into(),
            repo: repo(),
            from: AgentRef {
                login: login("ana"),
                card: "c1".into(),
            },
            to: Target::Person { login: login("bo") },
            text: "how?".into(),
            reply_to: Some("a0".into()),
        },
        TeamMsg::Ask {
            id: "a2".into(),
            repo: repo(),
            from: AgentRef {
                login: login("ana"),
                card: "c1".into(),
            },
            to: Target::Agent { card: "c9".into() },
            text: "why?".into(),
            reply_to: None,
        },
        TeamMsg::Reply {
            id: "a1".into(),
            text: "so".into(),
        },
        TeamMsg::Leave {
            repo: repo(),
            login: login("ana"),
        },
        TeamMsg::Remove {
            repo: repo(),
            login: login("bo"),
        },
        TeamMsg::Refused {
            reason: "no".into(),
        },
    ];

    for msg in msgs {
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        assert_eq!(lenient::from_slice::<TeamMsg>(&bytes).unwrap(), msg);
    }
}

#[test]
fn a_team_record_reads_with_defaults() {
    let old = (repo(), vec![member()], Vec::<OpenInvite>::new());
    let bytes = rmp_serde::to_vec(&old).unwrap();
    let team: TeamRecord = lenient::from_slice(&bytes).unwrap();
    assert!(team.pickup);
    assert!(team.asks);
    assert_eq!(team.pickup_limit, None);
}

#[test]
fn old_project_issue_reads_with_no_assignees() {
    let old = (
        7u64,
        "title",
        vec!["bug"],
        "ana",
        at(5),
        2u32,
        "https://example.com/7",
        true,
    );
    let bytes = rmp_serde::to_vec(&old).unwrap();
    let issue: ProjectIssue = lenient::from_slice(&bytes).unwrap();
    assert_eq!(issue.number, 7);
    assert!(issue.linked_pr);
    assert!(issue.assignees.is_empty());
}

#[test]
fn an_empty_login_does_not_decode() {
    let bytes = rmp_serde::to_vec("").unwrap();
    assert!(lenient::from_slice::<ForgeLogin>(&bytes).is_err());

    let msg = rmp_serde::to_vec(&(10u8, ("github.com", "org/repo"), "")).unwrap();
    assert!(lenient::from_slice::<TeamMsg>(&msg).is_err());
}

#[test]
fn a_repo_decodes_normalized() {
    let bytes = rmp_serde::to_vec(&("GitHub.com", "Org/Repo.git")).unwrap();
    let decoded: TeamRepo = lenient::from_slice(&bytes).unwrap();
    assert_eq!(decoded, repo());

    let bad = rmp_serde::to_vec(&("github.com", "repo")).unwrap();
    assert!(lenient::from_slice::<TeamRepo>(&bad).is_err());
}

#[test]
fn a_merge_adds_and_updates_but_removes_only_through_removals() {
    let ana = member();
    let ben = Member {
        login: login("ben"),
        mesh: id(3),
        devices: vec![EndpointAddr::new(id(4))],
        joined_at: at(20),
    };
    let mut record = TeamRecord::new(repo(), vec![ana.clone(), ben.clone()]);
    let mut moved = ana.clone();
    moved.devices.push(EndpointAddr::new(id(5)));
    let cy = Member {
        login: login("cy"),
        mesh: id(6),
        devices: vec![EndpointAddr::new(id(7))],
        joined_at: at(30),
    };
    // Ben's login under another mesh: Ben keeps it.
    let ben_elsewhere = Member {
        mesh: id(8),
        ..ben.clone()
    };

    record.merge(TeamRecord::new(
        repo(),
        vec![moved.clone(), cy.clone(), ben_elsewhere],
    ));
    assert_eq!(record.members, [moved.clone(), ben.clone(), cy.clone()]);

    let mut gone = TeamRecord::new(repo(), Vec::new());
    gone.members.push(ben.clone());
    gone.remove(&ben.login);
    record.merge(gone);
    assert_eq!(record.members, [moved, cy]);
    assert_eq!(record.removed.len(), 1);
}
