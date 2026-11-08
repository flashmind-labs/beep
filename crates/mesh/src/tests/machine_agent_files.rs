use super::*;

#[test]
fn merging_replaces_the_marked_block_and_keeps_the_rest() {
    let file = format!("# Mine\n\n{START}\nold\n{END}\n\n## After\n");
    assert_eq!(
        merged(Some(&file), "- new\n"),
        format!("# Mine\n\n{START}\n- new\n\n{BEEP_RULES}\n{END}\n\n## After\n")
    );
    assert_eq!(
        merged(Some("# Mine\n"), "rules"),
        format!("# Mine\n\n{START}\nrules\n\n{BEEP_RULES}\n{END}\n")
    );
    assert_eq!(
        merged(None, "rules"),
        format!("{START}\nrules\n\n{BEEP_RULES}\n{END}\n")
    );
    assert_eq!(merged(None, ""), format!("{START}\n{BEEP_RULES}\n{END}\n"));
    assert!(holds(&merged(Some(&file), "x"), "x\n"));
    assert!(!holds(&file, "x"));
}

#[test]
fn the_read_output_names_each_agent_file() {
    let output = format!(
        "noise\n@@ .claude/CLAUDE.md 1 {}\n@@ .codex/AGENTS.md 0\n@@ .gemini/GEMINI.md -\n@@ .other/X.md 1 eA==\n",
        STANDARD.encode("# Rules\n")
    );
    let files = parse_read(&output).unwrap();
    assert_eq!(
        files,
        [
            ReadFile {
                agent: "claude",
                rel: ".claude/CLAUDE.md",
                text: Some("# Rules\n".into()),
            },
            ReadFile {
                agent: "codex",
                rel: ".codex/AGENTS.md",
                text: None,
            },
        ]
    );
    assert!(parse_read("command not found").is_err());
    assert!(parse_read("@@ .claude/CLAUDE.md 1 !!!").is_err());
}

#[test]
fn a_file_over_the_limit_is_refused_not_cut() {
    let full = format!(
        "@@ .claude/CLAUDE.md 1 {}\n",
        STANDARD.encode("x".repeat(MAX_FILE))
    );
    assert!(parse_read(&full).is_ok());
    let over = format!(
        "@@ .claude/CLAUDE.md 1 {}\n",
        STANDARD.encode("x".repeat(MAX_FILE + 1))
    );
    let error = parse_read(&over).expect_err("it is refused");
    assert!(
        error
            .to_string()
            .contains("~/.claude/CLAUDE.md is over 256 KB")
    );
    assert!(read_script().contains(&format!("head -c {}", MAX_FILE + 1)));
}

#[test]
fn the_windows_read_script_lists_every_file_and_reads_past_the_limit() {
    let script = read_script_windows();
    for (_, rel) in AGENT_FILES {
        assert!(script.contains(&format!("'{rel}'")));
    }
    assert!(script.contains(&format!("byte[] {}", MAX_FILE + 1)));
    assert!(script.contains("Join-Path $HOME"));
    assert!(!script.contains("&&"));
}

#[test]
fn the_windows_write_script_writes_bytes_and_skips_files_that_hold_the_text() {
    let held = merged(None, "rules");
    let files = [
        ReadFile {
            agent: "claude",
            rel: ".claude/CLAUDE.md",
            text: Some(held),
        },
        ReadFile {
            agent: "codex",
            rel: ".codex/AGENTS.md",
            text: None,
        },
    ];
    let script = write_script_windows(&files, "rules");
    assert!(!script.contains(".claude/CLAUDE.md"));
    assert!(script.contains("'.codex/AGENTS.md'"));
    assert!(script.contains(&STANDARD.encode(merged(None, "rules"))));
    assert!(script.contains("WriteAllBytes"));
    assert!(script.contains("Move-Item"));
    assert!(script.contains("exit 1"));
    assert!(!script.contains("&&"));
    assert!(write_script_windows(&files[..1], "rules").is_empty());
}

#[test]
fn the_windows_read_output_parses_like_the_posix_one() {
    let output = format!(
        "#< CLIXML\r\n@@ .claude/CLAUDE.md 1 {}\r\n@@ .codex/AGENTS.md 0\r\n@@ .gemini/GEMINI.md -\r\n",
        STANDARD.encode("# Rules\n")
    );
    let files = parse_read(&output).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].text.as_deref(), Some("# Rules\n"));
    assert_eq!(files[1].text, None);
}
