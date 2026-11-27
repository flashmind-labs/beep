use super::*;
use beep_proto::watched;

#[test]
fn adapters_and_harness_helpers_are_hidden_but_commands_are_not() {
    let process = |command: &str| Process {
        pid: 1,
        parent: 0,
        group: 1,
        birth: 1,
        started_at: 1,
        command: command.into(),
    };
    for command in [
        "node /srv/tools/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js",
        "codex app-server",
        "codex.exe app-server",
        "sh -c codex-acp",
        "beep-node --dir /srv/node mcp --session s1",
        "bwrap --unshare-pid",
        "/usr/bin/node /srv/npx/node_modules/@openai/codex/bin/codex.js app-server",
        "/srv/npx/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex-code-mode-host",
        "/opt/tools/cua_node/bin/node_repl",
        "/opt/tools/cua_node/bin/node /opt/tools/cua_node/lib/node_modules/@oai/cua-repl/bin/cua-repl.mjs",
        "node /opt/tools/node_modules/@oai/cua-repl/bin/cua-repl.mjs",
    ] {
        assert!(process(command).helper(), "{command}");
    }
    for command in [
        "npm run dev",
        "node server.js",
        "tsc --watch",
        "echo codex",
        "sh -c 'echo codex'",
        "pip install x",
        "node /srv/app/codex-report.js",
        "node_repl",
        "node /srv/app/cua-repl.mjs",
        "/opt/tools/cua_node/bin/node /srv/app/server.js",
    ] {
        assert!(!process(command).helper(), "{command}");
    }
}

#[tokio::test(start_paused = true)]
async fn the_process_table_is_read_every_second_only_while_followed() {
    let (demand, mut seen) = tokio::sync::watch::channel(0usize);
    let (_session, mut idle) = tokio::sync::watch::channel(false);
    let unfollowed = tokio::spawn(async move {
        let started = Instant::now();
        // Woken for a follower, the read wants a table no older than a second.
        assert_eq!(
            pace(&mut seen, &mut idle, |works: &bool| *works).await,
            Some(WATCHED)
        );
        (started.elapsed(), seen, idle)
    });
    tokio::time::sleep(WATCHED * 2).await;
    assert!(!unfollowed.is_finished());
    // A follower arriving is answered at once, without waiting out the slow pace.
    let waited_from = Instant::now();
    demand.send_replace(1);
    let (elapsed, mut seen, mut idle) = unfollowed.await.unwrap();
    assert_eq!(elapsed, WATCHED * 2);
    assert_eq!(Instant::now(), waited_from);

    let started = Instant::now();
    assert_eq!(
        pace(&mut seen, &mut idle, |works: &bool| *works).await,
        Some(WATCHED)
    );
    assert_eq!(started.elapsed(), WATCHED);

    demand.send_replace(0);
    let started = Instant::now();
    // Unwatched, the read may share a table as old as the time it waited.
    assert_eq!(
        pace(&mut seen, &mut idle, |works: &bool| *works).await,
        Some(UNWATCHED)
    );
    assert_eq!(started.elapsed(), UNWATCHED);
    drop(demand);
    assert_eq!(
        pace(&mut seen, &mut idle, |works: &bool| *works).await,
        None
    );
}

#[tokio::test(start_paused = true)]
async fn the_process_table_is_read_every_second_while_the_session_works() {
    let (_demand, mut seen) = tokio::sync::watch::channel(0usize);
    let (session, mut works) = tokio::sync::watch::channel(false);
    let idle = tokio::spawn(async move {
        let started = Instant::now();
        assert_eq!(
            pace(&mut seen, &mut works, |works: &bool| *works).await,
            Some(WATCHED)
        );
        (started.elapsed(), seen, works)
    });
    tokio::time::sleep(WATCHED * 2).await;
    assert!(!idle.is_finished());
    // A turn starting is answered at once, so a quick wrapper's child is seen.
    session.send_replace(true);
    let (elapsed, mut seen, mut works) = idle.await.unwrap();
    assert_eq!(elapsed, WATCHED * 2);

    let started = Instant::now();
    assert_eq!(
        pace(&mut seen, &mut works, |works: &bool| *works).await,
        Some(WATCHED)
    );
    assert_eq!(started.elapsed(), WATCHED);

    session.send_replace(false);
    let started = Instant::now();
    assert_eq!(
        pace(&mut seen, &mut works, |works: &bool| *works).await,
        Some(UNWATCHED)
    );
    assert_eq!(started.elapsed(), UNWATCHED);
}

#[test]
fn following_processes_holds_demand_until_dropped() {
    let mut seen = DEMAND.subscribe();
    let held = Demand::new();
    assert!(seen.has_changed().unwrap());
    assert!(watched::read_and_update(&mut seen, |seen| *seen) >= 1);
    drop(held);
    assert!(seen.has_changed().unwrap());
}

/// Processes of this test's parent, watched by a poll task of their own.
/// The sender keeps the session alive.
fn watched(
    id: &str,
) -> (
    Arc<Processes>,
    AbortOnDropHandle<()>,
    watch::Sender<crate::agents::live::Live>,
) {
    // The session's root is the agent, left out of the list: this test is its child.
    let processes = Processes::new(Some(std::os::unix::process::parent_id()), None);
    let (session, live) = watch::channel(crate::agents::live::Live::default());
    session.send_modify(|live| live.info.id = id.into());
    let task = processes.watch(live);
    (processes, task, session)
}

fn listening(rows: &[RunningProcess], port: u16) -> bool {
    rows.iter()
        .any(|row| row.pid == std::process::id() && row.ports.contains(&port))
}

