use super::*;

#[test]
fn parses_the_list() {
    let good = "Here you go:\n\
- Plan the weekend | Help me plan a relaxed weekend.\n\
- **Gym check-in** | Log today's workout.\n\n\
* Dinner idea | Suggest a quick dinner.\n\
1. Inbox | Summarize my unread email.\n\
- Extra | One more than needed.";
    let now = singapore("2026-10-04T22:00:00+08:00");
    let parsed = parse(good, &now).unwrap();
    let titles: Vec<_> = parsed
        .suggestions
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Plan the weekend", "Gym check-in", "Dinner idea", "Inbox"]
    );
    assert_eq!(parsed.stale_at, None);

    let long = format!("- {} | p\n- a | b\n- No prompt", "x".repeat(29));
    assert!(parse(&long, &now).is_err());
    assert!(parse("Sure! Here are some ideas", &now).is_err());
}

fn singapore(time: &str) -> LocalTime {
    LocalTime {
        time: time.parse().expect("valid time"),
        timezone: Some(Tz::Asia__Singapore),
    }
}

#[test]
fn list_items_skip_an_intro() {
    assert_eq!(
        list_items("Sure:\n\n- Why?\n1) How?\n- 3 ideas"),
        ["Why?", "How?", "3 ideas"]
    );
    assert_eq!(list_items("Why?\nHow?"), ["Why?", "How?"]);
    assert_eq!(list_items("```\n- Why?\n```"), ["Why?"]);
    assert_eq!(list_items("10.30\n11.00"), ["10.30", "11.00"]);
    assert_eq!(list_items("1. Walk\n2.5 hours"), ["Walk"]);
    assert_eq!(
        list_items("**Follow-ups:**\n* Why?\n* How?"),
        ["Why?", "How?"]
    );
}

#[test]
fn starters_go_stale_at_the_first_until() {
    let now = singapore("2026-10-04T22:00:00+08:00");
    let answer = "- a | b | until 2026-10-05T14:00\n\
- a | b | 2026-10-05T09:30\n\
- a | b\n\
- a | b | until soon";
    let stale_at = parse(answer, &now).unwrap().stale_at;
    assert_eq!(stale_at, Some("2026-10-05T01:30:00Z".parse().unwrap()));

    // A time already past waits a little instead of regenerating at once.
    let past = answer.replace("2026-10-05T09:30", "2026-10-04T08:00");
    let stale_at = parse(&past, &now).unwrap().stale_at;
    assert_eq!(stale_at, Some(now.time + MIN_STALE));
}

fn at(latitude: f64, longitude: f64) -> Location {
    Location {
        latitude,
        longitude,
        accuracy: 10.0,
        timezone: "UTC".into(),
        place: None,
        updated_at: Utc::now(),
    }
}

#[test]
fn starters_go_stale_after_moving_to_another_city() {
    let dubai = at(25.2, 55.27);
    let cached = Cached {
        expires: Instant::now() + FRESH,
        suggestions: PromptSuggestions {
            suggestions: fallback(),
            personal: true,
        },
        location: Some(dubai.clone()),
    };
    assert!(cached.fresh(Some(&dubai)));
    assert!(cached.fresh(Some(&at(25.25, 55.3))));
    assert!(cached.fresh(None));
    assert!(!cached.fresh(Some(&at(1.29, 103.85))));
}
