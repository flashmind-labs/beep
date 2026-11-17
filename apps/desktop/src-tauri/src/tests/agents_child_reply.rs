use super::*;

#[test]
fn a_child_reply_renders_markdown_without_its_envelope() {
    let text = "<beep-child id=\"k1@build-box\" name=\"Run tests\" state=\"finished\">\n[child k1@build-box finished] ## Result\n\n**Passed** `cargo test`.\n\n- One\n- Two\n\n| Suite | Passed |\n|---|---:|\n| Core | 12 |\n\n```rust\nlet count = 12;\n```\n\n[Log](https://example.com/log)\nReply with send_to_agent to continue it.\n</beep-child>";
    let html = replies(text).pop().unwrap();
    #[cfg(any(mobile, feature = "rich-text"))]
    for expected in [
        "<h2>Result</h2>",
        "<strong>Passed</strong>",
        "<code>cargo test</code>",
        "<ul>",
        "<table>",
        "<pre><code>",
        "https://example.com/log",
    ] {
        assert!(html.contains(expected), "missing {expected}: {html}");
    }
    #[cfg(not(any(mobile, feature = "rich-text")))]
    {
        assert!(
            html.starts_with("<div class=\"plain-reply\">## Result\n"),
            "{html}"
        );
        for expected in [
            "**Passed** `cargo test`.",
            "- One\n- Two",
            "| Core | 12 |",
            "```rust\nlet count = 12;",
            "[Log](https://example.com/log)",
        ] {
            assert!(html.contains(expected), "missing {expected}: {html}");
        }
        assert!(!html.contains("<strong>"), "{html}");
    }
    assert!(!html.contains("[child"));
    assert!(!html.contains("send_to_agent"));
}

#[test]
fn all_notice_states_hide_only_the_generated_prefix_and_footer() {
    for (state, status, footer) in [
        (
            "asking",
            "is waiting on the user",
            "Only the user can answer this.",
        ),
        (
            "stopped",
            "was stopped by the user",
            "Reply with send_to_agent to continue it.",
        ),
        (
            "finished",
            "finished",
            "Reply with agent send to continue it.",
        ),
        ("closed", "was stopped by the user", "Its session ended."),
    ] {
        let text = format!("[child k1@build-box {status}] Reply.\n{footer}");
        assert_eq!(body("k1@build-box", state, &text), "Reply.");
    }
}

#[test]
fn code_and_quoted_instructions_survive_and_html_does_not_run() {
    let text = "<beep-child id=\"k1\" state=\"finished\">\n[child k1 finished] ```text\n[child k1 finished]\nReply with send_to_agent to continue it.\n```\n\n<script>alert(1)</script>\n\n![image](https://example.com/image.png)\nReply with send_to_agent to continue it.\n</beep-child>";
    let html = replies(text).pop().unwrap();
    assert!(html.contains("[child k1 finished]"));
    assert!(html.contains("Reply with send_to_agent to continue it."));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<img"));
    assert_eq!(
        body("k2", "finished", "[child k1 finished] Keep it."),
        "[child k1 finished] Keep it."
    );
}

#[test]
fn child_parts_keep_their_order_among_other_prompt_parts() {
    let text = "User\n<beep-context>Context</beep-context>\n<beep-child id=\"k1\" state=\"finished\">First</beep-child>\n<from-session id=\"s1\">Other</from-session>\n<beep-child id=\"k2\" state=\"finished\">Second</beep-child>";
    #[cfg(any(mobile, feature = "rich-text"))]
    assert_eq!(replies(text), ["<p>First</p>\n", "<p>Second</p>\n"]);
    #[cfg(not(any(mobile, feature = "rich-text")))]
    assert_eq!(
        replies(text),
        [
            "<div class=\"plain-reply\">First</div>",
            "<div class=\"plain-reply\">Second</div>"
        ]
    );
}

#[test]
fn an_indented_code_block_keeps_its_indentation() {
    let text = "[child k1 finished]     let count = 12;\nReply with send_to_agent to continue it.";
    let html = markdown::untrusted_html(body("k1", "finished", text));
    #[cfg(any(mobile, feature = "rich-text"))]
    assert!(html.starts_with("<pre><code>let count = 12;"), "{html}");
    #[cfg(not(any(mobile, feature = "rich-text")))]
    assert_eq!(html, "<div class=\"plain-reply\">    let count = 12;</div>");
}
