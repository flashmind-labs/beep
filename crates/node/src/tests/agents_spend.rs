//! Passes over fabricated logs of every agent.

use super::*;
use crate::logs::spend::billing;
use beep_proto::node::usage::Billing;
use beep_proto::watched;
use std::{collections::BTreeMap, fs, io::Write, path::Path};

/// 2026-11-03T09:00:00Z.
const NOW: u64 = 1_793_696_400;

fn claude_line(session: &str, message: &str, at: &str, input: u64) -> String {
    format!(
        "{{\"type\":\"assistant\",\"sessionId\":\"{session}\",\"cwd\":\"/srv/project\",\"timestamp\":\"{at}\",\"requestId\":\"req_{message}\",\"message\":{{\"id\":\"{message}\",\"model\":\"claude-opus-5\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}}}}\n"
    )
}

fn append(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).expect("folders");
    }

    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .expect("a log line");
}

struct Fixture {
    _dir: tempfile::TempDir,
    roots: Roots,
    ledger: PathBuf,
    claude_main: PathBuf,
}

/// Logs of Claude Code (a session and its subagent), Codex, pi and
/// OpenCode, all written now.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("a temp folder");
    let root = dir.path();
    let claude_main = root.join("claude/projects/-srv-project/s1.jsonl");
    let line = claude_line("s1", "m1", "2026-11-03T08:10:00.000Z", 1_000_000);
    // Claude Code writes a message once per content block.
    append(
        &claude_main,
        &format!("{line}{line}{{\"type\":\"user\"}}\n"),
    );
    append(
        &root.join("claude/projects/-srv-project/s1/subagents/agent-a1.jsonl"),
        &claude_line("s1", "m2", "2026-11-03T08:20:00.000Z", 200_000),
    );
    append(
        &root.join("codex/sessions/2026/11/03/rollout-2026-11-03T08-00-00-t1.jsonl"),
        concat!(
            "{\"timestamp\":\"2026-11-03T08:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\",\"cwd\":\"/srv/other\"}}\n",
            "{\"timestamp\":\"2026-11-03T08:00:01.000Z\",\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6.1-sol\"}}\n",
            "{\"timestamp\":\"2026-11-03T08:01:00.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000000,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":1000000},\"last_token_usage\":{\"input_tokens\":1000000,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":1000000}}}}\n",
            "{\"timestamp\":\"2026-11-03T08:01:01.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000000,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":1000000},\"last_token_usage\":{\"input_tokens\":1000000,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":1000000}}}}\n",
        ),
    );
    append(
        &root.join("pi/--srv-project--/2026-11-03T08-00-00-000Z_p1.jsonl"),
        concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"p1\",\"timestamp\":\"2026-11-03T08:00:00.000Z\",\"cwd\":\"/srv/project\"}\n",
            "{\"type\":\"message\",\"timestamp\":\"2026-11-03T08:05:00.000Z\",\"message\":{\"role\":\"assistant\",\"model\":\"example/model\",\"usage\":{\"input\":100,\"output\":10,\"cacheRead\":0,\"cacheWrite\":0,\"cost\":{\"total\":0.5}}}}\n",
        ),
    );
    let opencode = root.join("opencode/opencode.db");
    fs::create_dir_all(opencode.parent().expect("a folder")).expect("folders");
    databases::tests::opencode_database(&opencode);
    Fixture {
        roots: Roots {
            claude: Some(root.join("claude/projects")),
            codex: Some(root.join("codex/sessions")),
            pi: Some(root.join("pi")),
            flsh: None,
            opencode: Some(root.join("opencode")),
            agy: None,
            accounts: billing::AccountFiles::default(),
        },
        ledger: root.join("cache/agent-spend.json"),
        claude_main,
        _dir: dir,
    }
}

fn by_session(reports: &[SessionSpend]) -> BTreeMap<SessionId, &SessionSpend> {
    reports
        .iter()
        .map(|report| (report.session.clone(), report))
        .collect()
}

fn cost(report: &SessionSpend) -> f64 {
    report.hours.iter().map(|hour| hour.cost_usd).sum()
}

