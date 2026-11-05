use super::{
    actions::{MAX_ACTION_VALUE_CHARS, describe_action},
    guard::{READ_ONLY, classify},
    records::*,
    *,
};
use crate::{
    apps::catalog_app,
    protocol::{HistoryEntry, PermissionLevel},
    store::{AppPermissionRow, ChatAllow},
};
use async_trait::async_trait;
use flashmind::{
    core::{Conversation, ConversationEntry, EntryKind},
    types::{
        ToolCall,
        tool::{Tool, ToolContext, ToolResult},
    },
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

#[test]
fn tools_are_classified_by_app() {
    let (app, acts) = lookup("GMAIL_SEND_EMAIL").unwrap();
    assert_eq!((app.slug(), acts), ("gmail", Acts::Always));
    assert_eq!(lookup("GMAIL_FETCH_EMAILS").unwrap().1, Acts::Never);
    let (app, acts) = lookup("FASTMAIL_UPDATE_EMAIL").unwrap();
    assert_eq!((app.slug(), acts), ("fastmail", Acts::WhenMoving));
    assert!(lookup("open_maps").is_none());
    // A tool of a server the user added always asks.
    let (app, acts) = lookup("MCP_MY-NOTES_SEARCH_PAGES").unwrap();
    assert_eq!((app.slug(), acts), ("mcp_my-notes", Acts::Always));
    assert_eq!(app.name(), "My notes");
    assert_eq!(
        action_name(&app, "MCP_MY-NOTES_SEARCH_PAGES"),
        "search pages"
    );
    assert!(call_acts("MCP_MY-NOTES_SEARCH_PAGES", &Value::Null).is_none());
    assert!(lookup("MCP_").is_none());
    let (app, acts) = lookup("code_agent").unwrap();
    assert_eq!((app.slug(), acts), (MACHINES_APP, Acts::WhenDirectingAgent));
    for action in [
        "start",
        "takeover",
        "prompt",
        "answer",
        "steer",
        "interrupt",
    ] {
        assert!(acts.on(&json!({ "action": action })), "{action}");
    }
    for action in [
        "status",
        "sessions",
        "transcript",
        "live",
        "follow",
        "unfollow",
        "cancel",
        "close",
    ] {
        assert!(!acts.on(&json!({ "action": action })), "{action}");
    }
}

#[test]
fn a_chat_allow_runs_without_asking_but_never_past_read_only() {
    let mut rules = Rules::from_rows(vec![AppPermissionRow {
        app: "notion".into(),
        level: PermissionLevel::ReadOnly {},
        allowed: Vec::new(),
    }]);
    let args = Value::Null;
    let gmail = lookup("GMAIL_SEND_EMAIL").unwrap().0;
    assert!(matches!(
        check(&rules, &gmail, "GMAIL_SEND_EMAIL", &args),
        Check::Ask(_)
    ));
    let allow = |app: &str, tool: &str| ChatAllow {
        app: app.into(),
        tool: tool.into(),
    };
    rules.allow_in_chat(vec![
        allow("gmail", "GMAIL_SEND_EMAIL"),
        allow("notion", "NOTION_CREATE_NOTION_PAGE"),
    ]);
    assert!(matches!(
        check(&rules, &gmail, "GMAIL_SEND_EMAIL", &args),
        Check::Run
    ));
    // Only the action allowed.
    assert!(matches!(
        check(&rules, &gmail, "GMAIL_MOVE_TO_TRASH", &args),
        Check::Ask(_)
    ));
    let notion = lookup("NOTION_CREATE_NOTION_PAGE").unwrap().0;
    assert!(matches!(
        check(&rules, &notion, "NOTION_CREATE_NOTION_PAGE", &args),
        Check::Refuse(_)
    ));
}

#[test]
fn the_chat_machine_log_tool_has_no_card_and_no_rule() {
    // Only the Beep MCP server's machine_log asks (through `check` with its own slug).
    assert!(lookup(MACHINE_LOG).is_none());
    assert!(lookup("machine_log").is_none());
}

#[test]
fn only_tools_that_never_act_are_read_tools() {
    let none = HashSet::new();
    assert_eq!(read_tool_app("gmail_fetch_emails", &none), Some("gmail"));
    assert_eq!(read_tool_app("GMAIL_SEND_EMAIL", &none), None);
    assert_eq!(read_tool_app("FASTMAIL_UPDATE_EMAIL", &none), None);
    assert_eq!(read_tool_app("MCP_MY-NOTES_SEARCH_PAGES", &none), None);
    assert_eq!(read_tool_app("open_maps", &none), None);
    let hinted = HashSet::from([ToolSlug::new("GMAIL_FETCH_EMAILS")]);
    assert_eq!(read_tool_app("GMAIL_FETCH_EMAILS", &hinted), None);
}

/// A tool that is only its name.
struct Named(&'static str);

#[async_trait]
impl Tool for Named {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        ""
    }
    fn parameters(&self) -> Value {
        json!({"type": "object"})
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success(ctx.tool_call_id, ""))
    }
}

