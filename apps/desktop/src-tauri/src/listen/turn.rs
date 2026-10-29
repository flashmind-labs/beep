//! What the user said in one turn: phrases already settled, and the one
//! still coming in from the live transcript.

use super::command::{Command, phrase_command, send_at_end, sentences, strip_send};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Turn {
    /// Phrases kept, in order. Commands never land here.
    said: Vec<String>,
    /// The transcript since the last pause.
    current: String,
}

/// How a phrase closed at a pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// Nothing new since the last pause.
    Nothing,
    /// Words kept as part of the message.
    Kept,
    /// The phrase was a command, taken out of the words.
    Command(Command),
}

impl Turn {
    /// Adds words the transcript gave.
    pub fn hear(&mut self, delta: &str) {
        self.current.push_str(delta);
    }

    pub fn is_empty(&self) -> bool {
        self.text().is_empty()
    }

    /// Whether words came in since the last pause.
    pub fn is_open(&self) -> bool {
        !self.current.trim().is_empty()
    }

    /// The words so far, without a send word at the end.
    pub fn text(&self) -> String {
        let mut parts: Vec<&str> = self.said.iter().map(|s| s.trim()).collect();
        parts.push(strip_send(self.current.trim()));
        parts.retain(|p| !p.is_empty());
        parts.join(" ")
    }

    /// Whether the words so far end in a send word.
    #[cfg(test)]
    pub fn ends_in_send(&self) -> bool {
        send_at_end(&self.current).is_some()
    }

    /// Settles the phrase since the last pause. When its last sentence is
    /// a command on its own, or ends in a send word, the words before it
    /// are kept and the command is returned.
    pub fn close(&mut self) -> Closed {
        let current = std::mem::take(&mut self.current);
        let parts = sentences(&current);
        let Some((last, before)) = parts.split_last() else {
            return Closed::Nothing;
        };
        let (kept, command) = match phrase_command(last) {
            Some(command) => (before.concat(), Some(command)),
            None if send_at_end(last).is_some() => {
                (before.concat() + strip_send(last), Some(Command::Send))
            }
            None => (parts.concat(), None),
        };
        if !kept.trim().is_empty() {
            self.said.push(kept.trim().to_owned());
        }
        match command {
            Some(command) => Closed::Command(command),
            None => Closed::Kept,
        }
    }

    /// Drops every word.
    pub fn clear(&mut self) {
        self.said.clear();
        self.current.clear();
    }

    /// Removes the last sentence said.
    pub fn scratch(&mut self) {
        let Some(last) = self.said.pop() else {
            return;
        };
        let parts = sentences(&last);
        if let Some((_, before)) = parts.split_last() {
            let kept = before.concat();
            if !kept.trim().is_empty() {
                self.said.push(kept.trim().to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(words: &str) -> Turn {
        let mut t = Turn::default();
        t.hear(words);
        t
    }

    #[test]
    fn words_come_in_as_deltas() {
        let mut t = turn("Remind me");
        t.hear(" to call");
        assert_eq!(t.text(), "Remind me to call");
        assert!(t.is_open());
    }

    #[test]
    fn a_phrase_that_is_a_command_is_taken_out() {
        let mut t = turn("Book a table for four.");
        assert_eq!(t.close(), Closed::Kept);
        t.hear(" Cancel.");
        assert_eq!(t.close(), Closed::Command(Command::Cancel));
        assert_eq!(t.text(), "Book a table for four.");
    }

    #[test]
    fn a_command_after_a_sentence_without_a_pause_still_counts() {
        let mut t = turn("Text Sam that I'm late. Scratch that.");
        assert_eq!(t.close(), Closed::Command(Command::ScratchThat));
        assert_eq!(t.text(), "Text Sam that I'm late.");
    }

    #[test]
    fn scratch_removes_the_last_sentence() {
        let mut t = turn("Book a table. Make it at the Italian place.");
        t.close();
        t.scratch();
        assert_eq!(t.text(), "Book a table.");
        t.scratch();
        assert_eq!(t.text(), "");
        t.scratch();
        assert!(t.is_empty());
    }

    #[test]
    fn a_command_word_inside_a_sentence_is_kept() {
        let mut t = turn("Stop the server on build-box");
        assert_eq!(t.close(), Closed::Kept);
        assert_eq!(t.text(), "Stop the server on build-box");
    }

    #[test]
    fn a_send_at_the_end_is_not_part_of_the_text() {
        let mut t = turn("Buy milk, send.");
        assert!(t.ends_in_send());
        assert_eq!(t.text(), "Buy milk");
        assert_eq!(t.close(), Closed::Command(Command::Send));
        assert_eq!(t.text(), "Buy milk");
    }

    #[test]
    fn nothing_new_closes_nothing() {
        let mut t = Turn::default();
        assert_eq!(t.close(), Closed::Nothing);
        t.hear("   ");
        assert_eq!(t.close(), Closed::Nothing);
    }
}
