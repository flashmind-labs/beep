use super::*;
use crate::{
    agents::mcp::children::{ChildView, full_id},
    team::cards::{Facts, IssueTitles, card_of},
};
use beep_proto::node::agent::SessionInfo;

fn pickup(started: &[u64], seen: &[u64]) -> Pickup {
    Pickup {
        started: started.iter().copied().collect(),
        seen_at_enable: seen.iter().copied().collect(),
        ..Pickup::default()
    }
}

fn set(issues: &[u64]) -> BTreeSet<u64> {
    issues.iter().copied().collect()
}

#[test]
fn backlog_is_not_started() {
    let plan = due(&pickup(&[], &[1, 2]), &[1, 2], &set(&[]), None);

    assert_eq!(plan, Plan::default());
}

#[test]
fn new_assignment_starts_once() {
    let mut p = pickup(&[], &[1]);
    let plan = due(&p, &[1, 3], &set(&[]), None);
    assert_eq!(plan.start, [3]);
    p.started.extend(plan.start);

    assert_eq!(due(&p, &[1, 3], &set(&[]), None), Plan::default());
}

#[test]
fn live_manual_session_is_not_doubled() {
    let plan = due(&pickup(&[], &[]), &[4], &set(&[4]), None);

    assert_eq!(plan, Plan::default());
}

#[test]
fn limit_queues_the_rest() {
    let plan = due(&pickup(&[], &[]), &[5, 6, 7], &set(&[]), Some(1));

    assert_eq!(plan.start, [5]);
    assert_eq!(plan.queued, [6, 7]);
}

#[test]
fn live_automatic_sessions_count_against_the_limit() {
    let plan = due(&pickup(&[9], &[]), &[9, 10], &set(&[9]), Some(1));

    assert_eq!(plan.start, Vec::<u64>::new());
    assert_eq!(plan.queued, [10]);
}

#[test]
fn no_limit_starts_all() {
    let plan = due(&pickup(&[], &[]), &[5, 6, 7], &set(&[]), None);

    assert_eq!(plan.start, [5, 6, 7]);
    assert!(plan.queued.is_empty());
}

/// The branches `issueBranch` in `apps/desktop/src/project/threads.ts` makes.
#[test]
fn branches_match_work_on_it() {
    let cases = [
        (
            52,
            "App crashes when the phone sleeps",
            "issue-52-app-crashes-when-the-phone",
        ),
        (
            7,
            "Café crème: ÜBER naïve \u{201c}quotes\u{201d} \u{212a}elvin İstanbul",
            "issue-7-caf-cr-me-ber-na-ve-quotes",
        ),
        (
            1069,
            "Supercalifragilisticexpialidocious-thing",
            "issue-1069-supercalifragilisticexpialidoc",
        ),
        (
            3,
            "  --Fix   the  FEE model!!  ",
            "issue-3-fix-the-fee-model",
        ),
        (9, "!!!", "issue-9"),
        (12, "", "issue-12"),
    ];

    for (number, title, branch) in cases {
        assert_eq!(issue_branch(number, title), branch, "{title}");
    }
}

#[test]
fn the_prompt_is_work_on_its() {
    assert_eq!(
        issue_prompt(52, "Crash", "https://forge.example/o/r/issues/52"),
        "Work on issue #52: Crash (https://forge.example/o/r/issues/52). Read the issue and its comments first, then propose and implement a fix."
    );
}

#[test]
fn state_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let mut state = pickup(&[3, 4], &[1, 2]);
    state.on = true;
    state.running.insert(
        4,
        Auto {
            card: "a1@build-box".to_owned(),
            branch: "issue-4-fix".to_owned(),
        },
    );

    let pickups = Pickups::new(dir.path().to_owned());
    pickups.save(&repo, &state);
    assert!(
        pickups
            .file(&repo)
            .starts_with(dir.path().join("team-pickup"))
    );

    let mut again = Pickups::new(dir.path().to_owned());
    assert_eq!(again.take(&repo), state);
}

