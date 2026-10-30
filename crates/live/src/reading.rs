//! Reading a reply aloud as it streams: the text cut into sentences to
//! speak one at a time, and when to fill a long quiet while tools run.

use beep_proto::MAX_SAY_CHARS;
use std::time::{Duration, Instant};

/// Cuts streamed reply text into sentences fit to be spoken: markdown
/// taken out, code and tables skipped.
#[derive(Debug, Default)]
pub struct Reading {
    buf: String,
    in_code: bool,
}

impl Reading {
    /// Adds `delta` and returns the sentences it finished.
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buf.push_str(delta);
        let mut out = Vec::new();
        while let Some(cut) = self.boundary() {
            let piece: String = self.buf.drain(..cut).collect();
            self.take(&piece, &mut out);
        }
        out
    }

    /// The text left once the reply ended.
    pub fn finish(&mut self) -> Vec<String> {
        let rest = std::mem::take(&mut self.buf);
        let mut out = Vec::new();
        self.take(&rest, &mut out);
        self.in_code = false;
        out
    }

    /// Where the first finished piece ends: after a newline, or after a
    /// sentence's end followed by a space. Inside code only lines count.
    fn boundary(&self) -> Option<usize> {
        let mut chars = self.buf.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '\n' {
                return Some(i + 1);
            }
            if self.in_code || !matches!(c, '.' | '!' | '?' | '…') {
                continue;
            }
            if let Some(&(j, next)) = chars.peek()
                && next.is_whitespace()
            {
                return Some(j);
            }
        }
        None
    }

    fn take(&mut self, piece: &str, out: &mut Vec<String>) {
        let line = piece.trim();
        if line.starts_with("```") {
            self.in_code = !self.in_code;
            return;
        }
        if self.in_code || line.starts_with('|') {
            return;
        }
        let text = speakable(line);
        if text.chars().any(char::is_alphanumeric) {
            out.extend(fit(&text));
        }
    }
}

/// `line` without markdown: no headings, list marks, emphasis, code
/// marks, or link targets.
fn speakable(line: &str) -> String {
    let mut line = line.trim_start_matches('#').trim_start();
    for mark in ["- ", "* ", "+ ", "> "] {
        if let Some(rest) = line.strip_prefix(mark) {
            line = rest;
        }
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find("](").and_then(|close| {
            let target = &after[close + 2..];
            target.find(')').map(|end| (close, close + 2 + end + 1))
        }) {
            Some((close, end)) => {
                out.push_str(&after[..close]);
                rest = &after[end..];
            }
            None => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    let words: Vec<&str> = out
        .split_whitespace()
        .filter(|w| !w.starts_with("http://") && !w.starts_with("https://"))
        .collect();
    words
        .join(" ")
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '_' | '~'))
        .collect()
}

/// `text` in pieces of at most `MAX_SAY_CHARS`, cut at spaces.
fn fit(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut piece = String::new();
    for word in text.split(' ') {
        if !piece.is_empty() && piece.chars().count() + 1 + word.chars().count() > MAX_SAY_CHARS {
            out.push(std::mem::take(&mut piece));
        }
        if !piece.is_empty() {
            piece.push(' ');
        }
        piece.extend(word.chars().take(MAX_SAY_CHARS));
    }
    if !piece.is_empty() {
        out.push(piece);
    }
    out
}

/// Quiet before a tool step gets a line of its own.
pub const STEP_QUIET: Duration = Duration::from_secs(2);
/// Quiet before Beep says the work goes on.
pub const STILL_QUIET: Duration = Duration::from_secs(10);
/// No filler before the run is this old: a quick answer needs none.
pub const FIRST_FILLER: Duration = Duration::from_millis(1500);
/// Fillers in one turn, at most.
pub const MAX_FILLERS: u32 = 2;

/// A line to say while the reply is quiet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filler {
    /// The tool step it is about, or the last one seen.
    pub step: String,
    /// The line that says the work goes on.
    pub still: bool,
}

/// Decides when the quiet during a run gets a filler.
#[derive(Debug)]
pub struct Fillers {
    sent_at: Instant,
    /// When something was last spoken, or the message sent.
    voice_at: Instant,
    speaking: bool,
    /// A step that started and was not talked about yet.
    step: Option<String>,
    last_step: String,
    said: u32,
    still_said: bool,
}

