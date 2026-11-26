use super::*;
use beep_mesh::MeshError;
use iroh::endpoint::WriteError;
use std::io;

#[test]
fn image_lines_stay_out_of_the_agents_html() {
    let said = EntryRow::from(AgentEntry {
        tool: None,
        sent_at: None,
        kind: AgentEntryKind::Agent {},
        text: "Saved chart.png\n[image: image-0123456789abcdef]".into(),
    });
    let html = said.html.unwrap();
    assert!(html.contains("Saved chart.png"), "{html}");
    assert!(!html.contains("image-0123"), "{html}");
    assert!(said.text.ends_with("[image: image-0123456789abcdef]"));
}

#[test]
fn what_the_agent_says_comes_as_clean_html() {
    let said = EntryRow::from(AgentEntry {
        tool: None,
        sent_at: None,
        kind: AgentEntryKind::Agent {},
        text: "See [sqlx #19](https://example.com/19) and `cargo test`.<script>x()</script>".into(),
    });
    let html = said.html.expect("an agent entry carries its HTML");
    {
        assert!(
            html.contains(r#"<a href="https://example.com/19""#),
            "{html}"
        );
        assert!(html.contains("<code>cargo test</code>"), "{html}");
    }
    assert!(!html.contains("<script"), "{html}");
    let tool = EntryRow::from(AgentEntry {
        tool: None,
        sent_at: None,
        kind: AgentEntryKind::Tool {},
        text: "Read file [completed]".into(),
    });
    assert_eq!(tool.html, None);
}

#[test]
fn what_the_user_wrote_comes_as_markdown_without_beeps_context() {
    let said = EntryRow::from(AgentEntry {
            tool: None,
        sent_at: None,
            kind: AgentEntryKind::User {},
            text: "<beep-context>\nBeep: the tools.\n</beep-context>\n# Review on ~/app\n\n<b>hi</b>\n[image: image-0123456789abcdef]".into(),
        });
    let html = said.html.expect("a user entry carries its HTML");
    assert!(html.contains("<h1>Review on ~/app</h1>"), "{html}");
    assert!(html.contains("&lt;b&gt;hi&lt;/b&gt;"), "{html}");
    assert!(!html.contains("Beep: the tools"), "{html}");
    assert!(!html.contains("image-0123"), "{html}");
}

#[test]
fn a_thought_comes_as_html_with_its_time() {
    let thought = EntryRow::from(AgentEntry {
        tool: None,
        sent_at: None,
        kind: AgentEntryKind::Thought { ms: Some(12_000) },
        text: "**Reading** the `tests`".into(),
    });
    assert_eq!(thought.kind, "thought");
    assert_eq!(thought.ms, Some(12_000));
    let html = thought.html.expect("a thought carries its HTML");
    assert!(html.contains("<strong>Reading</strong>"), "{html}");
}

#[test]
fn child_replies_have_their_own_html_without_changing_the_agents_prompt() {
    let text = "<beep-child id=\"k1\" state=\"finished\">\n[child k1 finished] **Passed**.\nReply with send_to_agent to continue it.\n</beep-child>\nThanks.";
    let row = EntryRow::from(AgentEntry {
        tool: None,
        sent_at: None,
        kind: AgentEntryKind::User {},
        text: text.into(),
    });
    assert_eq!(row.text, text);
    {
        assert_eq!(row.child_html, ["<p><strong>Passed</strong>.</p>\n"]);
        assert_eq!(row.html.as_deref(), Some("<p>Thanks.</p>\n"));
    }
}

#[test]
fn a_dropped_link_reads_as_lost_and_a_missing_session_as_gone() {
    let stopped = io::Error::from(WriteError::Stopped(0u32.into()));
    let dropped = DesktopError::Mesh(MeshError::Link(stopped.into()));
    assert_eq!(ViewError::from(dropped), ViewError::Lost);
    let offline = DesktopError::Offline("build-box".into());
    assert_eq!(ViewError::from(offline), ViewError::Lost);
    let gone = DesktopError::Api(ApiError::NotFound {});
    assert_eq!(ViewError::from(gone), ViewError::Gone);
    let said = DesktopError::BadRequest("no such agent".into());
    assert_eq!(
        ViewError::from(said),
        ViewError::Failed {
            message: "no such agent".into()
        }
    );
}

#[test]
fn machine_matching_normalizes_display_names_and_accepts_ids() {
    assert_eq!(machine_slug("Build Box's Mac"), "build-boxs-mac");
    assert!(machine_matches("build-box", "ep-1", "Build Box"));
    assert!(machine_matches("ep-1", "ep-1", "Build Box"));
    assert!(!machine_matches("gpu-box", "ep-1", "Build Box"));
    assert!(machine_matches("", "ep-1", "Build Box"));
}
