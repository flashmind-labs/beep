use beep_proto::SessionId;
use super::*;
use crate::tools::ProjectTools;
use crate::tools::Node;
use std::sync::Arc;

fn caller(table: Table) -> Caller {
    Caller {
        table,
        from: None,
        desktop_here: false,
        commands_off: None,
        transfers: None,
        agents: None,
        project_tools: None,
        team: None,
        mesh: None,
    }
}

fn run_on() -> Request {
    Request {
        session: SessionId::from("a3f9c1").to_owned(),
        tool: Tool::RunOn,
        args: r#"{"command":"echo ran-here"}"#.to_owned(),
        approved: Default::default(),
    }
}

#[tokio::test]
async fn an_older_nodes_run_on_with_a_prompt_starts_no_agent_here() {
    let table = Table::Peer(iroh::SecretKey::generate().public());
    for args in [
        r#"{"prompt":"fix it"}"#,
        r#"{"prompt":"fix it","agent":"codex"}"#,
        r#"{"command":"echo ran-here","prompt":"fix it"}"#,
    ] {
        let request = Request {
            args: args.to_owned(),
            ..run_on()
        };
        let Reply::Done { text, failed } = answer(request, caller(table)).await else {
            panic!("not done");
        };
        assert!(failed && text.contains("start_agent"), "{args}: {text}");
        assert!(!text.contains("ran-here"));
    }
}

#[tokio::test]
async fn a_nodes_file_and_sync_requests_reach_the_transfer_handlers() {
    let table = Table::Peer(iroh::SecretKey::generate().public());
    let receiving = caller(table);
    let send = Request {
        session: SessionId::from("a3f9c1").to_owned(),
        tool: Tool::SendFiles,
        args: "{}".to_owned(),
        approved: Default::default(),
    };
    let Reply::Done { text, failed } = answer(send, receiving.clone()).await else {
        panic!("not done");
    };
    assert!(failed && text.contains("file transfers did not start"), "{text}");
    let sync = Request {
        session: SessionId::from("a3f9c1").to_owned(),
        tool: Tool::SyncProject,
        args: r#"{"project":"game"}"#.to_owned(),
        approved: Default::default(),
    };
    let Reply::Done { text, failed } = answer(sync, receiving).await else {
        panic!("not done");
    };
    assert!(failed && text.contains("Sync did not start"), "{text}");
}

#[tokio::test]
async fn no_node_serves_its_log_to_another() {
    let table = Table::Peer(iroh::SecretKey::generate().public());
    let caller = caller(table);
    let request = Request {
        session: SessionId::from("a3f9c1").to_owned(),
        tool: Tool::MachineLog,
        args: r#"{"lines":5}"#.to_owned(),
        approved: Default::default(),
    };
    let Reply::Done { text, failed } = answer(request, caller).await else {
        panic!("not done");
    };
    assert!(
        failed && text.contains("does not serve machine_log"),
        "{text}"
    );
}

#[tokio::test]
async fn a_node_runs_a_peers_command_when_it_shares_the_machine() {
    let table = Table::Peer(iroh::SecretKey::generate().public());
    let Reply::Done { text, failed } = answer(run_on(), caller(table)).await else {
        panic!("not done");
    };
    assert!(!failed && text.contains("ran-here"), "{text}");
    let off = Caller {
        commands_off: Some(
            "mac doesn't run commands: Let Beep use this computer is off in its Settings.".into(),
        ),
        ..caller(table)
    };
    let Reply::Done { text, failed } = answer(run_on(), off).await else {
        panic!("not done");
    };
    assert!(
        failed && text.contains("Let Beep use this computer is off"),
        "{text}"
    );
    assert!(!text.contains("ran-here") && !text.contains("offline"));
}

