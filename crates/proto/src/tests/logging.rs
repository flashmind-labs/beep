use super::*;
use tracing_subscriber::Layer;

#[test]
fn dates_are_civil_dates() {
    assert_eq!(date(0), "1970-01-01");
    assert_eq!(date(59), "1970-03-01");
    assert_eq!(date(20_760), "2026-11-03");
    assert_eq!(date(11_016), "2000-02-29");
}

#[test]
fn a_file_from_an_earlier_day_is_moved_aside_and_old_ones_pruned() {
    let dir = std::env::temp_dir().join(format!("beep-log-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let current = dir.join("beep.log");
    let two_days_ago = SystemTime::now() - Duration::from_secs(2 * 86_400);

    for n in 0..3 {
        let old = dir.join(format!("beep.2020-01-0{}.log", n + 1));
        fs::write(&old, "old\n").unwrap();
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(two_days_ago - Duration::from_secs(60 * (3 - n)))
            .unwrap();
    }

    fs::write(&current, "yesterday\n").unwrap();
    File::options()
        .write(true)
        .open(&current)
        .unwrap()
        .set_modified(two_days_ago)
        .unwrap();

    let rolling = RollingFile::new(&current, 2).unwrap();
    (&rolling).write_all(b"today\n").unwrap();

    assert_eq!(fs::read_to_string(&current).unwrap(), "today\n");
    let moved = dir.join(format!("beep.{}.log", date(day(two_days_ago))));
    assert_eq!(fs::read_to_string(&moved).unwrap(), "yesterday\n");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(
        names.contains(&"beep.2020-01-03.log".to_owned()),
        "{names:?}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn rotating_log_format_keeps_nested_span_fields_and_request_outcomes() {
    let output = Captured::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(TextLayout)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
    );
    tracing::subscriber::with_default(subscriber, || {
        let outer = tracing::info_span!("connection", stream = 7);
        let _outer = outer.enter();
        let inner = tracing::info_span!("query", query = 9);
        let _inner = inner.enter();
        let mut completed = RequestTrace::new("folder");
        completed.phase("sending");
        completed.finish(true);
        drop(completed);
        let mut failed = RequestTrace::new("worktrees");
        failed.finish(false);
        drop(failed);
        drop(RequestTrace::new("attachment"));
    });
    let bytes = output.0.lock().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(
        text.contains("connection{stream=7} query{query=9}"),
        "{text}"
    );
    assert!(text.contains("outcome=\"ok\""), "{text}");
    assert!(text.contains("outcome=\"error\""), "{text}");
    assert!(text.contains("outcome=\"cancelled\""), "{text}");
    assert!(text.contains("phase=\"sending\""), "{text}");
    assert!(text.lines().all(|line| line.contains("request=")), "{text}");
}

/// A busy app makes thousands of requests a minute: one that ends well and
/// quickly writes nothing at `INFO`, one that fails or is dropped one line.
#[test]
fn quick_requests_that_end_well_write_nothing_at_info() {
    let output = Captured::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(TextLayout)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
    );
    tracing::subscriber::with_default(subscriber, || {
        for _ in 0..100 {
            let mut quick = RequestTrace::new("External");
            quick.phase("handling");
            quick.phase("sending");
            quick.finish(true);
        }

        let mut failed = RequestTrace::new("worktrees");
        failed.phase("handling");
        failed.finish(false);
        drop(failed);
        drop(RequestTrace::new("attachment"));
    });
    let bytes = output.0.lock().unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(!text.contains("External"), "{text}");
    assert!(text.contains("outcome=\"error\""), "{text}");
    assert!(text.contains("outcome=\"cancelled\""), "{text}");
}

fn trace_in(outcome: &'static str, stream: bool) -> RequestTrace {
    let mut trace = RequestTrace::new("op");
    trace.outcome = outcome;
    trace.stream = stream;
    trace
}

#[test]
fn a_stream_ends_quietly_unless_it_failed() {
    let long = SLOW_REQUEST * 10;
    // A stream's end is not news, however long it lived.
    assert!(trace_in("cancelled", true).ends_quietly(long));
    assert!(trace_in("ok", true).ends_quietly(long));
    assert!(!trace_in("error", true).ends_quietly(Duration::ZERO));
    assert!(!trace_in("refused", true).ends_quietly(Duration::ZERO));
    // A request is.
    assert!(!trace_in("cancelled", false).ends_quietly(Duration::ZERO));
    assert!(!trace_in("ok", false).ends_quietly(long));
    assert!(trace_in("ok", false).ends_quietly(Duration::ZERO));
}

/// A subscriber that captures what is logged at `INFO` and above.
fn capture_info() -> (Captured, tracing::subscriber::DefaultGuard) {
    let output = Captured::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(TextLayout)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
    );

    (output, tracing::subscriber::set_default(subscriber))
}
