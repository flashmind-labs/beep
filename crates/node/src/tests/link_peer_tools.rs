use super::*;

fn caller(table: Table, node_here: bool) -> Caller {
    Caller {
        table,
        desktop_here: !node_here,
        commands_off: None,
        transfers: None,
        agents: None,
    }
}

fn run_on() -> Request {
    Request {
        session: "a3f9c1".to_owned(),
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
        let Reply::Done { text, failed } = answer(request, caller(table, true)).await else {
            panic!("not done");
        };
        assert!(failed && text.contains("start_agent"), "{args}: {text}");
        assert!(!text.contains("ran-here"));
    }
}

#[tokio::test]
async fn a_desktop_takes_a_nodes_files_and_syncs_for_it() {
    let table = Table::Peer(iroh::SecretKey::generate().public());
    let on_desktop = caller(table, false);
    let send = Request {
        session: "a3f9c1".to_owned(),
        tool: Tool::SendFiles,
        args: "{}".to_owned(),
        approved: Default::default(),
    };
    let Reply::Done { text, .. } = answer(send, on_desktop.clone()).await else {
        panic!("not done");
    };
    assert!(!text.contains("desktop"), "{text}");
    let sync = Request {
        session: "a3f9c1".to_owned(),
        tool: Tool::SyncProject,
        args: r#"{"project":"game"}"#.to_owned(),
        approved: Default::default(),
    };
    let Reply::Done { text, .. } = answer(sync, on_desktop).await else {
        panic!("not done");
    };
    assert!(!text.contains("desktop"), "{text}");
}

#[tokio::test]
async fn no_node_serves_its_log_to_another() {
    let table = Table::Peer(iroh::SecretKey::generate().public());
    let caller = caller(table, true);
    let request = Request {
        session: "a3f9c1".to_owned(),
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
async fn a_desktop_runs_a_nodes_command_unless_it_lends_itself_to_no_one() {
    let table = Table::Peer(iroh::SecretKey::generate().public());
    let Reply::Done { text, failed } = answer(run_on(), caller(table, false)).await else {
        panic!("not done");
    };
    assert!(!failed && text.contains("ran-here"), "{text}");
    let off = Caller {
        commands_off: Some(
            "mac doesn't run commands: Let Beep use this Mac is off in its Settings.".into(),
        ),
        ..caller(table, false)
    };
    let Reply::Done { text, failed } = answer(run_on(), off).await else {
        panic!("not done");
    };
    assert!(
        failed && text.contains("Let Beep use this Mac is off"),
        "{text}"
    );
    assert!(!text.contains("ran-here") && !text.contains("offline"));
}