#[tokio::test]
async fn background_run_stops_at_its_timeout() {
    use beep_proto::node::process::TaskState;

    let table = Table::Peer(iroh::SecretKey::generate().public());
    let mut changes = crate::events::system().subscribe();
    let command = if cfg!(windows) {
        "Write-Output ready; Start-Sleep -Seconds 300"
    } else {
        "sleep 300 & echo $!; wait"
    };
    let request = Request {
        args: serde_json::json!({"command": command, "background": true, "timeout_secs": 1})
            .to_string(),
        ..run_on()
    };
    let Reply::Done { text, failed } = answer(request, caller(table)).await else {
        panic!("no start reply");
    };
    assert!(!failed, "{text}");
    let id = text
        .split("Job id: ")
        .nth(1)
        .unwrap()
        .split('.')
        .next()
        .unwrap()
        .parse()
        .map(TaskId)
        .unwrap();
    let ended = timeout(Duration::from_secs(15), async {
        loop {
            let status = tasks::answer(table, TaskRequest::Status { id }).await;
            if let TaskReply::Status { task, lines, .. } = status
                && !task.state.is_running()
            {
                return (task.state, lines);
            }

            // Subscribe before starting, so no state change can be missed.
            let _ = changes.recv().await;
        }
    })
    .await;

    if ended.is_err() {
        tasks::answer(table, TaskRequest::Stop { id }).await;
    }

    let (state, lines) = ended.expect("the background job honors its one-second timeout");
    assert_eq!(state, TaskState::Stopped);
    assert!(!lines.is_empty(), "the command actually started");

    #[cfg(target_os = "linux")]
    {
        let pid: u32 = lines[0].parse().unwrap();
        timeout(Duration::from_secs(5), async {
            loop {
                let stat = tokio::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .await
                    .unwrap_or_default();
                if !stat
                    .rsplit_once(") ")
                    .is_some_and(|(_, rest)| !rest.starts_with('Z'))
                {
                    break;
                }

                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the timeout also stops the child process");
    }
}

#[tokio::test]
async fn peer_requests_keep_accepting_extra_fields() {
    // Legacy peers include routing fields. They are already validated at
    // their own MCP boundary and must not be rejected by this receiver.
    let table = Table::Peer(iroh::SecretKey::generate().public());
    let request = Request {
        args: serde_json::json!({"command":"echo compatible", "machine":"build-box", "future_field":true}).to_string(),
        ..run_on()
    };
    let Reply::Done { text, failed } = answer(request, caller(table)).await else {
        panic!("no reply");
    };
    assert!(!failed && text.contains("compatible"), "{text}");
    assert!(parse::<JobArgs>(serde_json::json!({"id":"1", "future_field":true})).is_ok());
    assert!(
        parse::<SyncArgs>(serde_json::json!({"project":"example", "future_field":true})).is_ok()
    );
}

#[tokio::test]
async fn a_node_gets_a_projects_answer_from_a_desktop_and_a_refusal_otherwise() {
    struct Projects;

    impl ProjectTools for Projects {
        fn call(&self, _args: String) -> futures::future::BoxFuture<'_, Reply> {
            Box::pin(async {
                Reply::Done {
                    text: "no hosts".to_owned(),
                    failed: false,
                }
            })
        }
    }

    let table = Table::Peer(iroh::SecretKey::generate().public());
    let request = Request {
        tool: Tool::Projects,
        args: r#"{"action":"hosts"}"#.to_owned(),
        ..run_on()
    };
    let with = Caller {
        project_tools: Some(Arc::new(Projects)),
        ..caller(table)
    };

    let Reply::Done { text, failed } = answer(request.clone(), with).await else {
        panic!("not done");
    };
    assert!(!failed && text == "no hosts", "{text}");

    let Reply::Done { text, failed } = answer(request, caller(table)).await else {
        panic!("not done");
    };
    assert!(failed && text.contains("only the Beep desktop"), "{text}");
}

/// A team's gateway starts issue sessions on any machine of the mesh with
/// the project: from a desktop's own node, and on a desktop.
#[tokio::test]
async fn team_pickup_is_taken_from_a_desktop_and_on_one() {
    let pickup = Request {
        session: SessionId::default(),
        tool: Tool::TeamPickup,
        args: "{}".to_owned(),
        approved: Default::default(),
    };
    let node = Table::Peer(iroh::SecretKey::generate().public());
    let desktop = Caller {
        desktop_here: true,
        ..caller(node)
    };

    for caller in [caller(Table::Shared), desktop] {
        let Reply::Done { text, failed } = answer(pickup.clone(), caller).await else {
            panic!("not done");
        };
        // Past the gate: this test machine acts for no team.
        assert!(failed);
        assert_eq!(text, "This machine does not act for a team.");
    }
}

/// A desktop that acts for a team lists a node's agents through its
/// controller connection.
#[tokio::test]
async fn a_desktop_lists_a_nodes_agents() {
    let dir = tempfile::tempdir().unwrap();
    let node = Node::test(dir.path()).await;
    let request = |tool, args: &str| Request {
        session: SessionId::default(),
        tool,
        args: args.to_owned(),
        approved: Default::default(),
    };

    let Reply::Done { text, failed } =
        answer(request(Tool::ListAgents, "{}"), Caller::of(&node)).await
    else {
        panic!("not done");
    };
    assert!(!failed, "{text}");
    assert_eq!(text, "[]");

    let start = request(Tool::StartAgent, r#"{"harness":"codex","prompt":"hi"}"#);
    let Reply::Done { failed, .. } = answer(start, Caller::of(&node)).await else {
        panic!("not done");
    };
    assert!(failed);
}

fn note() -> Request {
    Request {
        session: SessionId::from("a3f9c1").to_owned(),
        tool: Tool::AgentNote,
        args: "{}".to_owned(),
        approved: Default::default(),
    }
}

#[tokio::test]
async fn a_desktops_agent_call_is_answered_not_refused() {
    let agents = Some(Arc::new(crate::agents::Agents::new(crate::agents::Parts::default())));
    let from_desktop = Caller {
        from: Some(iroh::SecretKey::generate().public()),
        agents: agents.clone(),
        ..caller(Table::Shared)
    };
    let Reply::Done { text, failed } = answer(note(), from_desktop).await else {
        panic!("not a done reply");
    };
    assert!(!text.contains("own Beep MCP server"), "{text}");
    assert!(failed, "{text}");

    let own = Caller {
        from: None,
        agents,
        ..caller(Table::Shared)
    };
    let Reply::Done { text, failed } = answer(note(), own).await else {
        panic!("not a done reply");
    };
    assert!(failed && text.contains("own Beep MCP server"), "{text}");
}

#[tokio::test]
async fn a_desktops_start_agent_is_refused_while_commands_are_off() {
    let agents = Some(Arc::new(crate::agents::Agents::new(crate::agents::Parts::default())));
    let start = Request {
        tool: Tool::StartAgent,
        args: r#"{"harness":"codex","prompt":"hi"}"#.to_owned(),
        ..note()
    };
    let off = Caller {
        from: Some(iroh::SecretKey::generate().public()),
        agents,
        commands_off: Some("box doesn't run commands: Let Beep use this computer is off".into()),
        ..caller(Table::Shared)
    };
    let Reply::Done { text, failed } = answer(start, off).await else {
        panic!("not a done reply");
    };
    assert!(failed && text.contains("Let Beep use this computer is off"), "{text}");
}
