use super::*;

#[test]
fn a_whole_phrase_is_a_command() {
    assert_eq!(phrase_command("  STOP! "), Some(Command::Stop));
    assert_eq!(phrase_command("Send."), Some(Command::Send));
    assert_eq!(phrase_command("Send it"), Some(Command::Send));
    // Only those two: the rest are messages now.
    assert_eq!(phrase_command("Cancel."), None);
    assert_eq!(phrase_command("Scratch that"), None);
    assert_eq!(phrase_command("Bye"), None);
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
    assert_eq!(phrase_command("Para."), Some(Command::Stop));
    assert_eq!(phrase_command("Arrête !"), Some(Command::Stop));
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
