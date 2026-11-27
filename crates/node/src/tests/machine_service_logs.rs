use super::print::parse_time;
use super::*;

const NOISE: &str = "2026-11-03T14:30:59.238909+01:00   INFO iroh::socket::transports: \
                     transports.rs:964 poll_send; len=42";
const SPAN: &str = "2026-11-03T14:31:00.000001+01:00   INFO tracing::span: actor.rs:796 tx;";
const NODE: &str = "2026-11-03T14:31:01.5+01:00   INFO beep_node::link: link.rs:10 connected";
const IROH_WARN: &str = "2026-11-03T14:31:02.000000+01:00   WARN iroh::socket: lost the relay";

fn options(lines: Option<usize>, all: bool) -> LogOptions {
    LogOptions {
        since: Duration::from_secs(3600),
        follow: false,
        lines,
        all,
    }
}

fn shown(options: &LogOptions, cutoff: i64, lines: &[&str]) -> Vec<String> {
    let mut printer = Printer::new(options, cutoff, Vec::new());

    for line in lines {
        printer.line(line).expect("writing to a Vec cannot fail");
    }

    printer.caught_up().expect("writing to a Vec cannot fail");
    String::from_utf8(printer.out)
        .expect("the lines written were UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn timestamps_read_with_their_offset() {
    assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_time("1970-01-01T01:00:00+01:00"), Some(0));
    assert_eq!(parse_time("1969-12-31T23:00:00.5-01:00"), Some(0));
    assert_eq!(
        parse_time("2026-11-03T14:30:59.238909+01:00"),
        Some(1_793_712_659)
    );
    assert_eq!(parse_time("2024-02-29T00:00:00Z"), Some(1_709_164_800));
    for wrong in [
        "Nov 03 14:30:59",
        "2026-11-03",
        "2026-13-03T00:00:00Z",
        "2026-11-03T14:30:59",
    ] {
        assert_eq!(parse_time(wrong), None, "{wrong}");
    }
}

#[test]
fn colors_do_not_hide_the_level() {
    let colored = "2026-11-03T14:30:59Z \x1b[32m INFO\x1b[0m iroh::net_report: report";
    assert_eq!(
        plain(colored),
        "2026-11-03T14:30:59Z  INFO iroh::net_report: report"
    );
    let mut filter = Filter::new(false, None);
    assert!(!filter.keep(colored));
}

#[test]
fn the_default_hides_noise_and_keeps_everything_else() {
    let lines = [NOISE, SPAN, NODE, "thread 'main' panicked", IROH_WARN];
    let cutoff = head(NOISE).expect("a timestamped line").time;
    assert_eq!(
        shown(&options(None, false), cutoff, &lines),
        [NODE, "thread 'main' panicked", IROH_WARN]
    );
    assert_eq!(shown(&options(None, true), cutoff, &lines), lines);
}

#[test]
fn lines_before_the_cutoff_are_left_out_with_their_continuations() {
    let old = "2026-11-03T13:00:00+01:00  WARN beep_node: old";
    let lines = [old, "  continued", NODE, "  continued too"];
    let cutoff = head(NOISE).expect("a timestamped line").time;
    assert_eq!(
        shown(&options(None, false), cutoff, &lines),
        [NODE, "  continued too"]
    );
}

#[test]
fn after_catching_up_lines_print_as_they_come() {
    let mut printer = Printer::new(&options(Some(1), false), 0, Vec::new());
    printer.line(NODE).expect("writing to a Vec cannot fail");
    printer
        .line(IROH_WARN)
        .expect("writing to a Vec cannot fail");
    printer.caught_up().expect("writing to a Vec cannot fail");
    printer
        .bytes(format!("{NODE}\r\n").as_bytes())
        .expect("writing to a Vec cannot fail");
    let out = String::from_utf8(printer.out).expect("the lines written were UTF-8");
    assert_eq!(out, format!("{IROH_WARN}\n{NODE}\n"));
}

/// A node log line written `secs` after the Unix epoch.
fn at(secs: i64, text: &str) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    format!("1970-01-01T{h:02}:{m:02}:{s:02}.000001Z  INFO beep_node: {text}")
}

fn log_file(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
    let path = dir.join(name);
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(&path, text).expect("the temp folder is writable");
    path
}

#[test]
fn window_keeps_lines_since_and_their_untimed_followers() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let trace = "   0: beep_node::link::connect".to_owned();
    let lines = [
        at(100, "old"),
        at(200, "kept"),
        trace.clone(),
        at(300, "new"),
    ];
    let file = log_file(dir.path(), "beep.log", &lines);
    let text = window(&[file], 150, 1 << 20).expect("the file reads");
    assert_eq!(text, format!("{}\n{trace}\n{}\n", lines[1], lines[3]));
}

#[test]
fn window_reads_rotated_files_oldest_first() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let old = log_file(dir.path(), "beep.1.log", &[at(100, "a"), at(200, "b")]);
    let new = log_file(dir.path(), "beep.log", &[at(300, "c")]);
    let text = window(&[old, new], 150, 1 << 20).expect("the files read");
    assert_eq!(text, format!("{}\n{}\n", at(200, "b"), at(300, "c")));
}

#[test]
fn window_skips_a_listed_file_that_is_missing() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let missing = dir.path().join("beep.2.log");
    let new = log_file(dir.path(), "beep.log", &[at(300, "c")]);
    let text = window(&[missing, new], 150, 1 << 20).expect("the file reads");
    assert_eq!(text, format!("{}\n", at(300, "c")));
}

