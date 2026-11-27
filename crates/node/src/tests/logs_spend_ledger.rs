use super::*;
use crate::logs::spend::Request;

fn request(session: &str, at: u64, key: Option<u64>, input: u64) -> Line {
    Line::Request(Request {
        session: Some(session.into()),
        cwd: Some("/srv/project".into()),
        at,
        model: Some("claude-opus-5".into()),
        tokens: Tokens {
            input,
            ..Tokens::default()
        },
        cost_usd: None,
        key,
        total: None,
        provider: None,
        billing: Billing::Unknown,
    })
}

#[test]
fn copies_of_a_message_count_once_and_hours_split() {
    let accounts = &mut Accounts::default();
    let mut log = LogFile::new(Agent::Claude);
    assert_eq!(
        log.take(request("s1", 7_200, Some(1), 1_000_000), accounts),
        Some(SessionId::from("s1"))
    );
    assert_eq!(
        log.take(request("s1", 7_201, Some(1), 1_000_000), accounts),
        None
    );
    log.take(request("s1", 10_900, Some(2), 2_000_000), accounts);
    let part = &log.parts["s1"];
    assert_eq!(part.cwd, "/srv/project");
    let costs: Vec<(u64, f64)> = part
        .buckets
        .iter()
        .map(|(key, bucket)| (key.hour, bucket.cost_usd))
        .collect();
    assert_eq!(costs, [(7_200, 5.0), (10_800, 10.0)]);
}

#[test]
fn a_repeated_running_total_counts_once() {
    let accounts = &mut Accounts::default();
    let mut log = LogFile::new(Agent::Codex);
    log.take(
        Line::Session {
            id: "t1".into(),
            cwd: "/srv/project".into(),
            provider: Some("openrouter".into()),
        },
        accounts,
    );
    log.take(Line::Model("gpt-9-unknown".into()), accounts);
    let count = |total| {
        Line::Request(Request {
            at: 100,
            tokens: Tokens {
                input: 10,
                ..Tokens::default()
            },
            total: Some(total),
            ..Request::default()
        })
    };
    log.take(count(10), accounts);
    log.take(count(10), accounts);
    log.take(count(20), accounts);
    let bucket = &log.parts["t1"].buckets[&BucketKey {
        hour: 0,
        model: "gpt-9-unknown".into(),
        billing: Billing::Metered,
    }];
    assert_eq!(bucket.tokens.input, 20);
    assert_eq!(bucket.unpriced, 20, "no price for it");
    assert_eq!(log.parts["t1"].cwd, "/srv/project");
}

#[test]
fn a_report_joins_a_session_across_logs() {
    let mut ledger = Ledger::default();
    let accounts = &mut Accounts::default();
    let mut main = LogFile::new(Agent::Claude);
    main.take(request("s1", 3_600, Some(1), 100), accounts);
    let mut sub = LogFile::new(Agent::Claude);
    sub.take(request("s1", 3_700, Some(2), 50), accounts);
    ledger.logs.insert("/a.jsonl".into(), main);
    ledger.logs.insert("/b.jsonl".into(), sub);
    let key = SessionKey {
        agent: Agent::Claude,
        session: "s1".into(),
    };
    let report = ledger.report(&key, 0).expect("a report");
    assert_eq!(report.agent, "claude");
    assert_eq!(report.hours.len(), 1);
    assert_eq!(report.hours[0].input_tokens, 150);
    assert_eq!(ledger.last_hour(&key), 3_600);
    let other = SessionKey {
        agent: Agent::Codex,
        session: "s1".into(),
    };
    assert!(ledger.report(&other, 0).is_none());
}
