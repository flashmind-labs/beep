//! Spoken commands, found on the live transcript. A command counts only
//! when it is a whole phrase on its own, so "stop the server" is a message
//! and "stop" is a command. "send" may also end the last phrase.

/// What a spoken command asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Send what was said.
    Send,
    /// Stop Beep speaking; while the user talks, send what was said.
    Stop,
}

/// Each command in English, Spanish, Catalan, French, German, Italian and
/// Portuguese, normalized: lowercase, no punctuation, single spaces.
const PHRASES: &[(Command, &[&str])] = &[
    (
        Command::Send,
        &[
            "send",
            "sent",
            "send it",
            "envía",
            "envia",
            "enviar",
            "envíalo",
            "envialo",
            "envoie",
            "envoyer",
            "envoie le",
            "senden",
            "abschicken",
            "schick",
            "schicken",
            "invia",
            "inviare",
            "manda",
            "envie",
        ],
    ),
    (
        Command::Stop,
        &[
            "stop", "para", "detente", "parar", "atura't", "atura", "arrête", "stopp", "halt",
            "basta", "fermati", "pare",
        ],
    ),
];

/// The words that may end a phrase to send it. "sent" is left out: "the
/// email was sent" ends a question, not a message.
const SEND_WORDS: &[&str] = &["send", "envía", "envia", "envoie", "senden", "invia"];

/// `text` lowercased, with punctuation dropped and spaces collapsed. The
/// Catalan middle dot and apostrophes inside words stay.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut last = None;
    while let Some(c) = chars.next() {
        let inner = matches!(c, '·' | '\'' | '’')
            && last.is_some_and(char::is_alphanumeric)
            && chars.peek().is_some_and(|n| n.is_alphanumeric());
        let keep = c.is_alphanumeric() || inner;
        if keep {
            out.extend(if c == '’' { '\'' } else { c }.to_lowercase());
        } else if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        last = Some(c);
    }
    out.trim_end().to_owned()
}

/// The command `phrase` is when it is one, whole.
pub fn phrase_command(phrase: &str) -> Option<Command> {
    let phrase = normalize(phrase);
    if phrase.is_empty() {
        return None;
    }
    PHRASES
        .iter()
        .find(|(_, words)| words.contains(&phrase.as_str()))
        .map(|(command, _)| *command)
}

/// Where a send word that ends `text` starts, when one does.
pub fn send_at_end(text: &str) -> Option<usize> {
    let end = text.trim_end_matches(|c: char| !c.is_alphanumeric());
    let start = end
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let word = normalize(&end[start..]);
    SEND_WORDS.contains(&word.as_str()).then_some(start)
}

/// `text` without the send word at its end and the comma or space that led
/// up to it.
pub fn strip_send(text: &str) -> &str {
    let kept = send_at_end(text).map_or(text, |start| &text[..start]);
    kept.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-'))
}

/// The sentences in `text`, each ending after its `.`, `?`
/// or `!`, the last one wherever the text ends.
pub fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '?' | '!' | '。' | '¿' | '¡') {
            // Opening marks start a sentence; closing ones end it.
            let end = if matches!(c, '¿' | '¡') {
                i
            } else {
                i + c.len_utf8()
            };
            if !text[start..end].trim().is_empty() {
                out.push(&text[start..end]);
                start = end;
            }
        }
    }
    if !text[start..].trim().is_empty() {
        out.push(&text[start..]);
    }
    out
}

#[cfg(test)]
#[path = "tests/command.rs"]
mod tests;
