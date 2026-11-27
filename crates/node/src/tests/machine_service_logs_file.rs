use super::*;
use crate::machine::service::logs::LogOptions;
use std::sync::mpsc;

/// A node log line, with its newline.
fn line(text: &str) -> String {
    format!("1970-01-01T00:00:01.000001Z  INFO beep_node: {text}\n")
}

/// Sends each write to the test.
struct Sink(mpsc::Sender<Vec<u8>>);

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(bytes.to_vec());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_followed_log_prints_a_line_when_it_is_appended_and_after_a_rollover() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let path = dir.path().join("beep.log");
    fs::write(&path, line("first")).expect("the log is written");
    let file = File::open(&path).expect("the log opens");
    let (sent, printed) = mpsc::channel();
    let followed = path.clone();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(file);
        let mut printer = Printer::new(
            &LogOptions {
                since: std::time::Duration::ZERO,
                follow: true,
                lines: None,
                all: true,
            },
            0,
            Sink(sent),
        );
        let mut pending = Vec::new();
        let read = drain(&mut reader, &mut pending, &mut printer).expect("the log reads");
        let _ = follow(&followed, reader, pending, read, &mut printer);
    });
    let mut text = String::new();

    let next = |text: &mut String| {
        while !text.ends_with('\n') || text.is_empty() {
            let bytes = printed.recv().expect("the follower prints");
            text.push_str(&String::from_utf8_lossy(&bytes));
        }
    };
    next(&mut text);
    assert_eq!(text, line("first"));

    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("the log opens");
    write!(log, "{}", line("second")).expect("the log grows");
    text.clear();
    next(&mut text);
    assert_eq!(text, line("second"));

    fs::write(&path, line("third")).expect("the log is truncated");
    text.clear();
    next(&mut text);
    assert_eq!(text, line("third"));
}