#[tokio::test]
async fn sending_to_a_machine_asks_first() {
    let (app, acts) = lookup("send_file").unwrap();
    assert_eq!(
        (app.slug(), app.name()),
        (MACHINES_APP, "Your machines".into())
    );
    assert_eq!(acts, Acts::WhenToMachine);
    let guarded = guard(
        vec![Arc::new(Named("send_file"))],
        Rules::default(),
        Approved(None),
        &HashSet::new(),
        None,
    );
    let token = tokio_util::sync::CancellationToken::new();
    let run = |args: Value| {
        let tool = guarded[0].clone();
        let token = token.clone();
        async move {
            tool.execute(ToolContext::new("call", args, None, &token))
                .await
                .unwrap()
        }
    };
    let to_machine = run(json!({ "path": "frames", "to": "laptop" })).await;
    assert!(to_machine.is_interrupt());
    let to_user = run(json!({ "path": "a.pdf", "to": "User" })).await;
    assert!(to_user.is_success() && !to_user.is_interrupt());
    assert!(run(json!({ "path": "a.pdf" })).await.is_success());
    let approval = approval_for(&app, "send_file", &json!({ "to": "laptop" }));
    assert_eq!(approval.action, "send file");
}

#[tokio::test]
async fn starting_a_coding_agent_asks_first_and_checking_on_one_does_not() {
    let token = tokio_util::sync::CancellationToken::new();
    let start = json!({
        "action": "start",
        "agent": "claude",
        "in": "build-box",
        "cwd": "/srv/project",
        "prompt": "Fix the tests",
        "wait_secs": 30,
    });
    let run = |rules: Rules, args: Value| {
        let guarded = guard(
            vec![Arc::new(Named("code_agent"))],
            rules,
            Approved(None),
            &HashSet::new(),
            None,
        );
        let token = token.clone();
        async move {
            guarded[0]
                .execute(ToolContext::new("call", args, None, &token))
                .await
                .unwrap()
        }
    };
    assert!(run(Rules::default(), start.clone()).await.is_interrupt());
    let status = run(Rules::default(), json!({ "action": "status" })).await;
    assert!(status.is_success() && !status.is_interrupt());
    // Read only refuses a start and keeps the checks.
    let read_only = || {
        Rules::from_rows(vec![AppPermissionRow {
            app: MACHINES_APP.into(),
            level: PermissionLevel::ReadOnly {},
            allowed: Vec::new(),
        }])
    };
    let refused = run(read_only(), start.clone()).await;
    assert!(
        refused.output().starts_with(READ_ONLY),
        "{}",
        refused.output()
    );
    let status = run(read_only(), json!({ "action": "status" })).await;
    assert!(status.is_success() && !status.output().starts_with(READ_ONLY));

    let approval = approval_for(&ToolApp::Machines, "code_agent", &start);
    let shown: Vec<(&str, &str)> = approval
        .fields
        .iter()
        .map(|field| (field.label.as_str(), field.value.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Action", "Start"),
            ("Agent", "Claude Code"),
            ("Machine", "build-box"),
            ("Folder", "/srv/project"),
            ("Prompt", "Fix the tests"),
        ]
    );
    // What a call leaves out still has its line.
    let approval = approval_for(
        &ToolApp::Machines,
        "code_agent",
        &json!({ "action": "start", "prompt": "Fix it" }),
    );
    let default: Vec<&str> = approval
        .fields
        .iter()
        .filter(|field| field.value == "Default")
        .map(|field| field.label.as_str())
        .collect();
    assert_eq!(default, ["Agent", "Machine", "Folder"]);
}

