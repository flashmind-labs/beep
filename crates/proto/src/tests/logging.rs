use super::*;

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
