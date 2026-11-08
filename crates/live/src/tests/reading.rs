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