#[tokio::test]
async fn making_media_asks_first_with_its_service_and_model() {
    let token = tokio_util::sync::CancellationToken::new();
    let args = json!({ "prompt": "a red fox", "provider": "openai" });
    for tool in ["image_gen", "video_gen", "3d_gen"] {
        let (app, acts) = lookup(tool).unwrap();
        assert_eq!(app.slug(), MEDIA_APP);
        assert_eq!(acts, Acts::Always);
        let guarded = guard(
            vec![Arc::new(Named(tool))],
            Rules::default(),
            Approved(None),
            &HashSet::new(),
            None,
        );
        let result = guarded[0]
            .execute(ToolContext::new("call", args.clone(), None, &token))
            .await
            .unwrap();
        assert!(result.is_interrupt(), "{tool}");
        // An untrusted turn never gets it.
        assert!(read_tools_only(vec![Arc::new(Named(tool))], &HashSet::new()).is_empty());
    }
    let approval = approval_for(&ToolApp::Media, "image_gen", &args);
    assert_eq!(approval.app, "Media generation");
    assert_eq!(approval.action, "make an image");
    let shown: Vec<(&str, &str)> = approval
        .fields
        .iter()
        .map(|field| (field.label.as_str(), field.value.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Prompt", "a red fox"),
            ("Provider", "OpenAI"),
            ("Model", "gpt-image-2")
        ]
    );
    // The user's yes runs it.
    let guarded = guard(
        vec![Arc::new(Named("image_gen"))],
        Rules::default(),
        Approved(Some("call".into())),
        &HashSet::new(),
        None,
    );
    let result = guarded[0]
        .execute(ToolContext::new("call", args, None, &token))
        .await
        .unwrap();
    assert!(!result.is_interrupt());
}

#[test]
fn an_untrusted_turn_keeps_only_read_tools() {
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(Named("GMAIL_FETCH_EMAILS")),
        Arc::new(Named("GMAIL_SEND_EMAIL")),
        Arc::new(Named("FASTMAIL_UPDATE_EMAIL")),
        Arc::new(Named("MCP_MY-NOTES_SEARCH_PAGES")),
    ];
    let kept: Vec<String> = read_tools_only(tools, &HashSet::new())
        .iter()
        .map(|tool| tool.name().to_owned())
        .collect();
    assert_eq!(kept, ["GMAIL_FETCH_EMAILS"]);
    assert!(crate::routines::wake::Rule::Accounts.untrusted());
    assert!(crate::routines::wake::Rule::Digest.untrusted());
    assert!(crate::routines::wake::Rule::Triaged.untrusted());
    assert!(!crate::routines::wake::Rule::Place.untrusted());
}

#[test]
fn project_tools_are_known_and_can_be_allowed_always() {
    let (app, acts) = lookup("project_plan").unwrap();
    assert_eq!((app.slug(), acts), ("project", Acts::Never));
    assert_eq!(app.name(), "Project");
    assert!(lookup("PROJECT_WORKER").is_some());
    assert!(lookup("PROJECT_OTHER").is_none());
    let saved = row(
        &PROJECT_APP,
        PermissionLevel::Ask {},
        vec![
            "PROJECT_PLAN".into(),
            "PROJECT_TASKS_DROP".into(),
            "PROJECT_TASKS".into(),
            "PROJECT_SETUP".into(),
            "EXEC".into(),
            "GMAIL_SEND_EMAIL".into(),
        ],
    );
    assert_eq!(
        saved.allowed,
        ["EXEC", "PROJECT_PLAN", "PROJECT_TASKS_DROP"]
    );
}

#[test]
fn hints_add_asking() {
    assert!(hinted_to_act(&[
        "openWorldHint".into(),
        "createHint".into()
    ]));
    assert!(hinted_to_act(&["destructiveHint".into()]));
    assert!(!hinted_to_act(&[
        "updateHint".into(),
        "openWorldHint".into()
    ]));
    let hinted: HashSet<ToolSlug> = [ToolSlug::new("GMAIL_FETCH_EMAILS")].into();
    assert_eq!(
        classify("gmail_fetch_emails", &hinted).unwrap().1,
        Acts::Always
    );
    // A read the catalog checked stays a read whatever its tags say.
    let hinted: HashSet<ToolSlug> = [ToolSlug::new("SLACK_OPEN_DM")].into();
    assert_eq!(classify("slack_open_dm", &hinted).unwrap().1, Acts::Never);
    // An update-only catalog action keeps asking without a hint.
    assert_eq!(
        classify("gmail_move_to_trash", &HashSet::new()).unwrap().1,
        Acts::Always
    );
}

