//! Spoken commands, found on the live transcript. A command counts only
//! when it is a whole phrase on its own, so "stop the server" is a message
//! and "stop" is a command. "send" may also end the last phrase.

/// What a spoken command asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Send what was said.
    Send,
    /// Drop what was said and keep listening.
    Cancel,
    /// Remove the last sentence.
    ScratchThat,
    /// Stop Beep speaking.
    Stop,
    /// Read the last reply again.
    Repeat,
    /// The next message starts a new chat.
    NewChat,
    /// Leave live mode.
    Leave,
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
        Command::Cancel,
        &[
            "cancel",
            "cancela",
            "cancelar",
            "cancel·la",
            "cancella",
            "annule",
            "annuler",
            "abbrechen",
            "annulla",
            "cancele",
            "never mind",
        ],
    ),
    (
        Command::ScratchThat,
        &[
            "scratch that",
            "delete that",
            "borra eso",
            "borra esto",
            "esborra això",
            "esborra",
            "efface ça",
            "efface",
            "lösch das",
            "streich das",
            "cancella quello",
            "apaga isso",
            "apaga",
        ],
    ),
    (
        Command::Stop,
        &[
            "stop", "para", "detente", "parar", "atura't", "atura", "arrête", "stopp", "halt",
            "basta", "fermati", "pare",
        ],
    ),
    (
        Command::Repeat,
        &[
            "repeat that",
            "repeat",
            "say that again",
            "repite",
            "repítelo",
            "repitelo",
            "repeteix",
            "répète",
            "repete",
            "wiederhole",
            "wiederholen",
            "ripeti",
            "repita",
        ],
    ),
    (
        Command::NewChat,
        &[
            "new chat",
            "nuevo chat",
            "xat nou",
            "nou xat",
            "nouveau chat",
            "neuer chat",
            "nuova chat",
            "novo chat",
        ],
    ),
    (
        Command::Leave,
        &[
            "stop listening",
            "bye",
            "bye bye",
            "goodbye",
            "adiós",
            "adios",
            "adéu",
            "adeu",
            "au revoir",
            "tschüss",
            "tschüs",
            "ciao",
            "tchau",
            "deja de escuchar",
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
mod tests {
    use super::*;

    #[test]
    fn a_whole_phrase_is_a_command() {
        assert_eq!(phrase_command("Cancel."), Some(Command::Cancel));
        assert_eq!(
            phrase_command("  scratch THAT! "),
            Some(Command::ScratchThat)
        );
        assert_eq!(phrase_command("Stop"), Some(Command::Stop));
        assert_eq!(phrase_command("Repeat that?"), Some(Command::Repeat));
        assert_eq!(phrase_command("New chat."), Some(Command::NewChat));
        assert_eq!(phrase_command("Stop listening."), Some(Command::Leave));
        assert_eq!(phrase_command("Bye-bye!"), Some(Command::Leave));
        assert_eq!(phrase_command("Send."), Some(Command::Send));
    }

    #[test]
    fn sent_is_heard_as_send_when_alone() {
        assert_eq!(phrase_command("Sent."), Some(Command::Send));
        assert_eq!(send_at_end("The email was sent."), None);
    }

    #[test]
    fn a_command_inside_a_sentence_is_a_word() {
        assert_eq!(phrase_command("Stop the server on build-box"), None);
        assert_eq!(phrase_command("cancel my meeting tomorrow"), None);
        assert_eq!(phrase_command("I said stop"), None);
        assert_eq!(phrase_command("send the email to Ana"), None);
        assert_eq!(phrase_command(""), None);
        assert_eq!(phrase_command(" ... "), None);
    }

    #[test]
    fn other_languages() {
        assert_eq!(phrase_command("¡Envía!"), Some(Command::Send));
        assert_eq!(phrase_command("Cancela."), Some(Command::Cancel));
        assert_eq!(phrase_command("Cancel·la"), Some(Command::Cancel));
        assert_eq!(phrase_command("Para."), Some(Command::Stop));
        assert_eq!(phrase_command("Arrête !"), Some(Command::Stop));
        assert_eq!(phrase_command("Nuevo chat"), Some(Command::NewChat));
        assert_eq!(phrase_command("Tschüss"), Some(Command::Leave));
        assert_eq!(phrase_command("Para mañana a las nueve"), None);
    }

    #[test]
    fn send_may_end_the_last_phrase() {
        assert_eq!(send_at_end("Buy milk send"), Some(9));
        assert_eq!(send_at_end("Buy milk, send."), Some(10));
        assert_eq!(send_at_end("buy milk SEND"), Some(9));
        assert_eq!(send_at_end("Compra leche, envía."), Some(14));
        assert_eq!(send_at_end("please send it"), None);
        assert_eq!(send_at_end("I need to resend"), None);
        assert_eq!(send_at_end("sender"), None);
        assert_eq!(send_at_end(""), None);
    }

    #[test]
    fn the_send_word_is_stripped() {
        assert_eq!(strip_send("Buy milk send"), "Buy milk");
        assert_eq!(strip_send("Buy milk, send."), "Buy milk");
        assert_eq!(
            strip_send("What is the weather? Send."),
            "What is the weather?"
        );
        assert_eq!(strip_send("Send"), "");
        assert_eq!(strip_send("Buy milk."), "Buy milk.");
    }

    #[test]
    fn text_splits_into_sentences() {
        assert_eq!(
            sentences("Book a table. Make it at eight! Thanks"),
            vec!["Book a table.", " Make it at eight!", " Thanks"]
        );
        assert_eq!(sentences("One sentence"), vec!["One sentence"]);
        assert_eq!(sentences("¿Qué tal? Bien."), vec!["¿Qué tal?", " Bien."]);
        assert!(sentences("  ").is_empty());
    }

    #[test]
    fn normalize_keeps_words_and_drops_marks() {
        assert_eq!(normalize("  Hello,   WORLD! "), "hello world");
        assert_eq!(normalize("Cancel·la."), "cancel·la");
        assert_eq!(normalize("Atura’t"), "atura't");
        assert_eq!(normalize("Bye-bye"), "bye bye");
    }
}
