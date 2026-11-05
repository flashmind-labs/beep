//! Passes over fabricated logs of every agent.

use super::*;
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
            accounts: billing::AccountFiles::default(),
        },
        ledger: root.join("cache/agent-spend.json"),
        claude_main,
        _dir: dir,
    }
}

fn by_session(reports: &[SessionSpend]) -> BTreeMap<String, &SessionSpend> {
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
    let reports = spend.pass(NOW);
    let sessions = by_session(&reports);
    let names: Vec<(&str, &str)> = sessions
        .values()
        .map(|report| (report.agent.as_str(), report.session.as_str()))
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
    let reports = spend.pass(NOW);
    assert!(!reports.is_empty());
    for report in &reports {
        spend.sent(report);
    }
    assert!(spend.pass(NOW).is_empty(), "an older stream heard them all");
    assert_eq!(spend.pass_all(NOW).len(), reports.len());

    append(
        &fixture.claude_main,
        &claude_line("s1", "m3", "2026-11-03T08:30:00.000Z", 1_000),
    );
    assert_eq!(spend.pass_all(NOW + 1).len(), reports.len());
    assert_eq!(
        spend.pass(NOW + 1).len(),
        1,
        "the older stream still hears the change"
    );
}

#[test]
fn only_changed_sessions_go_again_and_a_restart_reads_nothing_twice() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    for report in spend.pass(NOW) {
        spend.sent(&report);
    }
    assert!(spend.pass(NOW).is_empty(), "nothing changed");

    append(
        &fixture.claude_main,
        &claude_line("s1", "m3", "2026-11-03T08:30:00.000Z", 1_000_000),
    );
    let reports = spend.pass(NOW);
    assert_eq!(reports.len(), 1);
    // The whole session, not just the new line: 1M + 0.2M + 1M at $5.
    assert!((cost(&reports[0]) - 11.0).abs() < 1e-9);
    for report in &reports {
        spend.sent(report);
    }
    assert!(spend.pass(NOW).is_empty());

    // A restart keeps what was read.
    let again = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    let first = again.pass(NOW);
    assert!(first.is_empty(), "{first:?}");
    append(
        &fixture.claude_main,
        &claude_line("s1", "m4", "2026-11-03T08:40:00.000Z", 1_000_000),
    );
    let reports = again.pass(NOW);
    assert_eq!(reports.len(), 1);
    assert!((cost(&reports[0]) - 16.0).abs() < 1e-9);
}

#[test]
fn a_line_still_being_written_waits() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    for report in spend.pass(NOW) {
        spend.sent(&report);
    }
    let line = claude_line("s1", "m5", "2026-11-03T08:50:00.000Z", 1_000_000);
    let (head, tail) = line.split_at(40);
    append(&fixture.claude_main, head);
    assert!(spend.pass(NOW).is_empty(), "half a line counts nothing");
    append(&fixture.claude_main, tail);
    let reports = spend.pass(NOW);
    assert!((cost(&reports[0]) - 11.0).abs() < 1e-9);
}

#[test]
fn the_window_starts_on_an_hour() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
    // The window starts after ses_a's message (1_793_614_600) but within
    // its hour, which the server replaces whole.
    let reports = spend.pass(1_793_614_650 + WINDOW_SECS);
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
    let moved = fixture
        .roots
        .claude
        .clone()
        .expect("a claude root")
        .join("-work-app/s9.jsonl");
    let line = claude_line("s9", "m9", "2026-11-03T08:30:00.000Z", 1_000_000);
    append(&moved, &line);
    spend
        .mark_counted(Agent::Claude, "s9", &moved)
        .expect("marked");
    let reports = spend.pass(NOW);
    assert!(
        !by_session(&reports).contains_key("s9"),
        "the machine it came from counted that: {reports:?}"
    );
    // A turn run here is counted, once.
    append(
        &moved,
        &claude_line("s9", "m10", "2026-11-03T08:40:00.000Z", 200_000),
    );
    let reports = spend.pass(NOW);
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
    let before = spend.pass_all(NOW);
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
        .mark_counted(Agent::Claude, "s9", &path)
        .expect("marked");
    let after = spend.pass_all(NOW);
    assert!(
        (cost(by_session(&after)["s9"]) - 5.0).abs() < 1e-9,
        "the hours it spent here stay, and the turn run elsewhere is not added: {after:?}"
    );
    append(
        &path,
        &claude_line("s9", "m11", "2026-11-03T08:50:00.000Z", 100_000),
    );
    let reports = spend.pass_all(NOW);
    assert!(
        (cost(by_session(&reports)["s9"]) - 5.5).abs() < 1e-9,
        "{reports:?}"
    );
}

#[test]
fn a_moved_codex_session_keeps_its_folder_and_provider_for_what_runs_here() {
    let fixture = fixture();
    let spend = Spend::at(fixture.roots.clone(), fixture.ledger.clone());
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
        .mark_counted(Agent::Codex, "t9", &path)
        .expect("marked");
    // A turn run here: its lines name neither the folder nor the provider
    // nor the model, the header did.
    append(&path, &count("2026-11-03T08:30:00.000Z", 2_000_000));
    let reports = spend.pass(NOW);
    let report = by_session(&reports)["t9"];
    assert_eq!(report.cwd, "/work/app");
    assert!(
        (cost(report) - 2.0).abs() < 1e-9,
        "only the new turn: {report:?}"
    );
    assert_eq!(report.hours[0].billing, Billing::Metered);
}