#[test]
fn moving_mail_acts() {
    let update = Acts::WhenMoving;
    assert!(!update.on(&json!({"ids": ["M1"], "isRead": true})));
    assert!(update.on(&json!({"ids": ["M1"], "folder": "Trash"})));
}

#[test]
fn closing_open_calls_gives_each_a_result() {
    let call = |id: &str| ToolCall {
        id: id.into(),
        name: "GMAIL_SEND_EMAIL".into(),
        arguments: json!({}),
    };
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::user("hi"));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![call("a"), call("b")],
        None,
    ));
    conversation.add(ConversationEntry::tool("a", "sent"));
    close_open_calls(&mut conversation, SUPERSEDED);
    assert!(open_calls(&conversation).is_empty());
    // Sanitizing for the model keeps the call now that it has a result.
    conversation.sanitize();
    let kept = conversation.entries().iter().any(|entry| {
        matches!(&entry.kind, EntryKind::Tool { call_id, output, .. } if call_id == "b" && output == SUPERSEDED)
    });
    assert!(kept);
}

#[test]
fn a_session_stopped_on_a_call_shows_its_card() {
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::user("Send the email"));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![ToolCall {
            id: "send".into(),
            name: "gmail_send_email".into(),
            arguments: json!({"recipient_email": "a@example.com", "subject": "Stay"}),
        }],
        None,
    ));
    let Some(HistoryEntry::Approval { query_id, approval }) =
        waiting_approval(&conversation, false)
    else {
        panic!("expected the open call's card");
    };
    assert_eq!(query_id, "send");
    assert_eq!(
        (approval.app.as_str(), approval.action.as_str()),
        ("Gmail", "send email")
    );
    assert_eq!(approval.fields.len(), 2);
    conversation.add(ConversationEntry::tool("send", DENIED));
    assert!(waiting_approval(&conversation, false).is_none());
}

#[test]
fn only_a_project_chat_shows_a_parked_exec_as_project() {
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::user("Run the tests"));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![ToolCall {
            id: "run".into(),
            name: "exec".into(),
            arguments: json!({"command": "cargo test"}),
        }],
        None,
    ));
    assert!(waiting_approval(&conversation, false).is_none());
    let Some(HistoryEntry::Approval { approval, .. }) = waiting_approval(&conversation, true)
    else {
        panic!("expected the project card");
    };
    assert_eq!(approval.app, "Project");
    assert!(lookup("exec").is_none());
    assert!(lookup("process").is_none());
    assert_eq!(lookup_in("exec", true).unwrap().0.slug(), "project");
    assert_eq!(
        lookup_in("project_plan", false).unwrap().0.slug(),
        "project"
    );
}

#[test]
fn a_card_says_which_email_an_id_names() {
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![ToolCall {
            id: "find".into(),
            name: "gmail_fetch_emails".into(),
            arguments: json!({"query": "in:inbox"}),
        }],
        None,
    ));
    let found = json!({"data": {"messages": [
        {"messageId": "m1", "subject": "Renew example.com", "sender": "Registrar <billing@example.com>"},
        {"messageId": "m2", "subject": "New listings", "sender": "alerts@example.com"},
    ]}});
    conversation.add(ConversationEntry::tool("find", found.to_string()));
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![ToolCall {
            id: "trash".into(),
            name: "gmail_move_to_trash".into(),
            arguments: json!({"message_id": "m2"}),
        }],
        None,
    ));
    let Some(HistoryEntry::Approval { approval, .. }) = waiting_approval(&conversation, false)
    else {
        panic!("expected the open call's card");
    };
    let shown: Vec<_> = approval
        .fields
        .iter()
        .map(|f| (f.label.as_str(), f.value.as_str()))
        .collect();
    assert_eq!(
        shown,
        [("Subject", "New listings"), ("From", "alerts@example.com")]
    );
    assert_eq!(describe_records("trash", &conversation).len(), 2);
}

