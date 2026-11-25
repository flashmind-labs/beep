use super::*;
use iroh::SecretKey;
use std::time::Duration;

const PROJECT: &str = "org/repo";
const TRANSCRIPT: &str = "the agent's private words";

fn view(state: &str, project: &str) -> ChildView {
    ChildView {
        id: "s1@build-box".to_owned(),
        machine: "Build box".to_owned(),
        state: state.to_owned(),
        title: Some("Fix the fee model".to_owned()),
        last_message: TRANSCRIPT.to_owned(),
        question: Some(TRANSCRIPT.to_owned()),
        project: Some(project.to_owned()),
        ..ChildView::default()
    }
}

fn login() -> ForgeLogin {
    "alice".parse().unwrap()
}

fn card(view: &ChildView, branch: Option<&str>, linked: &IssueTitles) -> Option<Card> {
    let login = login();
    let facts = Facts {
        project: PROJECT,
        login: &login,
        linked,
        now: SystemTime::now(),
    };

    card_of(view, &facts, branch)
}

#[test]
fn card_skips_other_projects() {
    let none = IssueTitles::default();

    assert!(card(&view("working", "org/other"), None, &none).is_none());
    assert!(card(&view("working", PROJECT), None, &none).is_some());

    let mut unregistered = view("working", PROJECT);
    unregistered.project = None;
    assert!(card(&unregistered, None, &none).is_none());
}

#[test]
fn card_takes_the_issue_its_prompt_named_when_its_branch_names_none() {
    let linked: IssueTitles = [(234, "Trading interface".to_owned())].into_iter().collect();
    let mut session = view("working", PROJECT);
    session.issue = Some(234);

    let by_prompt = card(&session, Some("manual-trading-interface"), &linked).unwrap();
    assert_eq!(by_prompt.issue, Some(234));
    assert_eq!(by_prompt.title, "Trading interface");

    let by_branch = card(&session, Some("52-fee-model"), &linked).unwrap();
    assert_eq!(by_branch.issue, Some(52));
}

#[test]
fn card_title_prefers_issue() {
    let linked: IssueTitles = [(52, "Fees are wrong".to_owned())].into_iter().collect();
    let session = view("working", PROJECT);

    let on_issue = card(&session, Some("52-fee-model"), &linked).unwrap();
    assert_eq!(on_issue.title, "Fees are wrong");
    assert_eq!(on_issue.issue, Some(52));

    // An issue that is not open keeps its number and the session's title.
    let closed = card(&session, Some("53-other"), &linked).unwrap();
    assert_eq!(closed.title, "Fix the fee model");
    assert_eq!(closed.issue, Some(53));

    let plain = card(&session, Some("main"), &linked).unwrap();
    assert_eq!(plain.title, "Fix the fee model");
    assert_eq!(plain.issue, None);
    assert_eq!(plain.machine, "Build box");
    assert_eq!(plain.login, login());
    assert_eq!(plain.id, "s1@build-box");
}

#[test]
fn issue_of_branch_reads_leading_number() {
    assert_eq!(issue_of_branch("52-fee-model"), Some(52));
    assert_eq!(issue_of_branch("52"), Some(52));
    assert_eq!(issue_of_branch("alice/52-fee-model"), Some(52));
    assert_eq!(issue_of_branch("issue-52-fee-model"), Some(52));
    assert_eq!(issue_of_branch("issue-52"), Some(52));
    assert_eq!(issue_of_branch("issue-fee"), None);
    assert_eq!(issue_of_branch("main"), None);
    assert_eq!(issue_of_branch("v2-release"), None);
    assert_eq!(issue_of_branch("52fee"), None);
    assert_eq!(issue_of_branch(""), None);
}

#[test]
fn lanes_follow_state() {
    let none = IssueTitles::default();
    let lane = |state: &str| card(&view(state, PROJECT), None, &none).map(|card| card.lane);

    assert_eq!(lane("waiting_on_user"), Some(Lane::NeedsYou {}));
    assert_eq!(lane("working"), Some(Lane::Working {}));
    assert_eq!(lane("idle"), Some(Lane::ReadyToReview {}));
    assert_eq!(lane("ended"), Some(Lane::DoneToday {}));
    assert_eq!(lane("unreachable"), None);
}

