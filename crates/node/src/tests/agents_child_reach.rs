use super::*;

#[test]
fn a_childs_id_names_its_machine() {
    let id = ChildId::from(" a3f9c1@build-box ");
    assert_eq!(
        (id.session, id.machine),
        (SessionId::from("a3f9c1"), Some("build-box"))
    );
    let id = ChildId::from("a3f9c1");
    assert_eq!((id.session, id.machine), (SessionId::from("a3f9c1"), None));
}

#[test]
fn a_start_call_reads_back_as_a_subagent_card_with_a_link_to_the_child() {
    let view = ChildView {
        id: "a3f9c1@mac-box".into(),
        key: "k1".into(),
        harness: "codex".into(),
        machine: "mac-box".into(),
        state: "working".into(),
        title: None,
        last_message: String::new(),
        files_changed: 0,
        question: None,
        turns: 0,
        tokens: 0,
        cost_usd: None,
        error: None,
        note: None,
        project: None,
        branch: None,
        notes_settles: false,
        issue: None,
    };
    let call = format!(
        "Beep: Start codex agent on mac-box: Build the iOS app [completed]\n{}",
        serde_json::to_string(&view).unwrap()
    );
    assert_eq!(started_view(&call), Some(view.clone()));
    let text = child_entry(&call, &view);
    assert_eq!(
        text,
        "Build the iOS app\nBuild the iOS app [in_progress]\n[steps] 0\n[agent] codex on mac-box\n\
         [child] a3f9c1@mac-box working k1"
    );
    let done = ChildView {
        state: "idle".into(),
        turns: 1,
        title: Some("iOS build".into()),
        last_message: "Built and tested.".into(),
        ..view.clone()
    };
    assert_eq!(
        child_entry(&call, &done),
        "iOS build\nBuild the iOS app [completed]\n[steps] 1\n[agent] codex on mac-box\n\
         [child] a3f9c1@mac-box idle k1\nBuilt and tested."
    );
    // What it spent shows on the card, after the link and before its answer.
    let spent = ChildView {
        tokens: 18_400,
        cost_usd: Some(0.0412),
        ..done.clone()
    };
    assert_eq!(
        child_entry(&call, &spent),
        "iOS build\nBuild the iOS app [completed]\n[steps] 1\n[agent] codex on mac-box\n\
         [child] a3f9c1@mac-box idle k1\n[spend] 18400 0.0412\nBuilt and tested."
    );
    let asking = ChildView {
        state: "waiting_on_user".into(),
        question: Some("Run xcodebuild test".into()),
        ..view
    };
    assert!(child_entry(&call, &asking).ends_with("Waiting for you: Run xcodebuild test"));
    let prompt = format!(
        "Fix `login`.\n\n{{\"instruction\": \"keep this\"}}\n{}\nLast instruction.",
        "Keep every line. ".repeat(300)
    );
    let multiline = format!(
        "Beep: Start codex agent: {prompt} [completed]\n{}",
        serde_json::to_string(&done).unwrap()
    );
    assert!(
        child_entry(&multiline, &done)
            .starts_with(&format!("iOS build\n{prompt} [completed]\n[steps]"))
    );
    assert_eq!(
        started_view("Beep: Start codex agent: x [completed]\nnot json"),
        None
    );

    // flsh sends no rawInput on live MCP tool calls, so the call line has no prompt.
    let no_args = format!(
        "Beep: Start an agent [completed]\n{}",
        serde_json::to_string(&ChildView {
            state: "working".into(),
            turns: 0,
            title: None,
            last_message: String::new(),
            ..done.clone()
        })
        .unwrap()
    );
    assert_eq!(
        child_entry(
            &no_args,
            &ChildView {
                state: "working".into(),
                turns: 0,
                title: None,
                last_message: String::new(),
                ..done.clone()
            }
        ),
        "codex agent [in_progress]\n[steps] 0\n[agent] codex on mac-box\n\
         [child] a3f9c1@mac-box working k1"
    );
    assert_eq!(
        child_entry(&no_args, &done),
        "iOS build [completed]\n[steps] 1\n[agent] codex on mac-box\n\
         [child] a3f9c1@mac-box idle k1\nBuilt and tested."
    );
}