#[test]
fn window_without_timestamps_gives_the_last_lines() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let lines: Vec<String> = (0..10).map(|n| format!("untimed {n}")).collect();
    assert!(lines.iter().all(|line| line.len() + 1 == 10));
    let file = log_file(dir.path(), "beep.log", &lines);
    let text = window(&[file], 150, 35).expect("the file reads");
    assert_eq!(text, "untimed 7\nuntimed 8\nuntimed 9\n");
}

/// A status-only report asks for no log, and reading the whole journal
/// for nothing made it miss its time limit.
#[test]
fn window_of_no_bytes_reads_nothing() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let unreadable = dir.path().to_owned();
    assert_eq!(window(&[unreadable], 150, 0).expect("nothing to read"), "");
}

#[test]
fn window_keeps_the_newest_whole_lines_under_the_cap() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let lines: Vec<String> = (0..5).map(|n| at(100 + n, "line")).collect();
    let file = log_file(dir.path(), "beep.log", &lines);
    let line = lines[0].len() + 1;
    let text = window(&[file], 0, 2 * line + line / 2).expect("the file reads");
    assert_eq!(text, format!("{}\n{}\n", lines[3], lines[4]));
}

#[test]
fn daily_log_queries_discover_files_created_after_startup() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let old = log_file(
        dir.path(),
        "beep.2026-01-01.log",
        &[at(100, "before"), at(200, "kept")],
    );
    File::options()
        .write(true)
        .open(&old)
        .expect("open old log")
        .set_modified(UNIX_EPOCH + Duration::from_secs(200))
        .expect("set time");
    let logs = Logs::in_directory(dir.path().to_owned());
    assert_eq!(
        logs.window(150, 1 << 20).expect("read window"),
        format!("{}\n", at(200, "kept"))
    );

    let new = log_file(dir.path(), "beep.2026-01-02.log", &[at(300, "new day")]);
    File::options()
        .write(true)
        .open(&new)
        .expect("open new log")
        .set_modified(UNIX_EPOCH + Duration::from_secs(300))
        .expect("set time");
    log_file(dir.path(), "other.log", &[at(400, "unrelated")]);
    let text = logs.window(150, 1 << 20).expect("read across midnight");
    assert_eq!(
        text,
        format!("{}\n{}\n", at(200, "kept"), at(300, "new day"))
    );
}

#[test]
fn daily_log_queries_keep_legacy_files_readable() {
    let dir = tempfile::tempdir().expect("a temp folder");
    log_file(dir.path(), "beep.1.log", &[at(200, "legacy")]);
    let logs = Logs::in_directory(dir.path().to_owned());
    assert_eq!(
        logs.window(150, 1 << 20).expect("read legacy log"),
        format!("{}\n", at(200, "legacy"))
    );
}

#[test]
fn an_empty_log_directory_does_not_query_the_service() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let logs = Logs::in_directory(dir.path().to_owned());
    assert!(
        logs.window(0, 1 << 20)
            .expect("read empty directory")
            .is_empty()
    );
}

fn log_query() -> beep_proto::node::logs::Query {
    beep_proto::node::logs::Query {
        start: "2026-01-02T00:00:00.500Z".into(),
        end: "2026-01-02T00:00:02Z".into(),
        search: None,
        regex: false,
        lines: 300,
        offset: 0,
        allow_untimed: false,
    }
}

#[test]
fn queries_use_precise_bounds_and_inherit_continuation_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log");
    std::fs::write(
        &path,
        "untimed\n\
        2026-01-02T00:00:00.499Z INFO app: before\n\
        2026-01-02T01:00:00.500+01:00 INFO app: first\n\
        continuation\n\
        2026-01-02T00:00:02Z INFO app: end\n\
        excluded continuation\n\
        2026-01-02T00:00:01Z INFO app: out of order\n",
    )
    .unwrap();
    let text = query(&[path], &log_query()).unwrap();
    assert!(text.contains("first\ncontinuation\n"), "{text}");
    assert!(text.contains("out of order"), "{text}");
    assert!(
        !text.contains("before") && !text.contains("excluded") && !text.contains("untimed"),
        "{text}"
    );
}

#[test]
fn queries_find_matches_before_the_old_byte_limit_and_search_literally() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log");
    let mut log = String::from("2026-01-02T00:00:01Z INFO app: retry [1]\n");
    log.push_str(&format!("2026-01-02T00:00:02Z INFO app: {}\n", "x".repeat(1000)).repeat(1100));
    std::fs::write(&path, log).unwrap();
    let mut args = log_query();
    args.search = Some("[1]".into());
    let text = query(std::slice::from_ref(&path), &args).unwrap();
    assert!(text.contains("retry [1]"), "{text}");
    args.offset = 1;
    assert!(query(&[path], &args).unwrap().is_empty());
}

#[test]
fn queries_reject_oversized_pages_instead_of_silently_skipping_matches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log");
    std::fs::write(
        &path,
        format!("2026-01-02T00:00:01Z INFO app: {}\n", "x".repeat(1000)).repeat(1100),
    )
    .unwrap();
    let args = beep_proto::node::logs::Query {
        lines: 2000,
        ..log_query()
    };
    assert!(
        query(&[path], &args)
            .unwrap_err()
            .to_string()
            .contains("request fewer lines")
    );
}