impl Fillers {
    pub fn new(now: Instant) -> Self {
        Self {
            sent_at: now,
            voice_at: now,
            speaking: false,
            step: None,
            last_step: String::new(),
            said: 0,
            still_said: false,
        }
    }

    /// A tool step started.
    pub fn tool(&mut self, step: &str) {
        self.step = Some(step.to_owned());
        step.clone_into(&mut self.last_step);
    }

    /// Something is being spoken, or nothing is anymore.
    pub fn speaking(&mut self, on: bool, now: Instant) {
        self.speaking = on;
        self.voice_at = now;
    }

    /// The filler due now, if any.
    pub fn due(&mut self, now: Instant) -> Option<Filler> {
        if self.speaking
            || self.said >= MAX_FILLERS
            || now.saturating_duration_since(self.sent_at) < FIRST_FILLER
        {
            return None;
        }
        let quiet = now.saturating_duration_since(self.voice_at);
        if quiet >= STEP_QUIET
            && let Some(step) = self.step.take()
        {
            self.said += 1;
            return Some(Filler { step, still: false });
        }
        if quiet >= STILL_QUIET && !self.still_said {
            self.still_said = true;
            self.said += 1;
            return Some(Filler {
                step: self.last_step.clone(),
                still: true,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(parts: &[&str]) -> Vec<String> {
        let mut r = Reading::default();
        let mut out: Vec<String> = parts.iter().flat_map(|p| r.push(p)).collect();
        out.extend(r.finish());
        out
    }

    #[test]
    fn sentences_come_out_as_they_finish() {
        let mut r = Reading::default();
        assert!(r.push("Let me check").is_empty());
        assert_eq!(
            r.push(" build-box. The disk"),
            vec!["Let me check build-box."]
        );
        assert!(r.push(" is 91.5% full").is_empty());
        assert_eq!(r.finish(), vec!["The disk is 91.5% full"]);
    }

    #[test]
    fn markdown_is_not_read_out() {
        assert_eq!(
            read(&["## Plan\n- **Move** the `dentist` to [Friday](https://x.test/a).\n"]),
            vec!["Plan", "Move the dentist to Friday."]
        );
    }

    #[test]
    fn code_and_tables_are_skipped() {
        assert_eq!(
            read(&["Run this:\n```\ndf -h. Then\n```\n| a | b |\n|---|---|\nDone."]),
            vec!["Run this:", "Done."]
        );
    }

    #[test]
    fn long_text_is_cut_at_spaces() {
        let long = "word ".repeat(400);
        let out = read(&[&long]);
        assert!(out.len() > 1);
        assert!(out.iter().all(|p| p.chars().count() <= MAX_SAY_CHARS));
    }

    #[test]
    fn a_quick_answer_gets_no_filler() {
        let t = Instant::now();
        let mut f = Fillers::new(t);
        f.tool("Running df -h on build-box");
        assert_eq!(f.due(t + Duration::from_millis(1000)), None);
        f.speaking(true, t + Duration::from_millis(1200));
        assert_eq!(f.due(t + Duration::from_secs(5)), None);
    }

    #[test]
    fn a_quiet_tool_step_gets_one_line_then_still_on_it() {
        let t = Instant::now();
        let mut f = Fillers::new(t);
        f.tool("Running df -h on build-box");
        assert_eq!(
            f.due(t + STEP_QUIET),
            Some(Filler {
                step: "Running df -h on build-box".into(),
                still: false
            })
        );
        f.speaking(true, t + STEP_QUIET);
        f.speaking(false, t + Duration::from_secs(3));
        assert_eq!(f.due(t + Duration::from_secs(8)), None);
        assert_eq!(
            f.due(t + Duration::from_secs(13)).map(|f| f.still),
            Some(true)
        );
        f.tool("Running docker image prune on build-box");
        assert_eq!(f.due(t + Duration::from_secs(60)), None);
    }

    #[test]
    fn a_step_after_speech_waits_for_the_quiet() {
        let t = Instant::now();
        let mut f = Fillers::new(t);
        f.speaking(true, t);
        f.speaking(false, t + Duration::from_secs(2));
        f.tool("Searching Fastmail");
        assert_eq!(f.due(t + Duration::from_secs(3)), None);
        assert!(f.due(t + Duration::from_secs(4)).is_some());
    }
}