#[test]
fn a_pass_reports_every_agents_sessions_once_each() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    let reports = spend.pass(NOW, &mut heard);
    let sessions = by_session(&reports);
    let names: Vec<(&str, &str)> = sessions
        .values()
        .map(|report| (report.agent.as_ref(), report.session.as_ref()))
        .collect();
    assert_eq!(
        names,
        [
            ("pi", "p1"),
            ("claude", "s1"),
            ("opencode", "ses_a"),
            ("opencode", "ses_b"),
            ("codex", "t1")
        ]
    );
    // One copy of 1M input at $5, and the subagent's 0.2M.
    let claude = sessions["s1"];
    assert!((cost(claude) - 6.0).abs() < 1e-9, "{claude:?}");
    assert_eq!(claude.cwd, "/srv/project");
    assert_eq!(claude.since, NOW - WINDOW_SECS);
    // The repeated count once, at $2 a million.
    let codex = sessions["t1"];
    assert!((cost(codex) - 2.0).abs() < 1e-9, "{codex:?}");
    assert_eq!(codex.cwd, "/srv/other");
    assert_eq!(codex.hours[0].model, "gpt-6.1-sol");
    assert_eq!(codex.hours[0].hour, 1_793_692_800);
    assert!((cost(sessions["p1"]) - 0.5).abs() < 1e-9);
    // The newest first.
    assert_eq!(reports[0].hours[0].hour, 1_793_692_800);
}

#[test]
fn a_new_stream_hears_every_session_and_takes_none_from_an_older_one() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut old = Heard::default();
    let reports = spend.pass(NOW, &mut old);
    assert!(!reports.is_empty());

    assert!(spend.pass(NOW, &mut old).is_empty(), "an older stream heard them all");
    assert_eq!(spend.pass(NOW, &mut Heard::default()).len(), reports.len());

    append(
        &fixture.claude_main,
        &claude_line("s1", "m3", "2026-11-03T08:30:00.000Z", 1_000),
    );
    assert_eq!(
        spend.pass(NOW + 1, &mut Heard::default()).len(),
        reports.len()
    );
    assert_eq!(
        spend.pass(NOW + 1, &mut old).len(),
        1,
        "the older stream still hears the change"
    );
}

#[test]
fn only_changed_sessions_go_again_and_a_restart_reads_nothing_twice() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    spend.pass(NOW, &mut heard);

    assert!(spend.pass(NOW, &mut heard).is_empty(), "nothing changed");

    append(
        &fixture.claude_main,
        &claude_line("s1", "m3", "2026-11-03T08:30:00.000Z", 1_000_000),
    );
    let reports = spend.pass(NOW, &mut heard);
    assert_eq!(reports.len(), 1);
    // The whole session, not just the new line: 1M + 0.2M + 1M at $5.
    assert!((cost(&reports[0]) - 11.0).abs() < 1e-9);
    assert!(spend.pass(NOW, &mut heard).is_empty());

    // A restart keeps what was read, and a stream after it hears every session.
    let again = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    let first = again.pass(NOW, &mut heard);
    assert!((cost(by_session(&first)["s1"]) - 11.0).abs() < 1e-9);
    assert!(again.pass(NOW, &mut heard).is_empty());
    append(
        &fixture.claude_main,
        &claude_line("s1", "m4", "2026-11-03T08:40:00.000Z", 1_000_000),
    );
    let reports = again.pass(NOW, &mut heard);
    assert_eq!(reports.len(), 1);
    assert!((cost(&reports[0]) - 16.0).abs() < 1e-9);
}

#[test]
fn a_line_still_being_written_waits() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    spend.pass(NOW, &mut heard);

    let line = claude_line("s1", "m5", "2026-11-03T08:50:00.000Z", 1_000_000);
    let (head, tail) = line.split_at(40);
    append(&fixture.claude_main, head);
    assert!(spend.pass(NOW, &mut heard).is_empty(), "half a line counts nothing");
    append(&fixture.claude_main, tail);
    let reports = spend.pass(NOW, &mut heard);
    assert!((cost(&reports[0]) - 11.0).abs() < 1e-9);
}

#[test]
fn the_window_starts_on_an_hour() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    // The window starts after ses_a's message (1_793_614_600) but within
    // its hour, which the server replaces whole.
    let reports = spend.pass(1_793_614_650 + WINDOW_SECS, &mut heard);
    let sessions = by_session(&reports);
    let a = sessions
        .get("ses_a")
        .expect("the hour's first message counts");
    assert_eq!(a.since % 3600, 0);
    assert!((cost(a) - 3.0).abs() < 1e-9, "{a:?}");
}