#[tokio::test]
async fn a_list_that_is_published_is_read_without_asking_for_a_poll() {
    let processes = Processes::new(Some(1), None);
    let Some(Inbox { mut polls, publish }) = crate::lock(&processes.inbox).take() else {
        panic!("nothing watches yet");
    };
    let row = RunningProcess {
        pid: 7,
        command: "sleep 300".into(),
        started_at: 1,
        ports: vec![],
        left_group: false,
        parent: 0,
        pages: vec![],
    };
    let mut first = std::pin::pin!(processes.list());
    // Nothing is published yet, so the read waits for the first poll.
    assert!(futures::poll!(&mut first).is_pending());

    publish.send_replace(Some(Published {
        at: Instant::now(),
        listing: Ok(Listing {
            rows: Running {
                processes: vec![row.clone()],
                containers: vec![],
            },
            ours: vec![],
        }),
    }));
    assert_eq!(first.await.unwrap().processes, vec![row.clone()]);
    assert_eq!(processes.list().await.unwrap().processes, vec![row]);
    // No reader asked the task to read the table, and none could scan itself.
    assert!(polls.try_recv().is_none());

    // An old list is not served: the reader asks for a read with a port scan.
    publish.send_replace(Some(Published {
        at: Instant::now() - FRESH * 2,
        listing: Ok(Listing {
            rows: Running::default(),
            ours: vec![],
        }),
    }));
    let mut stale = std::pin::pin!(processes.list());
    assert!(futures::poll!(&mut stale).is_pending());
    let (poll, _timing) = polls.recv().await.unwrap();
    assert!(poll.scan);
    publish.send_replace(Some(Published {
        at: Instant::now(),
        listing: Ok(Listing {
            rows: Running::default(),
            ours: vec![],
        }),
    }));
    poll.done.send(Ok(())).unwrap();
    assert!(stale.await.unwrap().processes.is_empty());
}

#[tokio::test]
async fn a_failed_read_is_the_readers_error() {
    let processes = Processes::new(Some(1), None);
    let Some(Inbox { publish, .. }) = crate::lock(&processes.inbox).take() else {
        panic!("nothing watches yet");
    };
    publish.send_replace(Some(Published {
        at: Instant::now(),
        listing: Err("table unreadable".into()),
    }));
    let error = processes.list().await.unwrap_err();
    assert!(format!("{error:#}").contains("table unreadable"));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_port_opened_after_a_reader_listed_reaches_the_next_reader() {
    let id = "ports-reach-every-reader";
    let (processes, _task, _session) = watched(id);
    let mut changes = crate::events::system().subscribe();
    let signalled = |changes: &mut tokio::sync::broadcast::Receiver<crate::events::Change>| {
        let mut seen = false;

        while let Ok(change) = changes.try_recv() {
            seen |= change.topic == Topic::keyed("processes", id);
        }

        seen
    };
    // The first reader lists before the port exists.
    let first = processes.list().await.unwrap();

    // Other tests in this process may open ports too, so only this one is asserted.
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(!listening(&first.processes, port));
    // A second reader lists between the bind and the poll. It must not
    // swallow the change the poll finds.
    assert!(!listening(&processes.list().await.unwrap().processes, port));
    processes.poll(true).await.unwrap();
    assert!(listening(&processes.list().await.unwrap().processes, port));
    assert!(signalled(&mut changes));

    drop(listener);
    processes.poll(true).await.unwrap();
    assert!(!listening(&processes.list().await.unwrap().processes, port));
    assert!(signalled(&mut changes));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_port_that_answers_with_a_page_is_one_of_its_pages_once_it_is_ready() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (processes, _task, _session) = watched("pages-test-session");
    // One port answers like a web server that is still starting at first,
    // the other closes without a word.
    let web = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let silent = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let (web_port, silent_port) = (
        web.local_addr().unwrap().port(),
        silent.local_addr().unwrap().port(),
    );
    // Other tests list this process's ports too, so readiness is a flag
    // rather than a count of requests.
    let ready = Arc::new(AtomicBool::new(false));
    let serving = ready.clone();
    let _web = AbortOnDropHandle::new(tokio::spawn(async move {
        loop {
            let (mut stream, _) = web.accept().await.unwrap();
            let status = if serving.load(Ordering::Relaxed) {
                "200 OK"
            } else {
                "503 Service Unavailable"
            };
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await;
            let reply = format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\n\r\n");
            let _ = stream.write_all(reply.as_bytes()).await;
        }
    }));
    let _silent = AbortOnDropHandle::new(tokio::spawn(async move {
        loop {
            drop(silent.accept().await.unwrap());
        }
    }));

    let me = || async {
        let rows = processes.list().await.unwrap();
        rows.processes
            .into_iter()
            .find(|row| row.pid == std::process::id())
            .unwrap()
    };

    let starting = me().await;
    assert!(starting.ports.contains(&web_port) && starting.ports.contains(&silent_port));
    assert!(!starting.pages.contains(&web_port));
    ready.store(true, Ordering::Relaxed);
    processes.poll(true).await.unwrap();

    let serving = me().await;
    assert!(serving.pages.contains(&web_port));
    assert!(!serving.pages.contains(&silent_port));
}

#[cfg(unix)]
#[tokio::test]
async fn stop_returns_when_the_processes_exit_not_after_the_grace() {
    let mut child = tokio::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let process = read_table(Duration::ZERO)
        .await
        .unwrap()
        .get(&pid)
        .cloned()
        .expect("the child is in the table");

    // Not reaped, so it stays in the table as a zombie: its exit ends the wait.
    tokio::time::timeout(Duration::from_secs(2), stop(&[process]))
        .await
        .expect("stop ends when the process does")
        .unwrap();

    assert!(!child.wait().await.unwrap().success());
}