#[test]
fn a_read_takes_the_ids_a_call_names() {
    let lookup = |tool| LOOKUPS.iter().find(|l| l.tools.contains(&tool)).unwrap();
    let reads = read_args(
        lookup("FASTMAIL_DELETE_EMAIL"),
        &json!({"ids": ["M1", "", "M2"]}),
    );
    assert_eq!(
        reads,
        [json!({"id": "M1"}), json!({"id": "M2"})].map(|v| v.as_object().unwrap().clone())
    );
    let reads = read_args(
        lookup("GOOGLECALENDAR_DELETE_EVENT"),
        &json!({"event_id": "e1", "calendar_id": "primary", "send_updates": "all"}),
    );
    assert_eq!(
        Value::Object(reads[0].clone()),
        json!({"event_id": "e1", "calendar_id": "primary"})
    );
    let reads = read_args(
        lookup("ONE_DRIVE_MOVE_ITEM"),
        &json!({"itemId": "i1", "driveId": "d1", "parentReference": {"id": "p"}}),
    );
    assert_eq!(
        Value::Object(reads[0].clone()),
        json!({"item_id": "i1", "drive_id": "d1"})
    );
    assert!(read_args(lookup("NOTION_UPDATE_PAGE"), &json!({})).is_empty());
    // Every tool listed is one that asks, so a renamed slug fails here.
    for tool in LOOKUPS.iter().flat_map(|l| l.tools) {
        let acts = call_acts(tool, &json!({"folder": "x"})).is_some_and(|(_, acts)| acts);
        assert!(acts, "{tool} is not an acting tool");
    }
}

#[test]
fn values_at_the_limit_are_not_marked_cut() {
    let exact = "a".repeat(MAX_VALUE_CHARS);
    assert_eq!(fields(&json!({ "body": exact }))[0].value, exact);
    let long = "é".repeat(MAX_VALUE_CHARS + 1);
    let shown = &fields(&json!({ "body": long }))[0].value;
    assert_eq!(shown.chars().count(), MAX_VALUE_CHARS + 3);
    assert!(shown.ends_with("é..."));
    let gmail = catalog_app("gmail").expect("gmail is in the catalog");
    let call = ToolCall {
        id: "send".into(),
        name: "GMAIL_SEND_EMAIL".into(),
        arguments: json!({ "subject": "s".repeat(MAX_ACTION_VALUE_CHARS) }),
    };
    let action = describe_action(&ToolApp::Catalog(gmail), &call, &Conversation::new());
    assert!(!action.contains("..."), "{action}");
}

#[test]
fn actions_are_the_calls_that_acted_and_went_through() {
    let call = |id: &str, name: &str, arguments| ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    };
    let mut conversation = Conversation::new();
    conversation.add(ConversationEntry::user("Email Jo about lunch"));
    let since = conversation.entries()[0].timestamp;
    let email = json!({"recipient_email": "jo@example.com", "subject": "Lunch", "body": "See you at noon."});
    conversation.add(ConversationEntry::assistant_with_tool_calls(
        "",
        vec![
            call("read", "GMAIL_FETCH_EMAILS", json!({})),
            call("sent", "GMAIL_SEND_EMAIL", email.clone()),
            call("denied", "GMAIL_SEND_EMAIL", email.clone()),
            call("read_only", "GMAIL_SEND_EMAIL", email.clone()),
            call("failed", "GMAIL_SEND_EMAIL", email),
        ],
        None,
    ));
    conversation.add(ConversationEntry::tool("read", "[]"));
    conversation.add(ConversationEntry::tool("sent", r#"{"successful": true}"#));
    conversation.add(ConversationEntry::tool("denied", DENIED));
    conversation.add(ConversationEntry::tool(
        "read_only",
        format!("{READ_ONLY} Gmail to read only."),
    ));
    conversation.add(ConversationEntry::tool(
        "failed",
        r#"{"successful": false, "error": "quota"}"#,
    ));
    let actions = actions_since(&conversation, since);
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert!(actions[0].starts_with("Gmail: "), "{}", actions[0]);
    assert!(actions[0].contains("jo@example.com"), "{}", actions[0]);
    assert!(actions[0].contains("Lunch"), "{}", actions[0]);
}