#[test]
fn a_moved_transcript_is_not_counted_again_but_what_follows_is() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    let moved = fixture
        .roots
        .claude
        .clone()
        .expect("a claude root")
        .join("-work-app/s9.jsonl");
    let line = claude_line("s9", "m9", "2026-11-03T08:30:00.000Z", 1_000_000);
    append(&moved, &line);
    spend
        .mark_counted(Agent::Claude, &SessionId::from("s9"), &moved)
        .expect("marked");
    let reports = spend.pass(NOW, &mut heard);
    assert!(
        !by_session(&reports).contains_key("s9"),
        "the machine it came from counted that: {reports:?}"
    );
    // A turn run here is counted, once.
    append(
        &moved,
        &claude_line("s9", "m10", "2026-11-03T08:40:00.000Z", 200_000),
    );
    let reports = spend.pass(NOW, &mut heard);
    let sessions = by_session(&reports);
    assert!((cost(sessions["s9"]) - 1.0).abs() < 1e-9, "{reports:?}");
}

#[test]
fn a_session_that_comes_back_keeps_what_it_spent_here_before() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let path = fixture
        .roots
        .claude
        .clone()
        .expect("a claude root")
        .join("-work-app/s9.jsonl");
    let first = claude_line("s9", "m9", "2026-11-03T08:30:00.000Z", 1_000_000);
    append(&path, &first);
    let before = spend.pass(NOW, &mut Heard::default());
    assert!(
        (cost(by_session(&before)["s9"]) - 5.0).abs() < 1e-9,
        "{before:?}"
    );
    // It ran elsewhere meanwhile and returns with that transcript: the
    // folder is written differently and a turn was added.
    let back = format!(
        "{}{}",
        first.replace("/srv/project", "/home/user/app"),
        claude_line("s9", "m10", "2026-11-03T08:40:00.000Z", 200_000)
    );
    fs::write(&path, back).expect("the transcript comes back");
    spend
        .mark_counted(Agent::Claude, &SessionId::from("s9"), &path)
        .expect("marked");
    let after = spend.pass(NOW, &mut Heard::default());
    assert!(
        (cost(by_session(&after)["s9"]) - 5.0).abs() < 1e-9,
        "the hours it spent here stay, and the turn run elsewhere is not added: {after:?}"
    );
    append(
        &path,
        &claude_line("s9", "m11", "2026-11-03T08:50:00.000Z", 100_000),
    );
    let reports = spend.pass(NOW, &mut Heard::default());
    assert!(
        (cost(by_session(&reports)["s9"]) - 5.5).abs() < 1e-9,
        "{reports:?}"
    );
}

#[test]
fn a_moved_codex_session_keeps_its_folder_and_provider_for_what_runs_here() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    let path = fixture
        .roots
        .codex
        .clone()
        .expect("a codex root")
        .join("2026/11/03/rollout-2026-11-03T07-00-00-t9.jsonl");
    let count = |at: &str, total: u64| {
        format!(
            "{{\"timestamp\":\"{at}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":{total},\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":{total}}},\"last_token_usage\":{{\"input_tokens\":1000000,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":1000000}}}}}}}}\n"
        )
    };
    append(
        &path,
        &format!(
            "{}{}{}",
            "{\"timestamp\":\"2026-11-03T07:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"t9\",\"cwd\":\"/work/app\",\"model_provider\":\"openrouter\"}}\n",
            "{\"timestamp\":\"2026-11-03T07:00:01.000Z\",\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6.1-sol\"}}\n",
            count("2026-11-03T07:01:00.000Z", 1_000_000),
        ),
    );
    spend
        .mark_counted(Agent::Codex, &SessionId::from("t9"), &path)
        .expect("marked");
    // A turn run here: its lines name neither the folder nor the provider
    // nor the model, the header did.
    append(&path, &count("2026-11-03T08:30:00.000Z", 2_000_000));
    let reports = spend.pass(NOW, &mut heard);
    let report = by_session(&reports)["t9"];
    assert_eq!(report.cwd, "/work/app");
    assert!(
        (cost(report) - 2.0).abs() < 1e-9,
        "only the new turn: {report:?}"
    );
    assert_eq!(report.hours[0].billing, Billing::Metered);
}

