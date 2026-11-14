use std::sync::{Mutex, Once};

use log::{Level, LevelFilter, Log, Metadata, Record};

use super::*;

static LINES: Mutex<Vec<(Level, String)>> = Mutex::new(Vec::new());

/// Collects what plugins log, for every test in the crate.
struct Capture;

impl Log for Capture {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if record.target() == TARGET {
            let mut lines = LINES.lock().unwrap_or_else(|error| error.into_inner());
            lines.push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

/// The lines logged for plugin `id` so far, without the `plugin: <id>: ` start.
pub(crate) fn captured(id: &str) -> Vec<(Level, String)> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = log::set_logger(&Capture);
        log::set_max_level(LevelFilter::Trace);
    });

    let start = format!("{id}: ");
    LINES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter_map(|(level, text)| Some((*level, text.strip_prefix(&start)?.to_owned())))
        .collect()
}

fn id(text: &str) -> PluginId {
    text.parse().unwrap()
}

#[test]
fn a_line_is_one_line_cut_to_the_cap() {
    captured("cap-test");
    let long = format!("a\nb{}", "é".repeat(MAX_LINE));

    emit(&id("cap-test"), Level::Info, &long);

    let lines = captured("cap-test");
    assert_eq!(lines.len(), 1);
    let (level, text) = &lines[0];
    assert_eq!(*level, Level::Info);
    assert!(text.starts_with("a\\nbé"));
    assert!(text.ends_with("..."));
    assert!(text.len() <= MAX_LINE + 3);
}

#[test]
fn a_flood_is_cut_to_twenty_lines_a_second_and_the_rest_counted() {
    captured("flood-test");
    let mut console = Console::new(id("flood-test"));
    let start = Instant::now();

    for n in 0..25 {
        console.line(start, Level::Info, &format!("line {n}"));
    }
    assert_eq!(captured("flood-test").len(), 20);

    console.line(start + SECOND, Level::Info, "next");

    let lines = captured("flood-test");
    assert_eq!(lines.len(), 22);
    assert_eq!(lines[20], (Level::Warn, "dropped 5 lines".to_owned()));
    assert_eq!(lines[21].1, "next");
}

#[test]
fn dropped_lines_are_reported_when_the_plugin_stops() {
    captured("stop-test");
    let mut console = Console::new(id("stop-test"));
    let start = Instant::now();
    for _ in 0..23 {
        console.line(start, Level::Info, "x");
    }

    drop(console);

    assert_eq!(
        captured("stop-test").last().map(|(_, text)| text.as_str()),
        Some("dropped 3 lines")
    );
}