#[test]
fn turning_pickup_off_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let mut pickups = Pickups::new(dir.path().to_owned());
    let mut state = pickup(&[], &[1]);
    state.on = true;
    pickups.save(&repo, &state);

    pickups.off(&repo);

    let mut again = Pickups::new(dir.path().to_owned());
    assert!(!again.take(&repo).on);
}

#[test]
fn live_issues_come_from_branches_and_automatic_sessions() {
    let card = |id: &str, lane: Lane, issue: Option<u64>| Card {
        id: id.to_owned(),
        title: String::new(),
        lane,
        login: "alice".parse().unwrap(),
        machine: String::new(),
        issue,
        pr: None,
        updated_at: SystemTime::now(),
    };
    let mut state = pickup(&[8], &[]);
    state.running.insert(
        8,
        Auto {
            card: "s8@laptop".to_owned(),
            branch: "issue-8".to_owned(),
        },
    );
    let cards = [
        card("s1@here", Lane::Working {}, Some(1)),
        card("s2@here", Lane::DoneToday {}, Some(2)),
        card("s8@laptop", Lane::ReadyToReview {}, None),
    ];

    assert_eq!(live(&state, &cards), set(&[1, 8]));
}

/// The card this mesh's listing makes of the session its agent named `acp`
/// on `machine`, titled `title`, in the team's project.
fn listed_card(acp: &str, machine: &str, title: &str) -> Card {
    let id = SessionId::from(acp);
    let info = SessionInfo {
        id: id.clone(),
        title: Some(title.to_owned()),
        ..SessionInfo::default()
    };
    let mut view = ChildView::of(&info, &id).named(machine);
    view.project = Some("org/repo".to_owned());
    let facts = Facts {
        project: "org/repo",
        login: &"alice".parse().unwrap(),
        linked: &IssueTitles::default(),
        now: SystemTime::now(),
    };

    card_of(&view, &facts, None).unwrap()
}

#[test]
fn a_followed_session_is_found_by_the_id_its_card_has() {
    let acp = "019a7c2e-acp";
    let mut state = pickup(&[8], &[]);
    state.running.insert(
        8,
        Auto {
            card: full_id(acp, "laptop"),
            branch: "issue-8-fix".to_owned(),
        },
    );
    let cards = [listed_card(acp, "laptop", "Fix it")];

    assert_eq!(live(&state, &cards), set(&[8]));
    assert!(off_the_board(&state, &cards).is_empty());
    assert_eq!(off_the_board(&state, &[]), [(8, full_id(acp, "laptop"))]);
}

#[test]
fn an_issue_assigned_twice_starts_once() {
    let plan = due(&pickup(&[], &[]), &[6, 5, 6], &set(&[]), None);

    assert_eq!(plan.start, [5, 6]);
}

#[test]
fn a_session_titled_for_an_issue_is_live() {
    let cards = [
        listed_card("acp-1", "laptop", "#42 Fix the crash"),
        listed_card("acp-2", "laptop", "#7"),
        listed_card("acp-3", "laptop", "#x9 not an issue"),
    ];

    assert_eq!(live(&pickup(&[], &[]), &cards), set(&[42]));
    assert_eq!(
        plan(&mut pickup(&[], &[]), &[42], &cards, None),
        Plan::default()
    );
}

#[test]
fn a_team_acted_for_again_leaves_its_assigned_issues_alone() {
    let dir = tempfile::tempdir().unwrap();
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let mut pickups = Pickups::new(dir.path().to_owned());
    let mut state = pickup(&[], &[1]);
    state.on = true;
    pickups.save(&repo, &state);
    pickups.states.insert(repo.clone(), state);

    // Another machine acts for the team and starts #42.
    pickups.keep(&BTreeSet::new());
    assert!(pickups.states.is_empty());

    let mut state = pickups.take(&repo);
    assert!(!state.on);
    assert_eq!(plan(&mut state, &[1, 42], &[], None), Plan::default());
    assert_eq!(state.seen_at_enable, set(&[1, 42]));
    assert_eq!(plan(&mut state, &[1, 42, 43], &[], None).start, [43]);
}