#[test]
fn a_pi_sessions_cost_is_read_from_its_log() {
    let fixture = fixture();
    let sessions = fixture.roots.pi.as_deref().expect("pi's folder");
    let cost = super::pi_session_cost(sessions, &SessionId::from(&"p1".to_owned()));

    assert!(
        cost.is_some_and(|cost| (cost - 0.5).abs() < 1e-9),
        "{cost:?}"
    );
    assert_eq!(
        super::pi_session_cost(sessions, &SessionId::from(&"p2".to_owned())),
        None
    );
}

#[test]
fn imported_openrouter_hours_replace_estimates_after_the_session_was_scanned() {
    let mut fixture = fixture();
    let root = fixture._dir.path().join("flsh");
    let path = root.join("folder/session.jsonl");
    let hour = NOW - 3600;
    let header = serde_json::json!({
        "sessionId": "chat",
        "metadata": {
            "model": "openrouter:example",
            "working_dir": "/srv/project",
            "total_usage": {"input_tokens": 30, "total_cost": "1.00"},
            "usage_log": [
                {"at": hour, "input_tokens": 20, "cached_input_tokens": 0, "output_tokens": 0, "cost": "1.00"},
                {"at": NOW, "input_tokens": 10, "cached_input_tokens": 0, "output_tokens": 0, "cost": "0", "cost_known": true}
            ]
        }
    });
    append(&path, &format!("{header}\n"));
    fixture.roots.flsh = Some(root);
    let spend = Spend::at(fixture.roots, fixture.ledger);
    let mut heard = Heard::default();
    spend.pass(NOW, &mut heard);

    let filled = serde_json::json!({hour.to_string(): {
        "example": {"input": 20, "cached": 5, "output": 2, "usd": 2.0},
        "another": {"input": 4, "cached": 0, "output": 3, "usd": 1.25}
    }});
    let billed = path.with_extension("openrouter.json");
    fs::write(&billed, filled.to_string()).unwrap();
    let reports = spend.pass(NOW, &mut heard);
    assert_eq!(reports.len(), 1);
    let chat = &reports[0];
    assert_eq!(chat.session.as_ref(), "chat");
    assert_eq!(chat.hours.len(), 3);
    assert_eq!(cost(chat), 3.25);
    assert_eq!(chat.hours.iter().map(|h| h.tokens()).sum::<u64>(), 39);
    assert!(spend.pass(NOW, &mut heard).is_empty());
    assert_eq!(cost(by_session(&spend.pass(NOW, &mut Heard::default()))["chat"]), 3.25);

    let mut empty = header;
    empty["metadata"]["total_usage"] = serde_json::Value::Null;
    empty["metadata"]["usage_log"] = serde_json::json!([]);
    fs::write(&path, format!("{empty}\n")).unwrap();
    assert_eq!(cost(&spend.pass(NOW, &mut heard)[0]), 3.25);

    fs::remove_file(billed).unwrap();
    assert!(!by_session(&spend.pass(NOW, &mut Heard::default())).contains_key("chat"));
}

