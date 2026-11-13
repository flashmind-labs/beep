use super::*;

/// A brief as Beep wrote it bare, before the tags.
pub(crate) const BARE_BRIEF: &str = "Beep: the `beep` MCP server's tools reach the user's other machines. remote_run runs a command there. ask_user asks the user a question and waits for the answer. For Rust, C, or C++ builds, check whether sccache is installed. Do not install sccache or change the user's global configuration unless asked.";

#[test]
fn tags_read_back_as_written() {
    let text = format!(
        "{}{}\nok, let's keep going",
        context("Beep: the tools.\n\nBeep: the user's standing rules:\n- Be brief."),
        from_session("a1", "Fix \"the\" build", "Run the tests <now>."),
    );
    assert_eq!(
        parts(&text),
        [
            Part::Context("Beep: the tools.\n\nBeep: the user's standing rules:\n- Be brief."),
            Part::Session {
                id: "a1".into(),
                name: "Fix \"the\" build".into(),
                text: "Run the tests <now>.",
            },
            Part::User("ok, let's keep going"),
        ]
    );
    let again = join(&parts(&text));
    assert_eq!(parts(&again), parts(&text));
}

#[test]
fn a_child_note_reads_back_as_written() {
    let text = format!(
        "{}\nand then",
        from_child(
            "k1@box",
            "Fix <it>",
            "finished",
            "[child k1@box finished] Done."
        )
    );
    let child = Part::Child {
        id: "k1@box".into(),
        name: "Fix <it>".into(),
        state: "finished".into(),
        text: "[child k1@box finished] Done.",
    };
    assert_eq!(parts(&text), [child.clone(), Part::User("and then")]);
    assert_eq!(parts(&join(&parts(&text))), parts(&text));
    assert_eq!(own_words(&text), "and then");
}

#[test]
fn own_words_leave_out_the_context_and_other_sessions() {
    let text = format!(
        "# Review\n\n{}\n### a.rs:3\n\nfix{}",
        context("Beep: the tools."),
        from_session("a1", "Build", "Run it."),
    );
    assert_eq!(own_words(&text), "# Review\n### a.rs:3\n\nfix");
}

#[test]
fn untagged_text_is_the_users() {
    assert_eq!(parts("a <b> c"), [Part::User("a <b> c")]);
    assert_eq!(said("<beep-contextual> hi"), "<beep-contextual> hi");
}

#[test]
fn a_bare_brief_becomes_context() {
    let brief = BARE_BRIEF;
    let kept = cut(brief, MAX_CONTEXT);
    assert_eq!(bare_block(brief), context(brief));
    assert_eq!(bare_block("ok"), "ok");
    let glued = format!("{brief}ok, let's keep going");
    assert_eq!(said(&glued), "ok, let's keep going");
    assert_eq!(
        parts(&untagged(&glued)),
        [
            Part::Context(kept.trim()),
            Part::User("ok, let's keep going")
        ]
    );
    let worktree = format!("{BARE_WORKTREE}\n\n{brief}fix it");
    assert_eq!(said(&worktree), "fix it");
    let old = brief.split(" For Rust, C, or C++ builds").next().unwrap();
    assert_eq!(said(&format!("{old}keep going")), "keep going");
}
