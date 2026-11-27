use super::*;
use crate::agents::start::{issue_of_prompt, issue_of_repo};
use crate::{
    agents::child_view::{ChildView, full_id},
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
fn a_start_names_an_issue_of_its_own_repository() {
    let github: TeamRepo = "github.com/org/repo".parse().unwrap();
    let forgejo: TeamRepo = "ssh://git@git.example.com:2222/org/repo".parse().unwrap();

    assert!(issue_of_repo(
        &github,
        52,
        "https://github.com/Org/Repo/issues/52"
    ));
    assert!(issue_of_repo(
        &forgejo,
        7,
        "https://git.example.com:3000/org/repo/issues/7"
    ));
    assert!(!issue_of_repo(
        &github,
        52,
        "https://github.com/org/repo/issues/53"
    ));
    assert!(!issue_of_repo(
        &github,
        52,
        "https://github.com/org/other/issues/52"
    ));
    assert!(!issue_of_repo(
        &github,
        52,
        "https://evil.example/org/repo/issues/52"
    ));
    assert!(!issue_of_repo(&github, 52, "github.com/org/repo/issues/52"));
}

#[test]
fn a_prompt_names_its_issue_by_url_or_words_but_not_by_number() {
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let of = |prompt: &str| issue_of_prompt(&repo, prompt);

    assert_eq!(
        of("Fix https://github.com/org/repo/issues/234 please"),
        Some(234)
    );
    assert_eq!(
        of("see (https://github.com/Org/Repo/issues/7#issuecomment-1)."),
        Some(7)
    );
    assert_eq!(of("Work on Issue #52: fees"), Some(52));
    assert_eq!(
        of("the url is https://github.com/org/other/issues/9, see issue #3"),
        Some(3)
    );
    assert_eq!(of("https://github.com/org/other/issues/9"), None);
    assert_eq!(of("https://github.com/org/repo/pull/9"), None);
    assert_eq!(of("fix #234 and tissue #5"), None);
    assert_eq!(of("issue #"), None);
}

#[test]
fn issues_the_person_assigned_themselves_are_never_started() {
    let mut state = pickup(&[], &[]);
    let mut plan = Plan {
        start: vec![1, 2, 3],
        queued: vec![4],
    };

    leave_self_assigned(
        &mut state,
        &mut plan,
        &[(1, Some(true)), (2, None), (3, Some(false))],
    );

    assert_eq!(plan.start, [3]);
    assert_eq!(state.seen_at_enable, set(&[1]));
    assert_eq!(due(&state, &[1, 2, 3], &set(&[]), None).start, [2, 3]);
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

/// A session off the last list, whose machine did not say it ended, may
/// still work: its slot is not handed out again.
#[test]
fn a_gateway_that_took_over_counts_the_sessions_the_one_before_started() {
    // It started nothing itself; #5's session runs on the board all the same.
    let plan = due(&pickup(&[], &[]), &[5, 6], &set(&[5]), Some(1));

    assert_eq!(plan.start, Vec::<u64>::new());
    assert_eq!(plan.queued, [6]);
}

#[test]
fn a_followed_session_not_known_to_have_ended_counts_against_the_limit() {
    let mut p = pickup(&[9], &[]);
    p.running.insert(
        9,
        Auto {
            card: "s9@laptop".to_owned(),
            branch: "issue-9".to_owned(),
        },
    );
    let plan = due(&p, &[9, 10], &set(&[]), Some(1));

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
    state.failed.insert(5, 2);

    let pickups = Pickups::new(dir.path().to_owned());
    pickups.save(&repo, &state);
    assert!(
        pickups
            .file(&repo)
            .starts_with(dir.path().join("team-pickup"))
    );

    let mut again = Pickups::new(dir.path().to_owned());
    state.on = false;
    assert_eq!(again.take(&repo), state);
}

#[test]
fn a_restarted_node_takes_the_assigned_issues_as_backlog() {
    let dir = tempfile::tempdir().unwrap();
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let mut state = pickup(&[], &[1]);
    state.on = true;
    Pickups::new(dir.path().to_owned()).save(&repo, &state);

    // While the node was down another machine started #42.
    let mut state = Pickups::new(dir.path().to_owned()).take(&repo);

    assert_eq!(plan(&mut state, &[1, 42], &[], None), Plan::default());
    assert_eq!(plan(&mut state, &[1, 42, 43], &[], None).start, [43]);
}

#[test]
fn a_restarted_node_keeps_its_queue() {
    let dir = tempfile::tempdir().unwrap();
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let mut pickups = Pickups::new(dir.path().to_owned());
    let mut state = pickup(&[], &[1]);
    state.on = true;
    let first = plan(&mut state, &[1, 5, 6], &[], Some(1));
    assert_eq!((first.start, first.queued), (vec![5], vec![6]));
    state.started.insert(5);
    pickups.save(&repo, &state);

    // The session on #5 ended while the node was down: #6 gets its room.
    let mut state = Pickups::new(dir.path().to_owned()).take(&repo);
    assert_eq!(plan(&mut state, &[1, 5, 6], &[], Some(1)).start, [6]);
    assert_eq!(state.seen_at_enable, set(&[1, 5]));

    // Turned off, the queue goes: on again, it is backlog.
    let mut state = pickup(&[], &[1]);
    state.on = true;
    plan(&mut state, &[1, 5, 6], &[], Some(0));
    pickups.save(&repo, &state);
    pickups.states.insert(repo.clone(), state);
    pickups.off(&repo);
    let mut state = pickups.take(&repo);
    assert!(state.queued.is_empty());
    assert_eq!(plan(&mut state, &[1, 5, 6], &[], Some(1)), Plan::default());
}

#[test]
fn turning_pickup_off_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let mut pickups = Pickups::new(dir.path().to_owned());
    let mut state = pickup(&[], &[1]);
    state.on = true;
    pickups.save(&repo, &state);
    pickups.states.insert(repo.clone(), state);

    pickups.off(&repo);

    let mut state = pickups.take(&repo);
    assert!(!state.on);
    assert_eq!(plan(&mut state, &[1, 2], &[], None), Plan::default());
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

    // Ended today, its card stays on the board as Done today: it ended all
    // the same.
    let mut done = cards[0].clone();
    done.lane = Lane::DoneToday {};
    assert_eq!(
        off_the_board(&state, &[done]),
        [(8, full_id(acp, "laptop"))]
    );
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

    let mut state = pickup(&[], &[]);
    state.on = true;

    assert_eq!(live(&state, &cards), set(&[42]));
    assert_eq!(plan(&mut state, &[42], &cards, None), Plan::default());
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
    pickups.keep(&BTreeSet::new(), BTreeMap::new());
    assert!(pickups.states.is_empty());

    let mut state = pickups.take(&repo);
    assert!(!state.on);
    assert_eq!(plan(&mut state, &[1, 42], &[], None), Plan::default());
    assert_eq!(state.seen_at_enable, set(&[1, 42]));
    assert_eq!(plan(&mut state, &[1, 42, 43], &[], None).start, [43]);
}

#[test]
fn issues_waiting_for_room_go_on_waiting_on_the_gateway_that_takes_over() {
    let dir = tempfile::tempdir().unwrap();
    let repo: TeamRepo = "github.com/org/repo".parse().unwrap();
    let mut pickups = Pickups::new(dir.path().to_owned());
    let mut old = pickup(&[5], &[1]);
    old.on = true;
    old.queued = set(&[6]);
    pickups.save(&repo, &old);
    pickups.states.insert(repo.clone(), old);

    // Another machine acts for the team: the queue stays on disk.
    pickups.keep(&BTreeSet::new(), BTreeMap::new());
    assert_eq!(pickups.take(&repo).queued, set(&[6]));

    // The gateway of the team hands this machine its queue.
    let handed = BTreeMap::from([(repo.clone(), vec![6, 7])]);
    pickups.keep(&BTreeSet::new(), handed);
    let mut state = pickups.take(&repo);
    assert_eq!(state.queued, set(&[6, 7]));

    // Taking the team over, it starts them, and leaves #1 as the backlog.
    assert_eq!(plan(&mut state, &[1, 6, 7], &[], None).start, [6, 7]);
    assert_eq!(state.seen_at_enable, set(&[1]));

    // A machine acting for the team keeps its own queue.
    pickups.states.insert(repo.clone(), state);
    let handed = BTreeMap::from([(repo.clone(), vec![9])]);
    pickups.keep(&BTreeSet::from([repo.clone()]), handed);
    let saved: Pickup = note::read(&pickups.file(&repo));
    assert_eq!(saved.queued, set(&[6, 7]));
}

#[test]
fn an_issue_that_cannot_start_is_given_up_after_three_tries() {
    let mut state = pickup(&[], &[]);
    state.on = true;
    let mut tries = 0;
    let mut given_up = Vec::new();

    for _ in 0..5 {
        for n in due(&state, &[7], &set(&[]), None).start {
            tries += 1;
            let refused = Err(Unstarted::Refused("no coding agent".to_owned()));
            record(&mut state, n, refused, |title| {
                given_up.push(title);
                true
            });
        }
    }

    assert_eq!(tries, 3);
    assert_eq!(given_up, ["#7 could not start: no coding agent"]);
    assert!(!state.started.contains(&7));
}

#[test]
fn an_issue_whose_card_finds_no_room_is_tried_again() {
    let mut state = pickup(&[], &[]);
    state.on = true;
    let mut cards = 0;

    for room in [false, false, false, true] {
        for n in due(&state, &[7], &set(&[]), None).start {
            let refused = Err(Unstarted::Refused("no coding agent".to_owned()));
            record(&mut state, n, refused, |_| {
                cards += 1;
                room
            });
        }
    }

    assert_eq!(cards, 2);
    assert!(due(&state, &[7], &set(&[]), None).start.is_empty());
}

#[test]
fn an_ended_session_is_followed_until_its_card_finds_room() {
    let auto = |branch: &str| Auto {
        card: format!("{branch}@laptop"),
        branch: branch.to_owned(),
    };
    let mut state = pickup(&[7, 8, 9], &[]);
    state.running.insert(7, auto("issue-7"));
    state.running.insert(8, auto("issue-8"));
    state.running.insert(9, auto("issue-9"));
    let heads = ["issue-8".to_owned()];
    let mut titles = Vec::new();

    report_ended(&mut state, &[7, 8], &heads, |_, title| {
        titles.push(title);
        false
    });
    assert_eq!(titles, ["#7 ended without a pull request"]);
    assert_eq!(state.running.keys().copied().collect::<Vec<_>>(), [7, 9]);

    report_ended(&mut state, &[7], &heads, |_, _| true);
    assert_eq!(state.running.keys().copied().collect::<Vec<_>>(), [9]);
}

#[test]
fn a_start_after_a_refused_one_is_followed() {
    let mut state = pickup(&[], &[]);
    let auto = Auto {
        card: "acp-1@laptop".to_owned(),
        branch: "issue-7".to_owned(),
    };

    let busy = Err(Unstarted::Refused("busy".to_owned()));
    record(&mut state, 7, busy, |_| {
        unreachable!("one refusal gives nothing up")
    });
    record(&mut state, 7, Ok(auto.clone()), |_| {
        unreachable!("it started")
    });
    assert_eq!(state.started, set(&[7]));
    assert_eq!(state.running.get(&7), Some(&auto));
    assert!(state.failed.is_empty());
}

#[test]
fn a_full_list_makes_room_only_in_place_of_a_pickup_card() {
    let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    let card = |id: String, secs| Card {
        id,
        title: String::new(),
        lane: Lane::NeedsYou {},
        login: "alice".parse().unwrap(),
        machine: String::new(),
        issue: None,
        pr: None,
        updated_at: at(secs),
    };
    let mut cards: Vec<Card> = (0..MAX_CARDS - 1)
        .map(|i| card(format!("ask:{i}"), i as u64))
        .collect();
    cards.push(card(format!("{PICKUP_CARD}3"), 100));

    assert!(add_card(&mut cards, card(format!("{PICKUP_CARD}9"), 200)));
    assert_eq!(cards.len(), MAX_CARDS);
    assert!(cards.iter().any(|card| card.id == "ask:0"));
    assert!(!cards.iter().any(|card| card.id == "pickup:3"));

    assert!(add_card(&mut cards, card(format!("{PICKUP_CARD}9"), 300)));
    assert_eq!(cards.len(), MAX_CARDS);
    assert_eq!(cards.iter().filter(|card| card.id == "pickup:9").count(), 1);

    let asks: Vec<Card> = (0..MAX_CARDS)
        .map(|i| card(format!("ask:{i}"), i as u64))
        .collect();
    let mut full = asks.clone();
    assert!(!add_card(&mut full, card(format!("{PICKUP_CARD}9"), 200)));
    assert_eq!(full, asks);
}

#[test]
fn a_start_that_never_reached_its_machine_is_tried_again() {
    let unsent = Some(Err(CallFailed::Unsent(anyhow::anyhow!("not connected"))));
    let sent = Some(Err(CallFailed::Sent(anyhow::anyhow!("closed"))));
    let auto = Auto {
        card: "acp-1@laptop".to_owned(),
        branch: "issue-7".to_owned(),
    };
    let done = Some(Ok(Reply::Done {
        text: serde_json::to_string(&auto).unwrap(),
        failed: false,
    }));
    let refused = Some(Ok(Reply::Done {
        text: "no checkout".to_owned(),
        failed: true,
    }));

    assert!(matches!(started_there(unsent), Err(Unstarted::Refused(_))));
    assert!(matches!(started_there(sent), Err(Unstarted::Unknown(_))));
    assert!(matches!(started_there(None), Err(Unstarted::Unknown(_))));
    assert!(matches!(started_there(refused), Err(Unstarted::Refused(e)) if e == "no checkout"));
    assert!(matches!(started_there(done), Ok(started) if started == auto));
}