#[test]
fn resuming_a_legacy_flsh_session_corrects_its_cached_hour() {
    let mut fixture = fixture();
    let root = fixture._dir.path().join("flsh");
    let path = root.join("folder/session.jsonl");
    let header = serde_json::json!({
        "sessionId": "chat",
        "metadata": {
            "working_dir": "/srv/project",
            "model": "openrouter:example",
            "total_usage": {"input_tokens": 40, "total_cost": "0.50"}
        }
    });
    append(&path, &format!("{header}\n{{}}\n"));
    append(
        &path,
        "{\"type\":\"system\",\"timestamp\":\"2026-11-03T09:00:00Z\"}\n",
    );
    append(
        &path,
        "{\"type\":\"user\",\"timestamp\":\"2026-11-02T09:04:00Z\"}\n",
    );
    fixture.roots.flsh = Some(root);
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let mut heard = Heard::default();
    spend.pass(NOW, &mut heard);
    assert!(spend.pass(NOW, &mut heard).is_empty());
    drop(spend);

    let mut cache: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.ledger).unwrap()).unwrap();
    let database = &mut cache["databases"][path.to_str().unwrap()];
    database
        .as_object_mut()
        .unwrap()
        .remove("separates_historical_usage");
    database["parts"]["chat"]
        .as_object_mut()
        .unwrap()
        .remove("historical");
    database["parts"]["chat"]["buckets"] = serde_json::json!([[{"hour":NOW,"model":"openrouter:example","billing":"metered"},
        {"tokens":{"input":40,"output":0,"cache_read":0,"cache_write":0,"cache_write_long":0},"cost_usd":0.5,"unpriced":0}]]);

    fs::write(&fixture.ledger, serde_json::to_vec(&cache).unwrap()).unwrap();

    let spend = Spend::at(fixture.roots, fixture.ledger);
    let mut heard = Heard::default();
    let corrected = spend.pass(NOW, &mut heard);
    let chat = by_session(&corrected)["chat"];
    assert!(chat.hours.is_empty());
    assert_eq!(chat.historical[0].cost_usd, 0.5);
    assert_eq!(chat.historical[0].first, Some(NOW - 86400 + 240));

    append(
        &path,
        "{\"type\":\"user\",\"timestamp\":\"2026-11-03T09:03:00Z\"}\n",
    );
    let resumed = spend.pass(NOW, &mut heard);
    let chat = by_session(&resumed)["chat"];
    assert!(chat.hours.is_empty());
    assert_eq!(chat.historical[0].last, Some(NOW + 180));
    assert_eq!(chat.historical[0].cost_usd, 0.5);

    let receipt = |input: u64, usd: f64| {
        serde_json::json!({
            NOW.to_string(): {"example": {"input":input, "cached":0, "output":0, "usd":usd}}
        })
    };
    let sidecar = path.with_extension("openrouter.json");
    fs::write(&sidecar, receipt(20, 0.30).to_string()).unwrap();
    let partial = spend.pass(NOW, &mut heard);
    let chat = by_session(&partial)["chat"];
    assert_eq!(cost(chat), 0.30);
    assert_eq!(chat.historical[0].tokens, 20);
    assert!((chat.historical[0].cost_usd - 0.20).abs() < 1e-9);
    fs::write(&sidecar, receipt(40, 0.45).to_string()).unwrap();
    let complete = spend.pass(NOW, &mut heard);
    let chat = by_session(&complete)["chat"];
    assert_eq!(cost(chat), 0.45);
    assert!(chat.historical.is_empty());
}

#[tokio::test]
async fn streams_share_one_scan_and_each_sees_what_it_reported() {
    let fixture = fixture();
    let spend = Arc::new(Spend::at(fixture.roots.clone(), fixture.ledger.clone()));
    let (changes, _) = tokio::sync::broadcast::channel(8);
    let (first, mut first_reports) = spend.follow(LogChanges::new(changes.subscribe(), None));
    let (second, mut second_reports) = spend.follow(LogChanges::new(changes.subscribe(), None));

    assert!(Arc::ptr_eq(&first, &second));

    // The scan's own first pass reads the fabricated logs at the real clock,
    // which may be past their window: publish a report by hand instead.
    let report = SessionSpend {
        agent: "claude".into(),
        session: "s1".into(),
        ..SessionSpend::default()
    };
    spend.publish(vec![report.clone()]);
    first_reports.changed().await.unwrap();
    second_reports.changed().await.unwrap();

    assert_eq!(watched::read(&first_reports, |all| all.len()), 1);
    assert_eq!(watched::read(&second_reports, |all| all.len()), 1);
    // The same report again is no news.
    spend.publish(vec![report]);
    assert!(!first_reports.has_changed().unwrap());

    drop((first, second));
    let (again, _) = spend.follow(LogChanges::new(changes.subscribe(), None));
    assert_eq!(Arc::strong_count(&again), 1);
}

#[tokio::test]
async fn only_a_change_of_the_agents_logs_wakes_the_scan() {
    use beep_proto::node::events::Topic;

    let hub = crate::events::Hub::default();
    let mut logs = LogChanges::new(hub.subscribe(), None);
    hub.changed(Topic::new("terminals"));
    hub.changed(Topic::new("agents"));
    let mut waiting = Box::pin(logs.changed());
    assert!(futures::poll!(&mut waiting).is_pending());
    drop(waiting);

    hub.changed(Topic::new("agent"));
    logs.changed().await;

    // Rings that came before a read that covers them are forgotten.
    hub.changed(Topic::keyed("agent", "s1"));
    logs.settle();
    let mut waiting = Box::pin(logs.changed());
    assert!(futures::poll!(&mut waiting).is_pending());
}