#[test]
fn card_carries_no_transcript() {
    let card = card(
        &view("waiting_on_user", PROJECT),
        None,
        &IssueTitles::default(),
    )
    .unwrap();
    let bytes = rmp_serde::to_vec(&card).unwrap();

    assert!(
        !bytes
            .windows(TRANSCRIPT.len())
            .any(|window| window == TRANSCRIPT.as_bytes())
    );
}

#[test]
fn settled_cards_keep_their_time_and_drop_old_done_ones() {
    let now = SystemTime::now();
    let none = IssueTitles::default();
    let login = login();
    let at = |now| Facts {
        project: PROJECT,
        login: &login,
        linked: &none,
        now,
    };
    let earlier = now - Duration::from_secs(60);
    let old = vec![card_of(&view("working", PROJECT), &at(earlier), None).unwrap()];

    // Unchanged: it keeps the time it last changed.
    let same = vec![card_of(&view("working", PROJECT), &at(now), None).unwrap()];
    assert_eq!(settle(&old, same, now)[0].updated_at, earlier);

    // Moved lanes: it takes the new time.
    let moved = vec![card_of(&view("idle", PROJECT), &at(now), None).unwrap()];
    assert_eq!(settle(&old, moved, now)[0].updated_at, now);

    // Done two days ago: off the board.
    let days_ago = now - Duration::from_secs(2 * 24 * 60 * 60);
    let done = card_of(&view("ended", PROJECT), &at(days_ago), None).unwrap();
    assert!(settle(std::slice::from_ref(&done), vec![done.clone()], now).is_empty());

    // Sessions are listed as Done today from the start of this day here.
    let since = SystemTime::UNIX_EPOCH + Duration::from_secs(midnight(now));
    assert!(since <= now && now.duration_since(since).unwrap() < Duration::from_secs(25 * 60 * 60));
}

#[test]
fn changes_name_what_moved_and_what_went() {
    let none = IssueTitles::default();
    let a = card(&view("working", PROJECT), None, &none).unwrap();
    let mut b = a.clone();
    b.id = "s2@build-box".to_owned();
    let mut a_done = a.clone();
    a_done.lane = Lane::ReadyToReview {};

    let changed = changes(&[a.clone(), b.clone()], &[a_done.clone()]);
    assert_eq!(changed, vec![(a_done.clone(), false), (b, true)]);
    assert!(changes(&[a_done.clone()], &[a_done]).is_empty());
}

#[test]
fn teammates_keep_only_their_own_cards() {
    let none = IssueTitles::default();
    let bob: ForgeLogin = "bob".parse().unwrap();
    let mesh = SecretKey::generate().public();
    let mut theirs = card(&view("working", PROJECT), None, &none).unwrap();
    theirs.login = bob.clone();
    // A card that claims to be someone else's is not taken.
    let claimed = card(&view("working", PROJECT), None, &none).unwrap();
    let mut teammates = Teammates::default();

    teammates.board(mesh, &bob, vec![theirs.clone(), claimed.clone()]);
    assert_eq!(teammates.cards(), vec![theirs.clone()]);

    teammates.change(mesh, &bob, claimed, false);
    let mut moved = theirs.clone();
    moved.lane = Lane::NeedsYou {};
    teammates.change(mesh, &bob, moved.clone(), false);
    assert_eq!(teammates.cards(), vec![moved.clone()]);

    teammates.change(mesh, &bob, moved, true);
    assert!(teammates.cards().is_empty());

    let many = (0..MAX_CARDS + 5).map(|n| {
        let mut card = theirs.clone();
        card.id = format!("s{n}@build-box");
        card
    });
    teammates.board(mesh, &bob, many.collect());
    assert_eq!(teammates.cards().len(), MAX_CARDS);
    teammates.gone(mesh);
    assert!(teammates.cards().is_empty());
}

#[test]
fn teammates_titles_are_capped() {
    let bob: ForgeLogin = "bob".parse().unwrap();
    let mesh = SecretKey::generate().public();
    let mut long = card(&view("working", PROJECT), None, &IssueTitles::default()).unwrap();
    long.login = bob.clone();
    long.title = "é".repeat(TITLE * 10);
    let mut teammates = Teammates::default();

    teammates.board(mesh, &bob, vec![long.clone()]);
    assert_eq!(teammates.cards()[0].title, "é".repeat(TITLE));

    long.id = "s2@build-box".to_owned();
    teammates.change(mesh, &bob, long, false);
    assert!(
        teammates
            .cards()
            .iter()
            .all(|card| card.title.chars().count() == TITLE)
    );
}
