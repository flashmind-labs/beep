//! Desktops and machines on loopback, relays off. A desktop calls a machine through a fake node
//! that answers `status/1`, `shell/1` and `agents/1`; the desktops and the machines are real
//! members of one mesh. Waits ride on `updates()` and outer timeouts; nothing sleeps.

use beep_fleet::{
    Desk, FleetError, ImportJob, Machines, MeshState, Options, RepositoryRename,
    RepositoryRenameResult, Session, StandIn, project_id,
};
use beep_mesh::{
    DeviceKind, JOIN_ALPN, Key, Mesh, PEER_ALPN, PeerClient, SessionFn, transport_config,
};
use beep_proto::SessionId;
use beep_proto::watched;
use beep_proto::{
    ApiKey, SecretText, Service, SyncStatus, WorkDiff,
    node::{
        EnvReport, LinkRx, LinkTx, Open, Opened, SyncthingFolder, ToolInfo,
        agent::{DiffAsk, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
        harness::{AutoCompact, HarnessConfig, ModelCompact},
        mesh_state::{Credential, SkillSync, StateValue, SyncSpec},
        shell::{self, Event, Request},
        skill_library,
        status::Report,
        sync::{self, FOLDER_MISSING, ProjectStatus, State as SyncState, SyncthingFiles},
        usage::{HourSpend, PlanUsage, SessionSpend},
    },
};
use iroh::{Endpoint, EndpointId, SecretKey, endpoint::presets, protocol::Router};
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
    time::timeout,
};

const LIMIT: Duration = Duration::from_secs(60);
const SECRET: &str = "ghp_exampleSecretToken9f3a";

struct Dev {
    mesh: Mesh,
    id: EndpointId,
    _router: Router,
    _dir: TempDir,
}

async fn bind(key: &SecretKey) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .secret_key(key.clone())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("endpoint binds")
}

fn serve(endpoint: Endpoint, mesh: &Mesh) -> Router {
    let (peer, join) = mesh.protocols();
    Router::builder(endpoint)
        .accept(PEER_ALPN, peer)
        .accept(JOIN_ALPN, join)
        .spawn()
}

async fn create(name: &str) -> Dev {
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::create(dir.path(), endpoint.clone(), name)
        .await
        .expect("mesh is created");
    Dev {
        id: key.public(),
        _router: serve(endpoint, &mesh),
        mesh,
        _dir: dir,
    }
}

async fn join(from: &Dev, kind: DeviceKind, name: &str) -> Dev {
    let ticket = from
        .mesh
        .invite(kind, Duration::from_secs(3600))
        .await
        .expect("ticket is minted");
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let endpoint = bind(&key).await;
    let mesh = Mesh::join(dir.path(), endpoint.clone(), &ticket, name)
        .await
        .expect("device joins");
    Dev {
        id: key.public(),
        _router: serve(endpoint, &mesh),
        mesh,
        _dir: dir,
    }
}

async fn until(mesh: &Mesh, ok: impl Fn(&Mesh) -> bool) {
    let mut updates = mesh.updates();
    timeout(LIMIT, async {
        loop {
            watched::read_and_update(&mut updates, |_| ());

            if ok(mesh) {
                return;
            }

            updates.changed().await.expect("mesh is alive");
        }
    })
    .await
    .expect("the condition came true in time");
}

fn online(mesh: &Mesh, id: EndpointId) -> bool {
    mesh.devices().iter().any(|d| d.device.id == id && d.online)
}

/// A machine's side of its sessions: what it prints for a command, and what its agents spent.
#[derive(Default)]
struct FakeNode {
    /// A test supplies each Git inspection result after the request arrives.
    skill_reads: Option<mpsc::UnboundedSender<oneshot::Sender<skill_library::Reply>>>,
    /// Commands the shell ran.
    ran: Mutex<Vec<String>>,
    /// What a command prints, by a word it contains.
    prints: Vec<(&'static str, &'static str)>,
    /// What `FollowUsage` says.
    usage: Vec<SessionsReply>,
    /// What `FollowUsage` says after `usage`, set by a test while the node runs.
    later_usage: Mutex<Vec<SessionsReply>>,
    /// The tools its `status/1` reports.
    tools: Vec<&'static str>,
    /// The diffs it was asked for.
    diffed: Mutex<Vec<DiffAsk>>,
    /// It knows session moves; a node from before them closes the stream.
    moves: bool,
    /// The folders its Syncthing keeps in step.
    syncthing: Vec<SyncthingFolder>,
    /// What its `sync/1` says of the projects it syncs.
    synced: Vec<ProjectStatus>,
}

impl FakeNode {
    fn session(self: &Arc<Self>) -> SessionFn {
        let node = self.clone();
        Arc::new(move |tx, rx| {
            let node = node.clone();
            Box::pin(async move { node.serve(tx, rx).await })
        })
    }

    /// What `command` prints: its line in `prints`, else a folder that is not there yet, else a
    /// clone that lands.
    fn prints_for(&self, command: &str) -> String {
        if let Some((_, out)) = self.prints.iter().find(|(word, _)| command.contains(word)) {
            return (*out).to_owned();
        }

        if !command.contains("BEEP_FOLDER:") {
            return "BEEP_IMPORT: cloned\n".to_owned();
        }

        let word = command.lines().find_map(|line| line.strip_prefix("d="));
        let path = word
            .unwrap_or_default()
            .replace(['\'', '"'], "")
            .replace("$HOME", "/home/user");
        format!("BEEP_FOLDER: missing {path}\n")
    }

    async fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) {
        let Ok(Some(open)) = rx.recv::<Open>().await else {
            return;
        };

        if tx.send(&Opened::Ready {}).await.is_err() {
            return;
        }

        match open.protocol.as_str() {
            skill_library::PROTOCOL => {
                let Some(reads) = &self.skill_reads else {
                    return;
                };
                if let Ok(Some(skill_library::Request::Inspect { .. })) = rx.recv().await {
                    let (send, receive) = tokio::sync::oneshot::channel();
                    reads.send(send).expect("the test is listening");
                    if let Ok(reply) = receive.await {
                        tx.send(&reply).await.expect("the reader answers");
                    }
                }
            }
            "status/1" => {
                let offers = vec![shell::PROTOCOL.to_owned(), SESSIONS_PROTOCOL.to_owned()];
                let env = EnvReport {
                    os: "linux".into(),
                    home: "/home/user".into(),
                    tools: ["gh"]
                        .iter()
                        .chain(&self.tools)
                        .map(|name| ToolInfo {
                            name: (*name).into(),
                            version: None,
                        })
                        .collect(),
                    syncthing: self.syncthing.clone(),
                    ..EnvReport::default()
                };

                if tx.send(&Report::Offers(offers)).await.is_err()
                    || tx.send(&Report::Env(env)).await.is_err()
                {
                    return;
                }
                // Held open until the desktop goes.
                while let Ok(Some(_)) = rx.recv::<Open>().await {}
            }
            "shell/1" => {
                while let Ok(Some(Request::Run { command, .. })) = rx.recv::<Request>().await {
                    let text = self.prints_for(&command);
                    self.ran.lock().expect("log").push(command);
                    let done = Event::Done {
                        exit_code: Some(0),
                        timed_out: false,
                        cwd: None,
                        fresh: false,
                        truncated: false,
                    };

                    if tx.send(&Event::Output { text }).await.is_err()
                        || tx.send(&done).await.is_err()
                    {
                        return;
                    }
                }
            }
            "sync/1" => match rx.recv::<sync::Request>().await {
                Ok(Some(sync::Request::Status {})) => {
                    let reply = sync::Reply::Status {
                        projects: self.synced.clone(),
                    };
                    tx.send(&reply).await.ok();
                }
                // Logged with the commands, so a test sees it came before the folder was made.
                Ok(Some(sync::Request::SyncthingFiles { folder, enabled })) => {
                    self.ran
                        .lock()
                        .expect("log")
                        .push(format!("syncthing files {folder} {enabled:?}"));
                    let files = SyncthingFiles {
                        available: true,
                        enabled: enabled.unwrap_or(true),
                        label: Some("share".into()),
                    };
                    tx.send(&sync::Reply::SyncthingFiles { files }).await.ok();
                }
                _ => {}
            },
            "agents/1" => {
                let first = rx.recv::<SessionsRequest>().await;

                if let Ok(Some(SessionsRequest::Diff { ask })) = first {
                    let diff = WorkDiff {
                        root: ask.root.clone(),
                        base: "main".into(),
                        merge_base: "abc123".into(),
                        working_tree: false,
                        files: Vec::new(),
                        additions: 3,
                        deletions: 1,
                        more: 0,
                    };
                    self.diffed.lock().expect("log").push(ask);
                    tx.send(&SessionsReply::Diff { diff }).await.ok();
                    return;
                }

                if self.moves
                    && let Ok(Some(SessionsRequest::MoveProgress { .. })) = first
                {
                    tx.send(&SessionsReply::Moving { progress: None })
                        .await
                        .ok();
                    return;
                }

                let Ok(Some(SessionsRequest::FollowUsage { .. })) = first else {
                    return;
                };

                let later = self.later_usage.lock().expect("later usage").clone();
                for reply in self.usage.iter().chain(&later) {
                    if tx.send(reply).await.is_err() {
                        return;
                    }
                }

                while let Ok(Some(_)) = rx.recv::<Open>().await {}
            }
            _ => {}
        }
    }
}

/// The desktop's machines: real links to the other desktops, and the fake nodes standing in for
/// the machines' own sessions, each pushing the sync status it holds.
fn peering(dev: &Dev, nodes: &[(&Dev, &Arc<FakeNode>)], stuck: &[&Dev]) -> Arc<Machines> {
    let machines = Machines::for_tests(dev.mesh.clone());

    for (dev, node) in nodes {
        let topic = sync_topic();
        machines.stand_in(dev.id, StandIn::Node(node.session()));
        machines.snapshots().open_stream(dev.id, topic);
        machines.snapshots().push(
            dev.id,
            beep_proto::node::events::Read::SyncList,
            beep_proto::node::events::Value::Sync(Box::new(sync::Reply::Status {
                projects: node.synced.clone(),
            })),
        );
    }

    for dev in stuck {
        machines.stand_in(dev.id, StandIn::Stalled);
    }

    machines
}

fn sync_topic() -> beep_proto::node::events::Topic {
    beep_proto::node::events::Read::SyncList
        .topic()
        .expect("the sync list has a topic")
}

async fn desk(dev: &Dev, nodes: &[(&Dev, &Arc<FakeNode>)]) -> Desk {
    Desk::open(peering(dev, nodes, &[]))
        .await
        .expect("the desk opens")
}

/// Two desktops and a machine, all online to each other.
struct Fleet {
    studio: Dev,
    laptop: Dev,
    build: Dev,
}

async fn fleet() -> Fleet {
    let studio = create("Studio").await;
    let laptop = join(&studio, DeviceKind::Desktop, "Laptop").await;
    let build = join(&studio, DeviceKind::Node, "build-box").await;
    until(&studio.mesh, |m| {
        online(m, laptop.id) && online(m, build.id)
    })
    .await;
    until(&laptop.mesh, |m| {
        online(m, studio.id) && online(m, build.id)
    })
    .await;
    Fleet {
        studio,
        laptop,
        build,
    }
}

async fn next_skill_read(
    reads: &mut mpsc::UnboundedReceiver<oneshot::Sender<skill_library::Reply>>,
) -> oneshot::Sender<skill_library::Reply> {
    timeout(LIMIT, reads.recv())
        .await
        .expect("a Git inspection was requested")
        .expect("reader is running")
}

async fn check_without_fetching(
    desk: &Desk,
    reads: &mut mpsc::UnboundedReceiver<oneshot::Sender<skill_library::Reply>>,
) {
    tokio::select! {
        result = desk.check_skill_libraries() => result.unwrap(),
        _ = reads.recv() => panic!("unexpected Git inspection"),
    }
}

#[tokio::test]
async fn one_controller_updates_due_skill_libraries_and_keeps_edits_made_during_fetch() {
    use beep_proto::node::mesh_state::{LibrarySkill, Pick};

    let f = fleet().await;
    let (send, mut reads) = tokio::sync::mpsc::unbounded_channel();
    let reader = Arc::new(FakeNode {
        skill_reads: Some(send),
        ..FakeNode::default()
    });
    let hosts = [(&f.studio, &reader), (&f.laptop, &reader)];
    let a = desk(&f.studio, &hosts).await;
    let b = desk(&f.laptop, &hosts).await;
    let url = "https://example.org/skills.git";
    let inspected = |revision: &str| skill_library::Reply::Inspected {
        revision: revision.to_owned(),
        skills: vec![LibrarySkill {
            name: "notes".into(),
            description: "Notes".into(),
        }],
    };

    let (added, ()) = tokio::join!(a.add_skill_library(url.into(), true), async {
        next_skill_read(&mut reads)
            .await
            .send(inspected("first"))
            .unwrap();
    });
    let (name, first) = added.unwrap();
    let key = Key::SkillLibrary(name.clone());
    let checked = Key::SkillLibraryCheck(name.clone());
    until(&f.laptop.mesh, |mesh| {
        mesh.get(&checked).is_some() && mesh.get(&key).is_some()
    })
    .await;
    a.check_skill_libraries().await.unwrap();
    b.check_skill_libraries().await.unwrap();
    for dev in [&f.studio, &f.laptop] {
        until(&dev.mesh, |mesh| {
            mesh.scan("skill-library-checker/").len() == 2
        })
        .await;
    }

    let (leader, follower) = if f.studio.id < f.laptop.id {
        (&a, &b)
    } else {
        (&b, &a)
    };
    // Adding counted as today's check. An older installation or an expired check is due.
    check_without_fetching(leader, &mut reads).await;
    // Today's check makes the next one due in about a day.
    let wait = leader.next_library_check_in().expect("a library is held");
    assert!(wait > Duration::from_secs(23 * 60 * 60));
    let yesterday = rmp_serde::to_vec(&(url, &first.revision, f.studio.id, 1u64)).unwrap();
    f.studio
        .mesh
        .set(checked.clone(), Some(yesterday.clone()))
        .await
        .unwrap();
    until(&f.laptop.mesh, |mesh| {
        mesh.get(&checked).as_ref() == Some(&yesterday)
    })
    .await;
    check_without_fetching(follower, &mut reads).await;
    assert_eq!(follower.next_library_check_in(), Some(Duration::ZERO));

    until(&f.build.mesh, |mesh| mesh.get(&key).is_some()).await;
    let node_key = f.build._router.endpoint().secret_key().clone();
    f.build._router.shutdown().await.unwrap();
    drop(f.build.mesh);
    until(&f.studio.mesh, |mesh| !online(mesh, f.build.id)).await;

    let pick = Pick::Only(["notes".to_owned()].into());
    let (pass, ()) = tokio::join!(leader.check_skill_libraries(), async {
        let answer = next_skill_read(&mut reads).await;
        leader
            .edit_skill_library(&name, |lib| lib.global = pick.clone())
            .await
            .unwrap();
        answer.send(inspected("second")).unwrap();
    });
    pass.unwrap();
    // Reopen the offline node with its original files and membership.
    let endpoint = bind(&node_key).await;
    let reconnected = Mesh::open(f.build._dir.path(), endpoint.clone())
        .await
        .unwrap()
        .unwrap();
    let _router = serve(endpoint, &reconnected);
    until(&reconnected, |mesh| {
        mesh.get(&key).is_some_and(|bytes| {
            beep_proto::node::mesh_state::SkillLibrary::from_bytes(&bytes)
                .is_ok_and(|lib| lib.revision == "second" && lib.global == pick)
        })
    })
    .await;
    check_without_fetching(leader, &mut reads).await;

    // Manual update still reads immediately. Removing during a fetch keeps the library removed.
    let (updated, ()) = tokio::join!(leader.update_skill_library(&name), async {
        let answer = next_skill_read(&mut reads).await;
        leader.remove_skill_library(&name).await.unwrap();
        answer.send(inspected("third")).unwrap();
    });
    assert!(matches!(updated, Err(FleetError::NotFound)));
    assert!(leader.skill_library(&name).is_none());
}

#[tokio::test]
async fn an_existing_skill_library_retries_a_failed_check_and_stays_fresh_for_a_day() {
    use beep_proto::node::mesh_state::{Pick, SkillLibrary};

    let dev = create("desktop").await;
    let (send, mut reads) = tokio::sync::mpsc::unbounded_channel();
    let reader = Arc::new(FakeNode {
        skill_reads: Some(send),
        ..FakeNode::default()
    });
    let desk = desk(&dev, &[(&dev, &reader)]).await;
    let library = SkillLibrary {
        url: "https://example.org/tools.git".into(),
        revision: "first".into(),
        skills: Vec::new(),
        global: Pick::All,
        projects: Default::default(),
        chats: Default::default(),
    };
    desk.set_skill_library("tools", &library).await.unwrap();
    desk.check_skill_libraries().await.unwrap();
    let checked = Key::SkillLibraryCheck("tools".into());

    let (pass, ()) = tokio::join!(desk.check_skill_libraries(), async {
        next_skill_read(&mut reads)
            .await
            .send(skill_library::Reply::Failed {
                message: "Offline".into(),
            })
            .unwrap();
    });
    pass.unwrap();
    assert!(dev.mesh.get(&checked).is_none());
    assert_eq!(desk.skill_library("tools"), Some(library.clone()));

    let (pass, ()) = tokio::join!(desk.check_skill_libraries(), async {
        next_skill_read(&mut reads)
            .await
            .send(skill_library::Reply::Inspected {
                revision: library.revision.clone(),
                skills: Vec::new(),
            })
            .unwrap();
    });
    pass.unwrap();
    assert!(dev.mesh.get(&checked).is_some());

    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    for hours in [23, 24] {
        let stamp = rmp_serde::to_vec(&(
            &library.url,
            &library.revision,
            dev.id,
            now - hours * 3_600_000,
        ))
        .unwrap();
        dev.mesh.set(checked.clone(), Some(stamp)).await.unwrap();
        if hours == 23 {
            check_without_fetching(&desk, &mut reads).await;
            continue;
        }

        let (pass, ()) = tokio::join!(desk.check_skill_libraries(), async {
            next_skill_read(&mut reads)
                .await
                .send(skill_library::Reply::Inspected {
                    revision: library.revision.clone(),
                    skills: Vec::new(),
                })
                .unwrap();
        });
        pass.unwrap();
    }
}

#[tokio::test]
async fn a_project_imported_on_one_desktop_shows_on_the_other() {
    let f = fleet().await;
    let node = Arc::new(FakeNode::default());
    let studio = desk(&f.studio, &[(&f.build, &node)]).await;
    let imported = studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    assert_eq!(imported.project, "acme/widget");
    assert!(imported.cloned);
    assert_eq!(imported.machine_name, "build-box");
    let clones: Vec<String> = node
        .ran
        .lock()
        .expect("log")
        .iter()
        .filter(|command| command.contains("clone --quiet"))
        .cloned()
        .collect();
    assert_eq!(clones.len(), 1, "{clones:?}");

    // The other desktop was not asked anything: the project reaches it through the mesh.
    // The project and its checkout, which are two keys.
    until(&f.laptop.mesh, |m| m.scan("project/").len() == 2).await;
    let laptop = desk(&f.laptop, &[]).await;
    let seen = laptop.imported().await;
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].project, "acme/widget");
    assert_eq!(seen[0].url, "https://github.com/acme/widget");
    assert_eq!(seen[0].machine_name, "build-box");
    assert_eq!(seen[0].folder, imported.folder);
    // The page's id is the same on both.
    assert_eq!(
        laptop.project_name(project_id("acme/widget")).as_deref(),
        Some("acme/widget")
    );
    // Removing it on one forgets it on the other.
    laptop.remove("acme/widget").await.expect("it is removed");
    until(&f.studio.mesh, |m| {
        m.get(&Key::Project("acme/widget".into())).is_none()
    })
    .await;
}

/// Everything `log` was told, to look for a secret in.
fn logged() -> &'static Mutex<Vec<String>> {
    static LOG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    static INIT: OnceLock<()> = OnceLock::new();
    let lines = LOG.get_or_init(Mutex::default);
    INIT.get_or_init(|| {
        struct Capture;
        impl log::Log for Capture {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                true
            }
            fn log(&self, record: &log::Record<'_>) {
                logged()
                    .lock()
                    .expect("log")
                    .push(format!("{}", record.args()));
            }
            fn flush(&self) {}
        }
        log::set_logger(&Capture).expect("one logger");
        log::set_max_level(log::LevelFilter::Trace);
    });
    lines
}

/// A host that answers GitHub's `user` and `user/orgs`, once per connection.
async fn github() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let base = format!("http://{}", listener.local_addr().expect("an address"));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut head = vec![0; 2048];
            let read = socket.read(&mut head).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&head[..read]);
            const RUN: &str = r#"{"id":77,"name":"CI","run_number":5,"event":"push","status":"in_progress","head_branch":"main","actor":{"login":"octo"}}"#;
            let body = if head.starts_with("GET /user/orgs") {
                r#"[{"login":"acme"}]"#.to_owned()
            } else if head.starts_with("GET /repos/acme/widget/actions/runs/77/jobs") {
                r#"{"jobs":[{"id":3,"name":"Build","status":"in_progress","steps":[{"name":"Run tests","status":"in_progress","number":1}]}]}"#.to_owned()
            } else if head.starts_with("GET /repos/acme/widget/actions/runs/77") {
                RUN.to_owned()
            } else if head.starts_with("GET /repos/acme/widget/actions/runs") {
                format!(r#"{{"total_count":1,"workflow_runs":[{RUN}]}}"#)
            } else {
                r#"{"login":"octo"}"#.to_owned()
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.ok();
        }
    });
    base
}

#[tokio::test]
async fn a_saved_github_token_reaches_every_node_and_no_log() {
    let lines = logged();
    let f = fleet().await;
    let api = github().await;
    let studio = desk(&f.studio, &[]).await.github_api(&api);
    let saved = studio
        .save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .expect("the token is saved");
    assert_eq!(saved.account.as_deref(), Some("octo"));
    assert_eq!(saved.last4, "9f3a");
    assert_eq!(saved.env_var, "GH_TOKEN");

    // The machine reads it as the env var its processes get; so does the other desktop.
    for mesh in [&f.build.mesh, &f.laptop.mesh] {
        // The account's own key, and one for the org it serves.
        until(mesh, |m| m.scan("credential/").len() == 2).await;
        let mut vars: Vec<(String, String)> = mesh
            .scan("credential/")
            .iter()
            .map(|(_, bytes)| {
                let value = Credential::from_bytes(bytes).expect("a node reads it");
                assert!(!format!("{value:?}").contains(SECRET));

                match value {
                    Credential::Env(env) => (env.name, env.value.expose().to_owned()),
                    other => panic!("{other:?}"),
                }
            })
            .collect();
        vars.sort();
        let token = SECRET.to_owned();
        assert_eq!(
            vars,
            [
                ("BEEP_GH_TOKEN_ACME".to_owned(), token.clone()),
                ("GH_TOKEN".to_owned(), token)
            ]
        );
    }

    let laptop = desk(&f.laptop, &[]).await;
    let listed = laptop.credentials();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].account.as_deref(), Some("octo"));
    assert!(!format!("{listed:?}").contains(SECRET));
    let accounts = laptop.github_accounts();
    assert_eq!(accounts.len(), 1);
    assert!(accounts[0].default);
    assert_eq!(accounts[0].orgs, ["acme"]);

    for line in lines.lock().expect("log").iter() {
        assert!(!line.contains(SECRET), "{line}");
    }
}

#[tokio::test]
async fn issues_and_pull_requests_read_with_no_server() {
    let f = fleet().await;
    let node = Arc::new(FakeNode {
        prints: vec![
            (
                "issue list",
                r#"[{"number":7,"title":"Crash on save","labels":["bug"],"author":"octo","createdAt":"2026-10-01T10:00:00Z","comments":2,"url":"https://github.com/acme/widget/issues/7"}]"#,
            ),
            (
                "pr list",
                r#"[{"number":9,"title":"Fix save","url":"https://github.com/acme/widget/pull/9","author":"octo","isDraft":false,"createdAt":"2026-10-02T10:00:00Z","updatedAt":"2026-10-03T10:00:00Z","headRefName":"fix-save","labels":[]}]"#,
            ),
            ("api user", "octo\n"),
            (
                "pr view",
                r#"{"number":9,"headRefName":"fix-save","isCrossRepository":false}"#,
            ),
        ],
        ..FakeNode::default()
    });
    let studio = desk(&f.studio, &[(&f.build, &node)]).await;
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    // The other desktop reads them: the project came through the mesh, the host through the machine.
    // The project and its checkout, which are two keys.
    until(&f.laptop.mesh, |m| m.scan("project/").len() == 2).await;
    let laptop = desk(&f.laptop, &[(&f.build, &node)]).await;
    let issues = laptop.issues("acme/widget").await.expect("issues read");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 7);
    assert_eq!(issues[0].labels, ["bug"]);
    assert_eq!(issues[0].comments, 2);
    let prs = laptop
        .pull_requests("acme/widget")
        .await
        .expect("pull requests read");
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].number, 9);
    assert!(prs[0].mine);
    let lists = laptop.lists("acme/widget").await.expect("lists read");
    assert_eq!(lists.lists.pull_requests.len(), 1);
    assert_eq!(lists.pr_labels, [Vec::<String>::new()]);
    assert_eq!(lists.lists.issues.len(), 1);
    assert!(lists.lists.prs_error.is_none() && lists.lists.issues_error.is_none());
    let head = laptop.pr_head("acme/widget", 9).await.expect("a head");
    assert_eq!(head.branch, "fix-save");
    assert!(!head.fork);
}

#[tokio::test]
async fn home_lists_an_imported_projects_items_with_no_token() {
    let f = fleet().await;
    let node = widget_host();
    let studio = desk(&f.studio, &[(&f.build, &node)]).await;
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    let items = studio.git_items().await.expect("home's items read");
    let rows = &items.items.found.items;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .any(|row| row.number == 7 && row.repo == "acme/widget")
    );
    assert!(rows.iter().any(|row| row.number == 9 && row.authored));
    assert!(items.opened_at.iter().all(Option::is_some));
    assert!(
        items
            .items
            .found
            .hosts
            .iter()
            .all(|host| host.error.is_none())
    );
}

/// Two desktops, a machine that has `repo` with another author's PR and the user's own, and a
/// machine the mesh says is online that never answers a call and holds no project.
async fn with_a_silent_machine(repo: &'static str) -> (Fleet, Desk) {
    let f = fleet().await;
    let ghost = join(&f.studio, DeviceKind::Node, "ghost").await;
    until(&f.studio.mesh, |m| online(m, ghost.id)).await;
    let other = Box::leak(pr_json(repo, 4, "kim").into_boxed_str());
    let own = Box::leak(pr_json(repo, 9, "sam").into_boxed_str());
    let node = shared_host("true\n", other, own);
    desk(&f.studio, &[(&f.build, &node)])
        .await
        .import(ImportJob {
            repo,
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    let studio = Desk::open(peering(&f.studio, &[(&f.build, &node)], &[&ghost]))
        .await
        .expect("the desk opens");
    (f, studio)
}

#[tokio::test]
async fn home_lists_the_others_rows_while_a_machine_never_answers() {
    let (_fleet, studio) = with_a_silent_machine("acme/sprocket").await;
    let items = timeout(Duration::from_secs(8), studio.git_items())
        .await
        .expect("home does not wait on a machine that holds none of the projects")
        .expect("home's items read");
    assert_eq!(items.items.found.items.len(), 2);
}

#[tokio::test]
async fn home_sends_the_list_as_sources_answer_and_ends_on_the_final_one() {
    let (_fleet, studio) = with_a_silent_machine("acme/cog").await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let run = studio.git_items_with(None, Some(&tx));
    // The future must be `Send`, as a Tauri command's is.
    fn send<T: Send>(_: &T) {}
    send(&run);
    let done = timeout(Duration::from_secs(8), run)
        .await
        .expect("the run ends")
        .expect("home's items read");
    drop(tx);
    let mut sent = Vec::new();

    while let Some(items) = rx.recv().await {
        sent.push(items);
    }

    assert!(!sent.is_empty(), "the list is sent before the run ends");
    assert_eq!(sent.last(), Some(&done.items), "the last one is the final");
    let mut numbers: Vec<u64> = done
        .items
        .items
        .found
        .items
        .iter()
        .map(|i| i.number)
        .collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [4, 9]);
}

/// A machine whose account `sam` opened one PR of `repo` and who also lists another author's.
fn shared_host(
    can_push: &'static str,
    other_pr: &'static str,
    own_pr: &'static str,
) -> Arc<FakeNode> {
    Arc::new(FakeNode {
        prints: vec![
            ("issue list", "[]"),
            (
                "pr list",
                Box::leak(format!("[{other_pr},{own_pr}]").into_boxed_str()),
            ),
            ("api user", "sam\n"),
            ("permissions.push", can_push),
        ],
        ..FakeNode::default()
    })
}

fn pr_json(repo: &str, number: u64, author: &str) -> String {
    format!(
        r#"{{"number":{number},"title":"pr {number}","url":"https://github.com/{repo}/pull/{number}","author":"{author}","isDraft":false,"createdAt":"2026-10-02T10:00:00Z","updatedAt":"2026-10-03T10:00:00Z","headRefName":"b{number}","labels":[]}}"#
    )
}

async fn home_numbers(repo: &'static str, can_push: &'static str) -> Vec<u64> {
    let f = fleet().await;
    let other = Box::leak(pr_json(repo, 4, "kim").into_boxed_str());
    let own = Box::leak(pr_json(repo, 9, "sam").into_boxed_str());
    let node = shared_host(can_push, other, own);
    let studio = desk(&f.studio, &[(&f.build, &node)]).await;
    studio
        .import(ImportJob {
            repo,
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    let items = studio.git_items().await.expect("home's items read");
    let mut numbers: Vec<u64> = items
        .items
        .found
        .items
        .iter()
        .map(|row| row.number)
        .collect();
    numbers.sort_unstable();
    numbers
}

#[tokio::test]
async fn with_no_token_a_machine_that_can_push_lists_everyones_items() {
    assert_eq!(home_numbers("acme/gadget", "true\n").await, [4, 9]);
}

#[tokio::test]
async fn with_no_token_a_machine_that_cannot_push_lists_only_the_users_items() {
    assert_eq!(home_numbers("acme/gizmo", "false\n").await, [9]);
}

#[tokio::test]
async fn with_no_token_a_machine_that_cannot_say_lists_only_the_users_items() {
    assert_eq!(home_numbers("acme/doohickey", "no idea\n").await, [9]);
}

#[tokio::test]
async fn usage_sums_every_machine() {
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    let spend = |session: &str, cost: f64, tokens: u64| SessionsReply::SessionSpend {
        usage: SessionSpend {
            agent: "claude".into(),
            session: session.into(),
            cwd: "/home/user/widget".into(),
            since: 0,
            historical: vec![beep_proto::node::usage::HistoricalSpend {
                first: Some(now - 172800),
                last: Some(now - 86400),
                cost_usd: 1.25,
                tokens: 1000,
                billing: beep_proto::node::usage::Billing::Metered,
            }],
            hours: vec![HourSpend {
                hour: now - now % 3600,
                model: "claude-opus-4-1".into(),
                input_tokens: tokens,
                cost_usd: cost,
                billing: beep_proto::node::usage::Billing::Metered,
                ..HourSpend::default()
            }],
        },
    };
    let plan = |at: u64, used: f64| SessionsReply::PlanUsage {
        usage: PlanUsage {
            agent: "claude".into(),
            plan: Some("max".into()),
            windows: vec![beep_proto::node::usage::LimitWindow {
                minutes: 300,
                used_percent: used,
                ..Default::default()
            }],
            credits: Vec::new(),
            account: None,
            at,
        },
    };
    let build = Arc::new(FakeNode {
        usage: vec![
            spend("a", 0.5, 80),
            spend("a", 1.5, 100),
            plan(now - 60, 10.0),
        ],
        ..FakeNode::default()
    });
    let mac = Arc::new(FakeNode {
        usage: vec![spend("b", 2.0, 50), plan(now, 40.0)],
        ..FakeNode::default()
    });
    let studio = desk(&f.studio, &[(&f.build, &build), (&second, &mac)])
        .await
        .settle(Duration::from_millis(300));
    let bill = studio.spend().await.expect("the bill reads");
    assert_eq!(bill.today.cost_usd, 3.5);
    assert_eq!(bill.today.metered_tokens, 150);
    assert_eq!(bill.today.daily.len(), 1);
    assert_eq!(bill.today.daily[0].cost_usd, 3.5);
    assert_eq!(bill.historical.len(), 2);
    assert!(bill.historical.iter().all(|row| row.cost_usd == 1.25));
    assert_eq!(bill.week.cost_usd, 3.5);
    assert_eq!(bill.today.harnesses.len(), 1);
    // The freshest plan reading of an agent wins, and names the machine that read it.
    assert_eq!(bill.limits.len(), 1);
    assert_eq!(bill.limits[0].machine_name, "mac-box");
    assert_eq!(bill.limits[0].windows[0].used_percent, 40.0);
}

#[tokio::test]
async fn groups_pins_and_settings_replicate_to_every_desktop() {
    let f = fleet().await;
    let studio = desk(&f.studio, &[]).await;
    let group = studio
        .save_group(None, "", &["~/Projects/work".to_owned()])
        .await
        .expect("a group is saved");
    assert_eq!(group.name, "work");
    studio.pin("acme/widget", true).await.expect("pinned");
    studio
        .save_harness_settings(
            &f.build.id.to_string(),
            "codex",
            beep_proto::HarnessSettings {
                enabled: true,
                program: "/opt/codex".into(),
                home: String::new(),
                args: vec!["--profile".into(), "work".into()],
                env: vec![beep_proto::HarnessEnv {
                    name: "HTTPS_PROXY".into(),
                    value: Some(SecretText::new("proxy-secret-1")),
                }],
            },
        )
        .await
        .expect("settings are saved");
    let compact = AutoCompact {
        enabled: false,
        percent: Some(60),
        tokens: None,
    };
    studio
        .save_harness_compact("codex", Some(compact))
        .await
        .expect("compaction is saved");
    assert!(
        studio
            .save_harness_compact(
                "codex",
                Some(AutoCompact {
                    enabled: true,
                    percent: Some(101),
                    tokens: None,
                })
            )
            .await
            .is_err(),
        "a percent over 100 is refused"
    );
    studio
        .save_harness_default_model("codex", Some("model-large".into()))
        .await
        .expect("default saved");
    assert!(
        studio
            .save_harness_default_model("codex", Some("bad\nmodel".into()))
            .await
            .is_err()
    );
    let rules = vec![ModelCompact {
        model: "model-small".into(),
        compact: AutoCompact {
            enabled: true,
            percent: None,
            tokens: Some(240_000),
        },
    }];
    studio
        .save_harness_model_compacts("codex", rules.clone())
        .await
        .expect("model overrides saved");
    let mut duplicate = rules.clone();
    duplicate.extend(rules.clone());
    assert!(
        studio
            .save_harness_model_compacts("codex", duplicate)
            .await
            .is_err()
    );
    let first = studio
        .save_instructions("- Write plainly.", None, "")
        .await
        .expect("instructions are saved");
    assert_eq!(first.version, 1);
    let stale = studio.save_instructions("again", None, "").await;
    assert!(stale.is_err(), "a save from an old base is refused");
    until(&f.laptop.mesh, |m| {
        !m.scan("instructions/").is_empty()
            && m.get(&Key::Harness("codex".into()))
                .and_then(|bytes| HarnessConfig::from_bytes(&bytes).ok())
                .is_some_and(|config| config.compact.is_some() && !config.compact_models.is_empty())
            && m.get(&Key::Pin("acme/widget".into())).is_some()
    })
    .await;
    let laptop = desk(&f.laptop, &[]).await;
    assert_eq!(laptop.groups(), vec![group.clone()]);
    assert_eq!(laptop.on_top(), ["acme/widget"]);
    assert_eq!(
        laptop.instructions().map(|v| v.text).as_deref(),
        Some("- Write plainly.")
    );
    let shown = laptop.harness_settings("codex").expect("settings read");
    assert_eq!(shown.len(), 3, "one row per machine");
    assert_eq!(shown[0].settings.args, ["--profile", "work"]);
    // The path is the machine's own: no other machine has it.
    let build_id = f.build.id.to_string();

    for row in &shown {
        let want = if row.machine_id == build_id {
            "/opt/codex"
        } else {
            ""
        };
        assert_eq!(row.settings.program, want, "{}", row.machine_name);
    }
    // The value never comes back, and a save without one keeps it.
    assert_eq!(shown[0].settings.env[0].value, None);
    laptop
        .save_harness_settings(
            &f.build.id.to_string(),
            "codex",
            beep_proto::HarnessSettings {
                env: vec![beep_proto::HarnessEnv {
                    name: "HTTPS_PROXY".into(),
                    value: None,
                }],
                ..shown[0].settings.clone()
            },
        )
        .await
        .expect("settings are saved again");
    let bytes = f
        .laptop
        .mesh
        .get(&Key::Harness("codex".into()))
        .expect("held");
    let config = HarnessConfig::from_bytes(&bytes).expect("reads");
    assert_eq!(config.env[0].value.expose(), "proxy-secret-1");
    // Saving the other settings keeps the compaction setting.
    assert_eq!(config.compact, Some(compact));
    assert_eq!(config.compact_models, rules);
    assert_eq!(
        config.default_model.as_ref().map(AsRef::as_ref),
        Some("model-large")
    );
    assert_eq!(
        laptop.harness_default_model("codex").unwrap(),
        config.default_model
    );
    laptop
        .save_harness_default_model("codex", None)
        .await
        .unwrap();
    assert_eq!(laptop.harness_default_model("codex").unwrap(), None);
    assert_eq!(
        laptop.harness_model_compacts("codex").expect("model rules"),
        rules
    );
    assert_eq!(
        laptop.harness_compact("codex").expect("read"),
        Some(compact)
    );
    laptop.delete_group(group.id).await.expect("deleted");
    until(&f.studio.mesh, |m| {
        m.get(&Key::Group(group.id.to_string())).is_none()
    })
    .await;
}

#[tokio::test]
async fn a_node_cannot_write_a_credential() {
    let f = fleet().await;
    let key = Key::Credential("gh.1.octo".into());
    let value = Credential::Env(beep_proto::node::EnvSecret {
        name: "GH_TOKEN".into(),
        value: SecretText::new(SECRET),
    })
    .to_bytes()
    .expect("encodes");
    f.build
        .mesh
        .set(key.clone(), Some(value))
        .await
        .expect_err("a node may not write state");
    assert!(f.studio.mesh.get(&key).is_none());
}

#[tokio::test]
async fn a_removed_token_leaves_every_node() {
    let f = fleet().await;
    let api = github().await;
    let studio = desk(&f.studio, &[]).await.github_api(&api);
    let saved = studio
        .save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .expect("the token is saved");
    until(&f.build.mesh, |m| m.scan("credential/").len() == 2).await;
    studio
        .remove_credential(&saved.id)
        .await
        .expect("it is removed");
    until(&f.build.mesh, |m| m.scan("credential/").is_empty()).await;
    assert!(studio.credentials().is_empty());
    studio
        .remove_credential(&saved.id)
        .await
        .expect_err("it is gone");
}

#[tokio::test]
async fn a_forgejo_token_is_checked_from_a_machine_that_reaches_the_server() {
    let f = fleet().await;
    let node = Arc::new(FakeNode {
        tools: vec!["teacli"],
        prints: vec![("teacli", r#"{"id":1,"login":"octo"}"#)],
        ..FakeNode::default()
    });
    let studio = desk(&f.studio, &[(&f.build, &node)]).await;
    // Nothing answers here, so this Mac cannot check it and it is saved unchecked.
    let saved = studio
        .save_credential(
            &Service::Forgejo {
                url: "http://127.0.0.1:9".parse().unwrap(),
            },
            &SecretText::new("forge-token-5521"),
        )
        .await
        .expect("an unreachable server is saved unchecked");
    let checked = studio
        .check_credential(&saved.id)
        .await
        .expect("it is checked");
    assert_eq!(
        checked,
        beep_proto::CredentialCheck::Valid {
            account: "octo".into(),
            machine: Some("build-box".into()),
        }
    );
    assert_eq!(node.ran.lock().expect("log").len(), 1);
}

#[tokio::test]
async fn two_desktops_importing_a_project_on_different_machines_keep_both() {
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    until(&f.laptop.mesh, |m| online(m, second.id)).await;
    let (build, mac) = (Arc::new(FakeNode::default()), Arc::new(FakeNode::default()));
    let studio = desk(&f.studio, &[(&f.build, &build), (&second, &mac)]).await;
    let laptop = desk(&f.laptop, &[(&f.build, &build), (&second, &mac)]).await;
    let (a, b) = (f.build.id.to_string(), second.id.to_string());
    let job = |machine: &'static str| ImportJob {
        repo: "acme/widget",
        machine_id: Some(machine),
        folder: None,
        parent: None,
        worktrees: &[],
    };
    let (one, two) = tokio::join!(studio.import(job(a.leak())), laptop.import(job(b.leak())));
    one.expect("the first import works");
    two.expect("the second import works");

    for dev in [&f.studio, &f.laptop] {
        until(&dev.mesh, |m| m.scan("project/").len() == 3).await;
        let seen = desk(dev, &[]).await.imported().await;
        let mut names: Vec<_> = seen.iter().map(|p| p.machine_name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["build-box", "mac-box"]);
    }
}

#[tokio::test]
async fn a_project_on_two_machines_syncs_and_a_pause_and_a_pin_stay() {
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    let (build, mac) = (Arc::new(FakeNode::default()), Arc::new(FakeNode::default()));
    let nodes = [(&f.build, &build), (&second, &mac)];
    let studio = desk(&f.studio, &nodes).await;
    let job = |machine: String| ImportJob {
        repo: "acme/widget",
        machine_id: Some(machine.leak()),
        folder: None,
        parent: None,
        worktrees: &[],
    };
    let key = Key::Sync("acme/widget".to_owned());
    let spec = |mesh: &Mesh| {
        mesh.get(&key)
            .map(|bytes| SyncSpec::from_bytes(&bytes).expect("a spec reads"))
    };

    // One checkout has nothing to sync with; two do, imported one at a time.
    studio
        .import(job(f.build.id.to_string()))
        .await
        .expect("the first import works");
    assert_eq!(spec(&f.studio.mesh), None);
    studio
        .import(job(second.id.to_string()))
        .await
        .expect("the second import works");
    let written = spec(&f.studio.mesh).expect("the record is written");
    assert_eq!(written.folders.keys().copied().collect::<Vec<_>>().len(), 2);
    assert!(written.folders.contains_key(&f.build.id) && written.folders.contains_key(&second.id));

    // A project imported before the rule gets its record with the session's first desk.
    f.studio
        .mesh
        .set(key.clone(), None)
        .await
        .expect("the record goes");
    let peers = peering(&f.studio, &nodes, &[]);
    let session = Session::new(peers.clone(), Options::default()).expect("a session");
    session.desk().await.expect("the desk opens");
    assert_eq!(spec(&f.studio.mesh), Some(written.clone()));

    // A record that says more keeps it: a pause, or a pin with no machines.
    studio
        .pause_sync("acme/widget", true)
        .await
        .expect("it pauses");
    studio
        .import(job(f.build.id.to_string()))
        .await
        .expect("importing again works");
    assert!(spec(&f.studio.mesh).expect("still there").paused);
    let pinned = SyncSpec::default().to_bytes().expect("a spec encodes");
    f.studio
        .mesh
        .set(key.clone(), Some(pinned))
        .await
        .expect("the pin is written");
    let session = Session::new(peers, Options::default()).expect("a session");
    let desk = session.desk().await.expect("the desk opens");
    assert_eq!(spec(&f.studio.mesh), Some(SyncSpec::default()));
    assert!(
        desk.sync_detail("acme/widget")
            .await
            .expect("a detail")
            .pinned
    );
}

#[tokio::test]
async fn adding_a_project_to_a_machine_makes_the_folder_and_joins_its_sync() {
    let f = fleet().await;
    let joined = |name: &'static str| {
        let studio = &f.studio;
        async move {
            let dev = join(studio, DeviceKind::Node, name).await;
            until(&studio.mesh, |m| online(m, dev.id)).await;
            dev
        }
    };
    let (empty, full, kept, files) = (
        joined("empty-box").await,
        joined("full-box").await,
        joined("kept-box").await,
        joined("files-box").await,
    );
    let build = Arc::new(FakeNode::default());
    let lister = |prints: &'static str| {
        Arc::new(FakeNode {
            prints: vec![("BEEP_FOLDER:", prints)],
            ..FakeNode::default()
        })
    };
    let at_empty = lister("BEEP_FOLDER: missing /srv/it's new\n");
    let at_full = lister(
        "BEEP_FOLDER: path /srv/work\nBEEP_FOLDER: git https://github.com/acme/other.git\nBEEP_FOLDER: full\n",
    );
    let at_files = lister("BEEP_FOLDER: path /srv/widget\nBEEP_FOLDER: full\n");
    let at_kept = Arc::new(FakeNode {
        prints: vec![("BEEP_FOLDER:", "BEEP_FOLDER: missing /sync/share/widget\n")],
        syncthing: vec![SyncthingFolder {
            id: "share".into(),
            label: String::new(),
            path: "/sync/share".into(),
        }],
        ..FakeNode::default()
    });
    let studio = desk(
        &f.studio,
        &[
            (&f.build, &build),
            (&empty, &at_empty),
            (&full, &at_full),
            (&kept, &at_kept),
            (&files, &at_files),
        ],
    )
    .await;
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    let key = Key::Sync("acme/widget".to_owned());
    let spec = |mesh: &Mesh| {
        mesh.get(&key)
            .map(|bytes| SyncSpec::from_bytes(&bytes).expect("a spec reads"))
    };
    let made = |node: &FakeNode| {
        let ran = node.ran.lock().expect("log");
        ran.iter()
            .filter(|command| command.contains("mkdir"))
            .cloned()
            .collect::<Vec<_>>()
    };
    let refused = |said: Result<_, FleetError>| match said {
        Err(FleetError::Refused(text)) => text,
        Err(e) => panic!("another error: {e}"),
        Ok(_) => panic!("it was not refused"),
    };

    // Another repository's checkout, or a folder Syncthing keeps for a project that uses
    // Syncthing, is refused: nothing made or written.
    let said = studio
        .add_to_machine("acme/widget", &full.id.to_string(), "/srv/work")
        .await;
    assert_eq!(
        refused(said),
        "/srv/work is a checkout of another repository. Choose an empty folder or a checkout of acme/widget."
    );
    assert!(made(&at_full).is_empty());
    studio
        .project_syncthing_mode("acme/widget", Some(true))
        .await
        .expect("the project uses Syncthing");
    let said = studio
        .add_to_machine("acme/widget", &kept.id.to_string(), "/sync/share/widget")
        .await;
    assert_eq!(
        refused(said),
        "Syncthing keeps /sync/share/widget in step on kept-box, and this project uses Syncthing. Choose another folder."
    );
    assert!(made(&at_kept).is_empty());
    assert_eq!(spec(&f.studio.mesh), None);

    // A folder that is not there is made, quoted, and both machines sync.
    let added = studio
        .add_to_machine("ACME/widget", &empty.id.to_string(), "/srv/it's new")
        .await
        .expect("it is added");
    assert_eq!(added.project, "acme/widget");
    assert_eq!(added.folder, "/srv/it's new");
    assert_eq!(
        made(&at_empty),
        [r#"mkdir -p -- "/srv/it's new" 2>&1"#.to_owned()]
    );
    let written = spec(&f.studio.mesh).expect("the record is written");
    assert_eq!(
        written.folders.get(&empty.id).map(String::as_str),
        Some("/srv/it's new")
    );
    assert!(written.folders.contains_key(&f.build.id));
    let rows = studio
        .project_machines("acme/widget")
        .await
        .expect("the machines");
    let row = rows
        .iter()
        .find(|row| row.machine_id == empty.id.to_string())
        .expect("its row");
    assert_eq!(row.folder.as_deref(), Some("/srv/it's new"));

    // A folder that has the files and no `.git` joins as it is: the history comes to it.
    let added = studio
        .add_to_machine("acme/widget", &files.id.to_string(), "/srv/widget")
        .await
        .expect("it is added");
    assert_eq!(added.folder, "/srv/widget");
    assert!(made(&at_files).is_empty());
    let written = spec(&f.studio.mesh).expect("the record is written");
    assert_eq!(
        written.folders.get(&files.id).map(String::as_str),
        Some("/srv/widget")
    );

    // In a Syncthing share, the folder is made, then leaves the share: the node reads it first.
    studio
        .project_syncthing_mode("acme/widget", Some(false))
        .await
        .expect("the project uses Beep sync");
    let added = studio
        .add_to_machine("acme/widget", &kept.id.to_string(), "/sync/share/widget")
        .await
        .expect("it is added");
    assert_eq!(added.folder, "/sync/share/widget");
    let ran = at_kept.ran.lock().expect("log").clone();
    let asked = |line: &str| ran.iter().position(|ran| ran == line);
    let left = asked("syncthing files /sync/share/widget Some(false)").expect("it left the share");
    let made_at = ran
        .iter()
        .position(|ran| ran.contains("mkdir"))
        .expect("the folder is made");
    assert!(made_at < left);

    // On it already, a second Add changes nothing.
    let said = studio
        .add_to_machine("acme/widget", &empty.id.to_string(), "/srv/other")
        .await;
    assert_eq!(
        refused(said),
        "acme/widget is already on empty-box, in /srv/it's new."
    );
    assert_eq!(made(&at_empty).len(), 1);

    // A project pinned to where it is never syncs, so no folder would ever fill.
    let pinned = SyncSpec::default().to_bytes().expect("a spec encodes");
    f.studio
        .mesh
        .set(key.clone(), Some(pinned))
        .await
        .expect("the pin is written");
    let said = studio
        .add_to_machine("acme/widget", &full.id.to_string(), "/srv/new")
        .await;
    assert_eq!(
        refused(said),
        "acme/widget is pinned to where it is: it never syncs."
    );
    assert!(made(&at_full).is_empty());
}

#[tokio::test]
async fn a_machine_points_at_an_empty_folder_or_a_checkout_and_the_old_one_stays() {
    let f = fleet().await;
    let mac = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, mac.id)).await;
    let build = Arc::new(FakeNode::default());
    let at_mac = Arc::new(FakeNode {
        prints: vec![
            ("/srv/empty", "BEEP_FOLDER: missing /srv/empty\n"),
            (
                "/srv/other",
                "BEEP_FOLDER: path /srv/other\nBEEP_FOLDER: git https://github.com/acme/other.git\nBEEP_FOLDER: full\n",
            ),
            (
                "/srv/clone",
                "BEEP_FOLDER: path /srv/clone\nBEEP_FOLDER: git https://github.com/acme/widget.git\nBEEP_FOLDER: full\n",
            ),
        ],
        ..FakeNode::default()
    });
    let studio = desk(&f.studio, &[(&f.build, &build), (&mac, &at_mac)]).await;

    for dev in [&f.build, &mac] {
        studio
            .import(ImportJob {
                repo: "acme/widget",
                machine_id: Some(&dev.id.to_string()),
                folder: None,
                parent: None,
                worktrees: &[],
            })
            .await
            .expect("the import works");
    }

    let key = Key::Sync("acme/widget".to_owned());
    let folder_on = |id: EndpointId| {
        f.studio
            .mesh
            .get(&key)
            .map(|bytes| SyncSpec::from_bytes(&bytes).expect("a spec reads"))
            .and_then(|spec| spec.folders.get(&id).cloned())
    };
    let before = folder_on(f.build.id).expect("the build box syncs");
    let ran = |node: &FakeNode, word: &str| {
        node.ran
            .lock()
            .expect("log")
            .iter()
            .filter(|command| command.contains(word))
            .count()
    };
    let refused = |said: Result<_, FleetError>| match said {
        Err(FleetError::Refused(text)) => text,
        Err(e) => panic!("another error: {e}"),
        Ok(_) => panic!("it was not refused"),
    };

    let made = ran(&at_mac, "mkdir -p --");
    // Another repository's checkout is refused, and nothing is made or written.
    let said = studio
        .change_folder("acme/widget", &mac.id.to_string(), "/srv/other")
        .await;
    assert_eq!(
        refused(said),
        "/srv/other is a checkout of another repository. Choose an empty folder or a checkout of acme/widget."
    );
    assert_eq!(ran(&at_mac, "mkdir -p --"), made);

    // A missing folder is made, and the others fill it.
    let changed = studio
        .change_folder("ACME/widget", &mac.id.to_string(), "/srv/empty")
        .await
        .expect("it changes");
    assert_eq!(changed.folder, "/srv/empty");
    assert_eq!(folder_on(mac.id).as_deref(), Some("/srv/empty"));
    assert_eq!(ran(&at_mac, "mkdir -p --"), made + 1);

    // A checkout of the same repository joins as it is.
    studio
        .change_folder("acme/widget", &mac.id.to_string(), "/srv/clone")
        .await
        .expect("it changes");
    assert_eq!(folder_on(mac.id).as_deref(), Some("/srv/clone"));
    assert_eq!(ran(&at_mac, "mkdir -p --"), made + 1);
    assert_eq!(folder_on(f.build.id), Some(before));

    let said = studio
        .change_folder("acme/widget", &mac.id.to_string(), "/srv/clone")
        .await;
    assert_eq!(
        refused(said),
        "acme/widget is already in /srv/clone on mac-box."
    );
}

#[tokio::test]
async fn each_machine_says_how_it_syncs_and_only_an_unreachable_one_is_offline() {
    let f = fleet().await;
    let mut others = Vec::new();

    for name in ["mac-box", "win-box"] {
        let dev = join(&f.studio, DeviceKind::Node, name).await;
        until(&f.studio.mesh, |m| online(m, dev.id)).await;
        others.push(dev);
    }

    let status = |state: SyncState, error: Option<String>| ProjectStatus {
        name: "acme/widget".into(),
        folder: "/srv/widget".into(),
        state,
        error,
        ..ProjectStatus::default()
    };
    let missing = format!("{FOLDER_MISSING}: /srv/widget");
    let fakes = [
        Arc::new(FakeNode {
            synced: vec![status(SyncState::Idle, None)],
            ..FakeNode::default()
        }),
        // A node from before it reported the projects it refused.
        Arc::new(FakeNode::default()),
        Arc::new(FakeNode {
            synced: vec![status(SyncState::Failed, Some(missing.clone()))],
            ..FakeNode::default()
        }),
    ];
    let devs = [&f.build, &others[0], &others[1]];
    let nodes: Vec<(&Dev, &Arc<FakeNode>)> = devs.iter().copied().zip(fakes.iter()).collect();
    let studio = desk(&f.studio, &nodes).await;

    for dev in devs {
        studio
            .import(ImportJob {
                repo: "acme/widget",
                machine_id: Some(&dev.id.to_string()),
                folder: None,
                parent: None,
                worktrees: &[],
            })
            .await
            .expect("the import works");
    }

    let detail = studio.sync_detail("acme/widget").await.expect("a detail");
    let of = |dev: &Dev| {
        detail
            .machines
            .iter()
            .find(|m| m.sync.machine_id == dev.id.to_string())
            .map(|m| (m.sync.status, m.sync.error.clone()))
            .expect("its row")
    };
    assert_eq!(of(&f.build), (SyncStatus::InSync {}, None));
    let (said, why) = of(&others[0]);
    assert_eq!(said, SyncStatus::Failed {});
    assert!(why.is_some_and(|why| why.contains("Update beep-node")));
    assert_eq!(of(&others[1]), (SyncStatus::Failed {}, Some(missing)));
}

/// The machines a desk lists the project on, in order.
async fn on(desk: &Desk) -> Vec<EndpointId> {
    let mut ids: Vec<EndpointId> = desk
        .imported()
        .await
        .iter()
        .map(|p| p.machine_id.parse().expect("an id"))
        .collect();
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn removing_a_project_from_a_machine_stops_its_sync_and_keeps_the_last_one() {
    let f = fleet().await;
    let mut others = Vec::new();

    for name in ["mac-box", "mini-box"] {
        let dev = join(&f.studio, DeviceKind::Node, name).await;
        until(&f.studio.mesh, |m| online(m, dev.id)).await;
        others.push(dev);
    }

    let fakes = [
        Arc::new(FakeNode::default()),
        Arc::new(FakeNode::default()),
        Arc::new(FakeNode {
            prints: vec![("cygpath", "BEEP_RM: deleted\n")],
            ..FakeNode::default()
        }),
    ];
    let devs = [&f.build, &others[0], &others[1]];
    let nodes: Vec<(&Dev, &Arc<FakeNode>)> = devs.iter().copied().zip(fakes.iter()).collect();
    let studio = desk(&f.studio, &nodes).await;

    for dev in devs {
        studio
            .import(ImportJob {
                repo: "acme/widget",
                machine_id: Some(&dev.id.to_string()),
                folder: None,
                parent: None,
                worktrees: &[],
            })
            .await
            .expect("the import works");
    }

    let key = Key::Sync("acme/widget".to_owned());
    let spec = |mesh: &Mesh| {
        mesh.get(&key)
            .map(|bytes| SyncSpec::from_bytes(&bytes).expect("a spec reads"))
    };
    let sorted = |mut ids: Vec<EndpointId>| {
        ids.sort_unstable();
        ids
    };
    assert_eq!(spec(&f.studio.mesh).expect("synced").folders.len(), 3);

    // Off one of three, deleting its folder there: the other two keep syncing with each other,
    // and only that machine is asked to delete, once its node no longer syncs the project.
    let folder = studio
        .project_machines("acme/widget")
        .await
        .expect("the machines")
        .into_iter()
        .find(|row| row.machine_id == others[1].id.to_string())
        .and_then(|row| row.folder)
        .expect("its folder");
    let said = studio
        .remove_from_machine("ACME/widget", &others[1].id.to_string(), true)
        .await
        .expect("it is removed");
    assert_eq!(said, Some(format!("Deleted {folder} on mini-box.")));
    let deletes = |node: &FakeNode| {
        node.ran
            .lock()
            .expect("log")
            .iter()
            .filter(|command| command.contains("cygpath"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(deletes(&fakes[2]).len(), 1);
    assert!(deletes(&fakes[2])[0].contains(&folder));
    assert!(deletes(&fakes[0]).is_empty() && deletes(&fakes[1]).is_empty());
    let left = spec(&f.studio.mesh).expect("still synced");
    assert_eq!(
        left.folders.keys().copied().collect::<Vec<_>>(),
        sorted(vec![f.build.id, others[0].id])
    );
    assert_eq!(on(&studio).await, sorted(vec![f.build.id, others[0].id]));

    // Off one of two: nothing left to sync with, so the record goes. Its folder stays.
    let said = studio
        .remove_from_machine("acme/widget", &others[0].id.to_string(), false)
        .await
        .expect("it is removed");
    assert_eq!(said, None);
    assert!(deletes(&fakes[1]).is_empty());
    assert_eq!(spec(&f.studio.mesh), None);
    assert_eq!(on(&studio).await, vec![f.build.id]);

    // A new session puts every project on two machines in sync: the removed ones stay out.
    let session =
        Session::new(peering(&f.studio, &nodes, &[]), Options::default()).expect("a session");
    let again = session.desk().await.expect("the desk opens");
    assert_eq!(spec(&f.studio.mesh), None);
    assert_eq!(on(&again).await, vec![f.build.id]);

    // The last machine keeps it, and one without it has nothing to remove.
    let refused = |said: Result<Option<String>, FleetError>| match said {
        Err(FleetError::Refused(text)) => text,
        Err(e) => panic!("another error: {e}"),
        Ok(_) => panic!("it was not refused"),
    };
    let said = studio
        .remove_from_machine("acme/widget", &f.build.id.to_string(), true)
        .await;
    assert_eq!(refused(said), "A project stays on at least one machine.");
    assert!(deletes(&fakes[0]).is_empty());
    let said = studio
        .remove_from_machine("acme/widget", &others[0].id.to_string(), false)
        .await;
    assert_eq!(refused(said), "acme/widget is not on mac-box.");
    assert_eq!(on(&studio).await, vec![f.build.id]);
}

#[tokio::test]
async fn two_desktops_saving_instructions_keep_both_versions() {
    let f = fleet().await;
    let (studio, laptop) = (desk(&f.studio, &[]).await, desk(&f.laptop, &[]).await);
    studio
        .save_instructions("from the studio", None, "")
        .await
        .expect("the studio saves");
    until(&f.laptop.mesh, |m| m.scan("instructions/").len() == 1).await;
    let second = laptop
        .save_instructions("from the laptop", Some(1), "")
        .await
        .expect("the laptop saves on top");
    assert_eq!(second.version, 2);

    for dev in [&f.studio, &f.laptop] {
        until(&dev.mesh, |m| m.scan("instructions/").len() == 2).await;
        let versions = desk(dev, &[]).await.instructions_versions();
        let texts: Vec<_> = versions.iter().map(|v| v.text.as_str()).collect();
        assert_eq!(texts, ["from the laptop", "from the studio"]);
    }
    // Versions saved at the same moment on two desktops are two keys, so neither is lost.
    for (dev, id) in [(&f.studio, "a"), (&f.laptop, "b")] {
        dev.mesh
            .set(
                Key::InstructionVersion(format!("1793000000000.{id}")),
                Some(vec![1]),
            )
            .await
            .expect("a desktop writes it");
    }

    for dev in [&f.studio, &f.laptop] {
        until(&dev.mesh, |m| m.scan("instructions/").len() == 4).await;
    }
    // The groups list does not mistake a version for a group.
    assert!(desk(&f.studio, &[]).await.groups().is_empty());
}

#[tokio::test]
async fn a_cached_list_comes_at_once_and_a_cold_one_asks_only_this_mac() {
    let f = fleet().await;
    let node = Arc::new(FakeNode {
        prints: vec![
            (
                "issue list",
                r#"[{"number":7,"title":"Crash","labels":[],"author":"octo","createdAt":"2026-10-01T10:00:00Z","comments":0,"url":"https://github.com/acme/widget/issues/7"}]"#,
            ),
            ("pr list", "[]"),
            ("api user", "octo\n"),
        ],
        ..FakeNode::default()
    });
    // The project is checked out on this Mac.
    desk(&f.studio, &[(&f.studio, &node)])
        .await
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.studio.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    // The other machines never answer: a read that asked them would never end.
    let session = beep_fleet::Session::new(
        peering(&f.studio, &[(&f.studio, &node)], &[&f.laptop, &f.build]),
        beep_fleet::Options::default(),
    )
    .expect("a session");
    let cold = timeout(Duration::from_secs(20), session.lists("acme/widget", false))
        .await
        .expect("a cold read waits on no other machine")
        .expect("the lists read");
    assert_eq!(cold.lists.issues.len(), 1);
    let ran = node.ran.lock().expect("log").len();
    let warm = timeout(
        Duration::from_millis(500),
        session.lists("acme/widget", false),
    )
    .await
    .expect("a warm read does not wait")
    .expect("the lists read");
    assert_eq!(warm, cold);
    assert_eq!(
        node.ran.lock().expect("log").len(),
        ran,
        "no host was asked"
    );
}

#[tokio::test]
async fn a_projects_runs_read_with_the_hosts_token() {
    let f = fleet().await;
    let api = github().await;
    let node = Arc::new(FakeNode::default());
    let studio = desk(&f.studio, &[(&f.build, &node)]).await.github_api(&api);
    studio
        .save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .expect("the token is saved");
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    let runs = studio.runs("acme/widget").await.expect("the runs read");
    assert_eq!(runs.error, None);
    assert_eq!(runs.runs.len(), 1);
    assert_eq!(runs.runs[0].run.id, 77);
    assert_eq!(runs.hosts[0].repo, "acme/widget");
    assert!(runs.hosts[0].primary && runs.hosts[0].error.is_none());
    let one = studio
        .run("acme/widget", None, 77)
        .await
        .expect("the run reads");
    assert_eq!(one.jobs.len(), 1);
    assert!(one.run.progress.is_some(), "a running run shows its step");
}

/// GitHub at the root and a Forgejo server under `/forge`, on one port: GitHub has run 77,
/// started first, and Forgejo run 88, started later.
async fn two_forges() -> (String, Arc<AtomicBool>) {
    let unreachable = Arc::new(AtomicBool::new(false));
    let control = unreachable.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let base = format!("http://{}", listener.local_addr().expect("an address"));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut head = vec![0; 2048];
            let read = socket.read(&mut head).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&head[..read]);
            if control.load(Ordering::SeqCst) && head.starts_with("GET /forge/") {
                continue;
            }

            let run = |id: u64, at: &str| {
                format!(
                    r#"{{"id":{id},"name":"CI","run_number":{id},"event":"push","status":"completed","conclusion":"success","head_branch":"main","run_started_at":"{at}","actor":{{"login":"octo"}}}}"#
                )
            };
            let body = if head.starts_with("GET /forge/api/v1/repos/acme/widget/actions/runs") {
                format!(
                    r#"{{"workflow_runs":[{}]}}"#,
                    run(88, "2026-11-03T10:00:00Z")
                )
            } else if head.starts_with("GET /repos/acme/widget/actions/runs") {
                format!(
                    r#"{{"workflow_runs":[{}]}}"#,
                    run(77, "2026-11-03T09:00:00Z")
                )
            } else if head.starts_with("GET /user/orgs") {
                r#"[{"login":"acme"}]"#.to_owned()
            } else {
                r#"{"login":"octo"}"#.to_owned()
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.ok();
        }
    });
    (base, unreachable)
}

#[tokio::test]
async fn a_repository_on_github_and_forgejo_lists_both_hosts_runs_as_one() {
    let f = fleet().await;
    let (api, unreachable) = two_forges().await;
    let forge = format!("{api}/forge");
    let port = api.rsplit(':').next().expect("a port").to_owned();
    // The checkout reaches the Forgejo server twice, over http and over ssh.
    let remotes: &'static str = Box::leak(
        format!(
            "@@dir /srv/widget\nremote.origin.url https://github.com/acme/widget.git\nremote.forge.url http://127.0.0.1:{port}/forge/acme/widget.git\nremote.mirror.url git@127.0.0.1:acme/widget.git\n"
        )
        .into_boxed_str(),
    );
    let node = Arc::new(FakeNode {
        prints: vec![
            ("get-regexp", remotes),
            (
                "teacli api",
                r#"{"workflow_runs":[{"id":88,"name":"CI","status":"completed","conclusion":"success"}]}"#,
            ),
        ],
        ..FakeNode::default()
    });
    let studio = desk(&f.studio, &[(&f.build, &node)]).await.github_api(&api);
    studio
        .save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .expect("the token is saved");
    studio
        .save_credential(
            &Service::Forgejo {
                url: forge.parse().unwrap(),
            },
            &SecretText::new("forge-token-5521"),
        )
        .await
        .expect("the Forgejo token is saved");
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    let runs = studio.runs("acme/widget").await.expect("the runs read");
    assert_eq!(runs.error, None);
    let hosts: Vec<(&str, bool, Option<&str>)> = runs
        .hosts
        .iter()
        .map(|h| (h.host.as_str(), h.primary, h.url.as_deref()))
        .collect();
    let forge_host = format!("127.0.0.1:{port}");
    let forge_page = format!("{forge}/acme/widget");
    assert_eq!(
        hosts,
        [
            ("github.com", true, Some("https://github.com/acme/widget")),
            (forge_host.as_str(), false, Some(forge_page.as_str())),
        ],
        "one host per server, the project's own first"
    );
    let ids: Vec<(u64, &str)> = runs
        .runs
        .iter()
        .map(|r| (r.run.id, r.host.as_str()))
        .collect();
    assert_eq!(
        ids,
        [(88, forge_host.as_str()), (77, "github.com")],
        "newest first, each naming its host"
    );
    assert!(
        !node
            .ran
            .lock()
            .expect("commands")
            .iter()
            .any(|command| command.contains("teacli api"))
    );
    unreachable.store(true, Ordering::SeqCst);
    let fallback = studio
        .runs("acme/widget")
        .await
        .expect("one host falls back");
    assert_eq!(fallback.runs.len(), 2);
    assert!(fallback.hosts.iter().all(|host| host.error.is_none()));
    let commands = node.ran.lock().expect("commands");
    assert!(
        commands
            .iter()
            .any(|command| command.contains("teacli api") && command.contains(&forge))
    );
    assert!(
        !commands.iter().any(|command| command.contains("gh api")),
        "GitHub remains local"
    );
}

/// A GitHub with five running runs and failed run 90, whose failed job 9 has a log. It answers
/// each request and keeps the repository paths it was asked for.
async fn actions_host() -> (String, Arc<Mutex<Vec<String>>>) {
    let (base, asked, _) = actions_host_controlled().await;
    (base, asked)
}

async fn actions_host_controlled() -> (String, Arc<Mutex<Vec<String>>>, Arc<AtomicBool>) {
    let failing = Arc::new(AtomicBool::new(false));
    let control = failing.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let base = format!("http://{}", listener.local_addr().expect("an address"));
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            let failing = control.clone();
            tokio::spawn(async move {
                let mut head = vec![0; 2048];
                let read = socket.read(&mut head).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&head[..read]).into_owned();
                let path = head.split(' ').nth(1).unwrap_or("").to_owned();
                if path.starts_with("/repos/") {
                    log.lock().expect("log").push(path.clone());
                }

                let failed = failing.load(Ordering::SeqCst) && path.contains("/actions/runs?");
                let status = if failed {
                    "503 Service Unavailable"
                } else {
                    "200 OK"
                };
                let body = actions_reply(&path);
                let reply = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.ok();
            });
        }
    });
    (base, asked, failing)
}

fn actions_reply(path: &str) -> String {
    let run = |id: u64| {
        if id == 90 {
            r#"{"id":90,"name":"CI","run_number":9,"event":"push","status":"completed","conclusion":"failure","head_branch":"main","actor":{"login":"octo"}}"#.to_owned()
        } else {
            format!(
                r#"{{"id":{id},"name":"CI","run_number":{id},"event":"push","status":"in_progress","head_branch":"main","actor":{{"login":"octo"}}}}"#
            )
        }
    };
    let id = path
        .strip_prefix("/repos/acme/widget/actions/runs/")
        .and_then(|rest| rest.split(['/', '?']).next())
        .and_then(|id| id.parse::<u64>().ok());
    if path.starts_with("/user/orgs") {
        r#"[{"login":"acme"}]"#.to_owned()
    } else if path.starts_with("/repos/acme/widget/pulls?") {
        "[]".to_owned()
    } else if path == "/repos/acme/widget/actions/jobs/9/logs" {
        "##[group]Run tests\nok\n##[error]boom\n".to_owned()
    } else if let Some(id) = id {
        if path.contains("/jobs") && id == 90 {
            r#"{"jobs":[{"id":9,"name":"Build","status":"completed","conclusion":"failure","steps":[{"name":"Run tests","status":"completed","conclusion":"failure","number":1}]}]}"#.to_owned()
        } else if path.contains("/jobs") {
            r#"{"jobs":[{"id":3,"name":"Build","status":"in_progress","steps":[{"name":"Run tests","status":"in_progress","number":1}]}]}"#.to_owned()
        } else {
            run(id)
        }
    } else if path.starts_with("/repos/acme/widget/actions/runs") {
        let runs: Vec<String> = [71, 72, 73, 74, 75, 90].into_iter().map(run).collect();
        format!(
            r#"{{"total_count":6,"workflow_runs":[{}]}}"#,
            runs.join(",")
        )
    } else {
        r#"{"login":"octo"}"#.to_owned()
    }
}

/// The project `acme/widget` on the build machine with a GitHub token for `api`, and a session
/// reading it.
async fn session_on_actions(f: &Fleet, api: &str) -> Arc<Session> {
    let node = Arc::new(FakeNode::default());
    let studio = desk(&f.studio, &[(&f.build, &node)]).await.github_api(api);
    studio
        .save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .expect("the token is saved");
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    Session::new(
        peering(&f.studio, &[(&f.build, &node)], &[]),
        Options {
            github_api: Some(api.to_owned()),
            ..Options::default()
        },
    )
    .expect("a session")
}

#[tokio::test]
async fn a_finished_run_opened_again_asks_the_host_nothing() {
    let f = fleet().await;
    let (api, asked) = actions_host().await;
    let session = session_on_actions(&f, &api).await;
    let taken = || std::mem::take(&mut *asked.lock().expect("log"));
    let runs = session.runs("acme/widget", false).await.expect("the runs");
    assert_eq!(runs.runs.len(), 6);
    // The run list, pull request heads, and five running runs' jobs.
    assert_eq!(taken().len(), 7);
    let first = session
        .run("acme/widget", None, 90, false)
        .await
        .expect("the run");
    assert_eq!(first.log.expect("a failed run's log").job, "Build");
    let mut read = taken();
    read.sort();
    assert_eq!(
        read,
        [
            "/repos/acme/widget/actions/jobs/9/logs",
            "/repos/acme/widget/actions/runs/90",
            "/repos/acme/widget/actions/runs/90/jobs?per_page=50",
        ]
    );
    session
        .run("acme/widget", None, 90, false)
        .await
        .expect("the run");
    assert!(taken().is_empty(), "a finished run is given as read");
    session
        .run("acme/widget", None, 90, true)
        .await
        .expect("the run");
    assert_eq!(taken().len(), 3, "refresh reads the host");
    session.forget();
    session
        .run("acme/widget", None, 90, false)
        .await
        .expect("the run");
    assert_eq!(taken().len(), 3, "forgotten, it reads the host");
}

/// The project `acme/widget` checked out on this Mac, whose host answers from `node`, and a
/// session on it that cannot reach the other machines at all.
async fn local_project(
    f: &Fleet,
    node: &Arc<FakeNode>,
    fresh: Duration,
) -> (
    Arc<Machines>,
    Arc<beep_fleet::Session>,
    tokio::sync::mpsc::UnboundedReceiver<beep_fleet::Changed>,
) {
    desk(&f.studio, &[(&f.studio, node)])
        .await
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.studio.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    let peers = peering(&f.studio, &[(&f.studio, node)], &[&f.laptop, &f.build]);
    let session = beep_fleet::Session::new(
        peers.clone(),
        beep_fleet::Options {
            fresh,
            ..beep_fleet::Options::default()
        },
    )
    .expect("a session");
    let changes = session.changes().expect("the feed is taken once");
    (peers, session, changes)
}

/// How many changes the feed holds now.
fn heard(changes: &mut tokio::sync::mpsc::UnboundedReceiver<beep_fleet::Changed>) -> usize {
    std::iter::from_fn(|| changes.try_recv().ok()).count()
}

fn widget_host() -> Arc<FakeNode> {
    Arc::new(FakeNode {
        prints: vec![
            (
                "issue list",
                r#"[{"number":7,"title":"Crash","labels":[],"author":"octo","createdAt":"2026-10-01T10:00:00Z","comments":0,"url":"https://github.com/acme/widget/issues/7"}]"#,
            ),
            (
                "pr list",
                r#"[{"number":9,"title":"Fix","url":"https://github.com/acme/widget/pull/9","author":"octo","isDraft":false,"createdAt":"2026-10-02T10:00:00Z","updatedAt":"2026-10-03T10:00:00Z","headRefName":"fix","labels":[]}]"#,
            ),
            ("api user", "octo\n"),
            (
                "pr view",
                r#"{"number":9,"headRefName":"fix","isCrossRepository":false}"#,
            ),
        ],
        ..FakeNode::default()
    })
}

#[tokio::test]
async fn a_background_read_that_fails_does_not_tell_the_page() {
    let f = fleet().await;
    let node = widget_host();
    let (peers, session, mut changes) = local_project(&f, &node, Duration::ZERO).await;
    session
        .lists("acme/widget", false)
        .await
        .expect("a cold read");
    // Stale at once; the mesh goes away, so the read behind it fails.
    peers.set_opened(MeshState::Failed("gone".into()));
    session
        .lists("acme/widget", false)
        .await
        .expect("the last read");
    session.settled().await;
    assert_eq!(heard(&mut changes), 0, "a failure is not news");
    peers.set_opened(MeshState::Open(f.studio.mesh.clone()));
    session
        .lists("acme/widget", false)
        .await
        .expect("the last read");
    session.settled().await;
    assert_eq!(heard(&mut changes), 1);
}

#[tokio::test]
async fn forgetting_makes_the_next_read_go_to_the_host() {
    let f = fleet().await;
    let node = widget_host();
    let (_peers, session, _) = local_project(&f, &node, Duration::from_secs(3600)).await;
    session
        .lists("acme/widget", false)
        .await
        .expect("a cold read");
    let ran = node.ran.lock().expect("log").len();
    session
        .lists("acme/widget", false)
        .await
        .expect("a warm read");
    assert_eq!(node.ran.lock().expect("log").len(), ran);
    session.forget();
    session
        .lists("acme/widget", false)
        .await
        .expect("a cold read again");
    assert!(
        node.ran.lock().expect("log").len() > ran,
        "it asked the host again"
    );
}

#[tokio::test]
async fn a_session_does_not_wait_for_a_mesh_that_never_opens() {
    let f = fleet().await;
    let peers = Machines::new(f.studio.id, None);
    let session = beep_fleet::Session::new(
        Arc::new(peers),
        beep_fleet::Options {
            mesh_wait: Duration::from_millis(100),
            ..beep_fleet::Options::default()
        },
    )
    .expect("a session");

    match session.desk().await {
        Err(FleetError::NoMesh(_)) => {}
        _ => panic!("a mesh still opening is NoMesh"),
    }
}

#[tokio::test]
async fn a_project_imported_in_another_case_is_the_same_project() {
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    let (build, mac) = (Arc::new(FakeNode::default()), Arc::new(FakeNode::default()));
    let studio = desk(&f.studio, &[(&f.build, &build), (&second, &mac)]).await;
    let on = |repo: &'static str, machine: String| {
        let studio = &studio;
        async move {
            studio
                .import(ImportJob {
                    repo,
                    machine_id: Some(machine.leak()),
                    folder: None,
                    parent: None,
                    worktrees: &[],
                })
                .await
        }
    };
    on("acme/widget", f.build.id.to_string())
        .await
        .expect("the first");
    on("Acme/Widget", second.id.to_string())
        .await
        .expect("the same project in another case");
    let seen = studio.imported().await;
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|p| p.project == "acme/widget"), "{seen:?}");
}

#[tokio::test]
async fn a_checkout_here_is_read_without_asking_the_other_machines() {
    let f = fleet().await;
    let node = widget_host();
    let (peers, _session, _) = local_project(&f, &node, Duration::ZERO).await;
    let desk = Desk::open(peers).await.expect("the desk opens");
    let work = async {
        let issues = desk.issues("acme/widget").await.expect("issues");
        let prs = desk
            .pull_requests("acme/widget")
            .await
            .expect("pull requests");
        let head = desk.pr_head("acme/widget", 9).await.expect("a head");
        (issues.len(), prs.len(), head.branch)
    };
    let got = timeout(Duration::from_secs(20), work)
        .await
        .expect("nothing waits on a machine that never answers");
    assert_eq!(got, (1, 1, "fix".to_owned()));
}

/// What an `unmoved/...` record says, as an account moved from a server left it.
#[derive(serde::Serialize, serde::Deserialize)]
enum Unmoved {
    Credential(beep_proto::Credential),
    HarnessEnv { agent: String, name: String },
}

impl beep_proto::node::mesh_state::StateValue for Unmoved {}

/// A credential a server listed with no value, as a moved account kept it.
fn unmoved_credential(id: &str, service: Service, last4: &str, account: Option<&str>) -> Unmoved {
    Unmoved::Credential(beep_proto::Credential {
        id: id.into(),
        service,
        last4: last4.into(),
        account: account.map(Into::into),
        env_var: String::new(),
        to_machines: true,
        created_at: SystemTime::UNIX_EPOCH,
        unreadable: true,
    })
}

/// Writes `records` under `unmoved/<name>`, as a move from a server did.
async fn unmoved(mesh: &Mesh, records: Vec<(String, Unmoved)>) {
    use beep_proto::node::mesh_state::StateValue;

    for (name, record) in records {
        mesh.set(
            Key::Unmoved(name),
            Some(record.to_bytes().expect("a record encodes")),
        )
        .await
        .expect("the record is written");
    }
}

/// A Forgejo token and a codex variable left to enter again.
async fn last_four(mesh: &Mesh) {
    unmoved(
        mesh,
        vec![
            (
                "credential/8".into(),
                unmoved_credential(
                    "8",
                    Service::Forgejo {
                        url: "http://127.0.0.1:9".parse().unwrap(),
                    },
                    "5521",
                    None,
                ),
            ),
            (
                "harness/codex/OPENAI_API_KEY".into(),
                Unmoved::HarnessEnv {
                    agent: "codex".into(),
                    name: "OPENAI_API_KEY".into(),
                },
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn a_moved_credential_lists_as_one_to_enter_again_until_it_is_saved() {
    let studio = create("Studio").await;
    last_four(&studio.mesh).await;
    let desk = desk(&studio, &[]).await;
    let listed = desk.credentials();
    // Only the Forgejo token asks to be entered again.
    assert_eq!(listed.len(), 1);
    assert!(listed[0].unreadable);
    assert_eq!(listed[0].last4, "5521");
    assert!(matches!(listed[0].service, Service::Forgejo { .. }));
    // Entering the token again drops its record, so nodes may take the mesh's value.
    assert_eq!(studio.mesh.scan("unmoved/credential/").len(), 1);
    desk.save_credential(&listed[0].service, &SecretText::new("forge-token-5521"))
        .await
        .expect("saves");
    assert!(studio.mesh.scan("unmoved/credential/").is_empty());
    assert_eq!(desk.credentials().len(), 1);
    assert!(!desk.credentials()[0].unreadable);
    // The same for a harness variable, once it is given a value.
    assert_eq!(studio.mesh.scan("unmoved/harness/").len(), 1);
    let machine = studio.id.to_string();
    desk.save_harness_settings_all(
        &machine,
        "codex",
        beep_proto::HarnessSettings {
            enabled: true,
            program: String::new(),
            home: String::new(),
            args: Vec::new(),
            env: vec![beep_proto::HarnessEnv {
                name: "OPENAI_API_KEY".into(),
                value: Some(SecretText::new("sk-openai-newvalue")),
            }],
        },
        false,
    )
    .await
    .expect("saves");
    assert!(studio.mesh.scan("unmoved/harness/").is_empty());
    let id = desk.credentials()[0].id.clone();
    desk.remove_credential(&id).await.expect("removes it");
    assert!(desk.credentials().is_empty());
}

#[tokio::test]
async fn an_api_key_reaches_the_machines_as_its_env_var() {
    let studio = create("Studio").await;
    let desk = desk(&studio, &[]).await;
    let meshy = Service::ApiKey {
        provider: ApiKey::Meshy,
    };
    desk.save_credential(&meshy, &SecretText::new("msy-placeholder-0001"))
        .await
        .expect("saves");
    let saved = desk
        .save_credential(&meshy, &SecretText::new("msy-placeholder-0002"))
        .await
        .expect("replaces");
    assert_eq!(saved.env_var, "MESHY_API_KEY");
    assert_eq!(saved.last4, "0002");
    assert_eq!(desk.credentials().len(), 1, "one key per provider");

    let values: Vec<(String, Credential)> = studio
        .mesh
        .scan("credential/")
        .into_iter()
        .map(|(key, bytes)| {
            (
                key.to_string(),
                beep_proto::lenient::from_slice(&bytes).unwrap(),
            )
        })
        .collect();
    let [(key, Credential::Env(secret))] = values.as_slice() else {
        panic!("one env credential, got {values:?}");
    };
    assert!(key.starts_with("credential/key."), "{key}");
    assert_eq!(secret.name, "MESHY_API_KEY");
    assert_eq!(secret.value.expose(), "msy-placeholder-0002");

    desk.remove_credential(&saved.id).await.expect("removes");
    assert!(studio.mesh.scan("credential/").is_empty());
}

#[tokio::test]
async fn a_branch_diff_of_a_mesh_project_is_read_from_its_checkout_with_no_server() {
    let f = fleet().await;
    let node = Arc::new(FakeNode::default());
    let studio = desk(&f.studio, &[(&f.build, &node)]).await;
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("the import works");
    // The mesh records the project and where its checkout is: no server is asked.
    assert!(beep_fleet::has_project(&f.studio.mesh, "acme/widget"));
    let places = beep_fleet::places(&f.studio.mesh, "acme/widget");
    assert_eq!(places.len(), 1, "{places:?}");
    let (machine, folder) = places[0].clone();
    assert_eq!(machine, f.build.id);
    // The desktop reads the diff on that machine for the branch, against the checkout's default.
    let client = PeerClient::local(node.session());
    let ask = DiffAsk {
        root: folder.clone(),
        base: String::new(),
        head: "feature/login".into(),
        only: Vec::new(),
        lockfiles: false,
        context: 3,
    };
    let beep_mesh::machine::sessions::NodeWork::Done(diff) =
        beep_mesh::machine::sessions::diff(&client, ask)
            .await
            .expect("the machine answers")
    else {
        panic!("the machine refused the diff");
    };
    assert_eq!((diff.additions, diff.deletions), (3, 1));
    let asked = node.diffed.lock().expect("log").clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].root, folder);
    assert_eq!(asked[0].head, "feature/login");
    assert_eq!(asked[0].base, "");
}

#[tokio::test]
async fn saving_one_github_token_releases_only_its_own_record() {
    let f = fleet().await;
    let api = github().await;
    let github = |id: &str, login| {
        (
            format!("credential/{id}"),
            unmoved_credential(id, Service::GitHub, "9f3a", Some(login)),
        )
    };
    unmoved(
        &f.studio.mesh,
        vec![github("1", "octo"), github("2", "other")],
    )
    .await;
    assert_eq!(f.studio.mesh.scan("unmoved/credential/").len(), 2);
    let studio = desk(&f.studio, &[]).await.github_api(&api);
    // The fake GitHub says every token is octo's.
    studio
        .save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .expect("saved");
    let left: Vec<String> = f
        .studio
        .mesh
        .scan("unmoved/credential/")
        .into_iter()
        .map(|(key, _)| key.to_string())
        .collect();
    assert_eq!(left, ["unmoved/credential/2"]);
}

#[tokio::test]
async fn a_harness_variable_taken_out_drops_its_record_and_one_left_blank_keeps_it() {
    let studio = create("Studio").await;
    last_four(&studio.mesh).await;
    let desk = desk(&studio, &[]).await;
    let machine = studio.id.to_string();
    let settings = |env: Vec<beep_proto::HarnessEnv>| beep_proto::HarnessSettings {
        enabled: true,
        program: String::new(),
        home: String::new(),
        args: Vec::new(),
        env,
    };
    let blank = beep_proto::HarnessEnv {
        name: "OPENAI_API_KEY".into(),
        value: None,
    };
    desk.save_harness_settings_all(&machine, "codex", settings(vec![blank]), false)
        .await
        .expect("saves");
    assert_eq!(studio.mesh.scan("unmoved/harness/").len(), 1);
    // Taken out of the list: the record goes, so the node is not held for it.
    desk.save_harness_settings_all(&machine, "codex", settings(Vec::new()), false)
        .await
        .expect("saves");
    assert!(studio.mesh.scan("unmoved/harness/").is_empty());
}

#[tokio::test]
async fn a_chat_is_asked_to_move_with_its_threads() {
    use beep_fleet::{ChatMoveAsk, ChatSession};
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    let desk = desk(&f.studio, &[]).await;
    let session = |agent: &str, id: &str, cwd: &str, ended: bool| ChatSession {
        agent: agent.to_owned(),
        id: SessionId::from(id),
        cwd: cwd.to_owned(),
        model: None,
        ended,
    };
    let ask = |sessions: Vec<ChatSession>| ChatMoveAsk {
        from: f.build.id,
        to: second.id,
        sessions,
        stop: false,
    };
    let why = |result: Result<_, FleetError>| match result {
        Err(FleetError::Refused(why)) => why,
        other => panic!("not refused: {:?}", other.map(|_| ())),
    };
    let chat = "/srv/home/.beep/assistant/ab12";
    assert!(
        why(desk
            .move_chat(ask(vec![session("opencode", "c", chat, false)]))
            .await)
        .contains("yet")
    );
    assert!(
        why(desk
            .move_chat(ask(vec![session("flsh", "c", "/srv/home/notes", false)]))
            .await)
        .contains("folder of its own")
    );
    let stray = vec![
        session("flsh", "c", chat, false),
        session(
            "codex",
            "t",
            "/srv/home/.beep/assistant/zz99/pricing",
            false,
        ),
    ];
    assert!(why(desk.move_chat(ask(stray)).await).contains("not in this chat"));
    assert!(
        desk.moves().is_empty(),
        "a move that cannot go writes nothing"
    );

    // The user named the chat on its machine: the name goes with it.
    beep_fleet::session_titles::save(
        &f.studio.mesh,
        &f.build.id.to_string(),
        "flsh",
        &SessionId::from("c"),
        "Uni2 research",
    )
    .await
    .expect("the title is saved");
    let records = desk
        .move_chat(ask(vec![
            session("flsh", "c", chat, false),
            session(
                "codex",
                "t1",
                "/srv/home/.beep/assistant/ab12/pricing",
                false,
            ),
            session(
                "claude",
                "t2",
                "/srv/home/.beep/assistant/ab12/pricing/deep",
                true,
            ),
        ]))
        .await
        .expect("the move is asked for");
    let told: Vec<_> = records
        .iter()
        .map(|record| {
            let chat = record.conversation.as_ref().expect("a chat's session");
            (
                record.id.as_ref(),
                record.path.as_str(),
                chat.root.as_str(),
                chat.ended,
            )
        })
        .collect();
    assert_eq!(
        beep_fleet::session_titles::get(
            &f.studio.mesh,
            &second.id.to_string(),
            "flsh",
            &SessionId::from("c")
        )
        .as_deref(),
        Some("Uni2 research")
    );
    let root = "/srv/home/.beep/assistant";
    assert_eq!(
        told,
        [
            ("c", "ab12", root, false),
            ("t1", "ab12/pricing", root, false),
            ("t2", "ab12/pricing/deep", root, true),
        ]
    );
    assert!(records.iter().all(|record| record.project.is_empty()));
    assert_eq!(desk.moves().len(), 3);

    // A chat on a Windows machine keeps its folders' names.
    let records = desk
        .move_chat(ask(vec![session(
            "claude",
            "w",
            r"C:\Users\me\.beep\assistant\cd34",
            false,
        )]))
        .await
        .expect("the move is asked for");
    let chat = records[0].conversation.as_ref().expect("a chat's session");
    assert_eq!(
        (records[0].path.as_str(), chat.root.as_str()),
        ("cd34", r"C:\Users\me\.beep\assistant")
    );
}

#[tokio::test]
async fn a_move_is_asked_for_in_the_mesh_and_refused_in_plain_words_when_it_cannot_go() {
    use beep_fleet::MoveAsk;
    use beep_proto::node::mesh_state::{SessionMove, StateValue, SyncSpec};
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    until(&f.laptop.mesh, |m| online(m, second.id)).await;
    let desk = desk(&f.studio, &[]).await;
    let spec = SyncSpec {
        ignored: Default::default(),
        folders: [
            (f.build.id, "/srv/build/app".to_owned()),
            (second.id, "/Users/me/app".to_owned()),
        ]
        .into(),
        paused: false,
        rules: String::new(),
    };
    f.studio
        .mesh
        .set(
            Key::Sync("acme/app".to_owned()),
            Some(spec.to_bytes().expect("a spec encodes")),
        )
        .await
        .expect("the spec is written");
    let s1 = SessionId::from("s1");
    let ask = |agent: &'static str, cwd: &'static str| MoveAsk {
        agent,
        id: &s1,
        from: f.build.id,
        to: second.id,
        project: "acme/app",
        cwd,
        mode: None,
        model: None,
        worktrees: Vec::new(),
        stop: false,
    };
    let why = |result: Result<_, FleetError>| match result {
        Err(FleetError::Refused(why)) => why,
        other => panic!("not refused: {:?}", other.map(|_| ())),
    };
    assert!(why(desk.move_session(ask("opencode", "/srv/build/app")).await).contains("yet"));
    assert!(why(desk.move_session(ask("codex", "/elsewhere")).await).contains("outside"));

    let record = desk
        .move_session(ask("codex", "/srv/build/app/crates/a"))
        .await
        .expect("the move is asked for");
    assert_eq!(record.path, "crates/a");
    assert_eq!(desk.moves().len(), 1);
    // A record of a move long over goes when the next move is asked for.
    let old = SessionMove {
        at: 1,
        id: "s0".into(),
        ..record.clone()
    };
    f.studio
        .mesh
        .set(
            Key::SessionMove(SessionId::from("s0")),
            Some(old.to_bytes().expect("a record encodes")),
        )
        .await
        .expect("the old record is written");
    assert_eq!(desk.moves().len(), 2);
    let again = MoveAsk {
        id: &SessionId::from("s2"),
        ..ask("codex", "/srv/build/app")
    };
    desk.move_session(again).await.expect("a second move");
    let mut ids: Vec<_> = desk.moves().into_iter().map(|m| m.id).collect();
    ids.sort();
    assert_eq!(ids, ["s1", "s2"], "the old record is gone");
    desk.cancel_move(&SessionId::from("s2"))
        .await
        .expect("taken back");
    // Another desktop sees it, and may take it back.
    until(&f.laptop.mesh, |m| m.scan("session-move/").len() == 1).await;
    desk.cancel_move(&SessionId::from("s1"))
        .await
        .expect("taken back");
    until(&f.laptop.mesh, |m| m.scan("session-move/").is_empty()).await;
}

#[tokio::test]
async fn a_move_takes_folders_from_place_records_when_there_is_no_sync_record() {
    use beep_fleet::MoveAsk;
    use beep_proto::node::mesh_state::{Place, StateValue};
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    let desk = desk(&f.studio, &[]).await;
    let place = |folder: &str| {
        let place = Place {
            folder: folder.to_owned(),
            imported_ms: 0,
            fork: None,
        };
        Some(place.to_bytes().expect("a place encodes"))
    };
    let s1 = SessionId::from("s1");
    let ask_in = |cwd| MoveAsk {
        agent: "codex",
        id: &s1,
        from: f.build.id,
        to: second.id,
        project: "acme/app",
        cwd,
        mode: None,
        model: None,
        worktrees: Vec::new(),
        stop: false,
    };
    let ask = || ask_in("/srv/build/app/crates/a");
    f.studio
        .mesh
        .set(
            Key::Project(format!("acme/app/{}", f.build.id)),
            place("/srv/build/app"),
        )
        .await
        .expect("the place is written");
    match desk.move_session(ask()).await {
        Err(FleetError::Refused(why)) => assert!(why.contains("no checkout"), "{why}"),
        other => panic!("not refused: {:?}", other.map(|_| ())),
    }

    f.studio
        .mesh
        .set(
            Key::Project(format!("acme/app/{}", second.id)),
            place("/Users/me/app"),
        )
        .await
        .expect("the place is written");
    let record = desk
        .move_session(ask())
        .await
        .expect("the move is asked for");
    assert_eq!(record.path, "crates/a");
    // A session left in a worktree moves to the project folder.
    let record = desk
        .move_session(ask_in("/srv/build/app/.worktrees/fix/src"))
        .await
        .expect("the move is asked for");
    assert_eq!(record.path, "");
}

#[tokio::test]
async fn a_node_from_before_session_moves_is_not_offered_one() {
    let f = fleet().await;
    let second = join(&f.studio, DeviceKind::Node, "mac-box").await;
    until(&f.studio.mesh, |m| online(m, second.id)).await;
    let old = Arc::new(FakeNode::default());
    let new = Arc::new(FakeNode {
        moves: true,
        ..FakeNode::default()
    });
    let desk = desk(&f.studio, &[(&f.build, &old), (&second, &new)]).await;
    assert!(
        !desk
            .takes_moves(f.build.id)
            .await
            .expect("the old node answers")
    );
    assert!(
        desk.takes_moves(second.id)
            .await
            .expect("the new node answers")
    );
}

/// What a machine with Claude Code and a Codex sign-in says about itself.
fn with_agents() -> EnvReport {
    EnvReport {
        tools: ["claude", "codex"]
            .iter()
            .map(|name| ToolInfo {
                name: (*name).into(),
                version: None,
            })
            .collect(),
        logins: vec![beep_proto::node::Login {
            tool: "codex".into(),
            signed_in: true,
            account: None,
        }],
        ..EnvReport::default()
    }
}

#[tokio::test]
async fn the_agent_accounts_list_this_mac_though_its_own_probe_says_nothing() {
    let f = fleet().await;
    let build = Arc::new(FakeNode {
        tools: vec!["claude"],
        ..FakeNode::default()
    });
    // This Mac has no node to ask here: all it has is what its node reported before.
    let peering = Machines::for_tests(f.laptop.mesh.clone());
    peering.stand_in(f.build.id, StandIn::Node(build.session()));
    peering.set_env(f.laptop.id, with_agents());
    let laptop = Desk::open(peering).await.expect("the desk opens");
    let accounts = laptop.agent_accounts().await.expect("it lists");
    let names: Vec<_> = accounts
        .machines
        .iter()
        .map(|machine| machine.machine_name.as_str())
        .collect();
    assert_eq!(names, ["build-box", "Laptop"], "{accounts:?}");
    let mine = &accounts.machines[1];
    assert!(mine.claude && mine.codex && mine.online);
    assert_eq!(mine.codex_signed_in, Some(true));
}

#[tokio::test]
async fn a_temporary_device_offline_for_a_day_is_removed_and_a_normal_one_stays() {
    use beep_fleet::temporary::{GONE_AFTER, Leftovers, sweep};
    use std::time::Instant;
    let desk = create("desk").await;
    let other_desk = join(&desk, DeviceKind::Desktop, "other desk").await;
    let ticket = desk
        .mesh
        .invite_temporary(Duration::from_secs(3600))
        .await
        .expect("ticket is minted");
    let key = SecretKey::generate();
    let dir = TempDir::new().expect("temp dir");
    let container = Mesh::join(dir.path(), bind(&key).await, &ticket, "container")
        .await
        .expect("device joins");
    let gone = key.public();
    let normal = join(&desk, DeviceKind::Node, "normal").await;
    until(&other_desk.mesh, |m| m.temporaries().contains(&gone)).await;
    // Both are off now: the container never served, the normal node stops.
    drop(container);
    let normal_id = normal.id;
    drop(normal);
    until(&desk.mesh, |m| !online(m, gone) && !online(m, normal_id)).await;
    let (mut here, mut there) = (Leftovers::default(), Leftovers::default());
    let start = Instant::now();
    assert!(
        sweep(&desk.mesh, &mut here, start)
            .await
            .expect("swept")
            .is_empty()
    );
    assert!(
        sweep(&other_desk.mesh, &mut there, start)
            .await
            .expect("swept")
            .is_empty()
    );
    let later = start + GONE_AFTER - Duration::from_secs(1);
    assert!(
        sweep(&desk.mesh, &mut here, later)
            .await
            .expect("swept")
            .is_empty()
    );
    let due = start + GONE_AFTER;
    let removed = sweep(&desk.mesh, &mut here, due).await.expect("swept");
    assert_eq!(removed.iter().map(|d| d.id).collect::<Vec<_>>(), [gone]);
    assert!(desk.mesh.is_removed(gone));
    assert!(desk.mesh.is_member(normal_id));
    // The other desktop gets there too and finds it done, or removes it first: never an error.
    sweep(&other_desk.mesh, &mut there, due)
        .await
        .expect("swept");
    until(&other_desk.mesh, |m| {
        m.is_removed(gone) && !m.temporaries().contains(&gone)
    })
    .await;
    assert!(
        sweep(&other_desk.mesh, &mut there, due + GONE_AFTER)
            .await
            .expect("swept")
            .is_empty()
    );
    assert!(other_desk.mesh.is_member(normal_id));
}

#[tokio::test]
async fn skill_sync_is_off_by_default_and_shared_between_desktops() {
    let studio = create("Studio").await;
    let laptop = join(&studio, DeviceKind::Desktop, "Laptop").await;
    let a = desk(&studio, &[]).await;
    let b = desk(&laptop, &[]).await;
    assert!(!a.skill_sync_enabled());
    assert!(!b.skill_sync_enabled());

    a.set_skill_sync_enabled(true).await.expect("turns on");
    until(&laptop.mesh, |mesh| {
        mesh.get(&Key::SkillSync)
            .and_then(|bytes| SkillSync::from_bytes(&bytes).ok())
            .is_some_and(|setting| setting.enabled)
    })
    .await;
    assert!(b.skill_sync_enabled());

    a.set_skill_sync_enabled(false).await.expect("pauses");
    until(&laptop.mesh, |mesh| {
        mesh.get(&Key::SkillSync)
            .and_then(|bytes| SkillSync::from_bytes(&bytes).ok())
            .is_some_and(|setting| !setting.enabled)
    })
    .await;
    assert!(!b.skill_sync_enabled());
    assert!(!desk(&laptop, &[]).await.skill_sync_enabled());

    b.set_skill_sync_enabled(true).await.expect("resumes");
    until(&studio.mesh, |mesh| {
        mesh.get(&Key::SkillSync)
            .and_then(|bytes| SkillSync::from_bytes(&bytes).ok())
            .is_some_and(|setting| setting.enabled)
    })
    .await;
    assert!(a.skill_sync_enabled());
}

#[tokio::test]
async fn project_syncthing_choice_reaches_a_machine_that_joins_later() {
    use beep_proto::node::mesh_state::ProjectRecord;
    let studio = create("studio").await;
    let desk = desk(&studio, &[]).await;
    let key = Key::Project("example/repo".to_owned());
    studio
        .mesh
        .set(
            key.clone(),
            Some(
                ProjectRecord {
                    url: "https://example.com/example/repo".into(),
                    ..ProjectRecord::default()
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        desk.project_syncthing_mode("example/repo", None)
            .await
            .unwrap(),
        Some(false)
    );
    assert!(
        !desk
            .project_git_auto_recovery("example/repo", None)
            .await
            .unwrap()
    );
    desk.project_git_auto_recovery("example/repo", Some(true))
        .await
        .unwrap();
    desk.project_syncthing_mode("example/repo", Some(false))
        .await
        .unwrap();
    let later = join(&studio, DeviceKind::Node, "build-box").await;
    until(&later.mesh, |mesh| {
        mesh.get(&key).is_some_and(|bytes| {
            let record = ProjectRecord::from_bytes(&bytes).unwrap();
            record.syncthing_files == Some(false) && record.git_auto_recovery
        })
    })
    .await;
    desk.project_syncthing_mode("example/repo", Some(true))
        .await
        .unwrap();
    until(&later.mesh, |mesh| {
        mesh.get(&key).is_some_and(|bytes| {
            let record = ProjectRecord::from_bytes(&bytes).unwrap();
            record.git_auto_recovery
                && record.syncthing_files == Some(true)
                && record.url == "https://example.com/example/repo"
        })
    })
    .await;
    desk.project_git_auto_recovery("example/repo", Some(false))
        .await
        .unwrap();
    until(&later.mesh, |mesh| {
        mesh.get(&key).is_some_and(|bytes| {
            let record = ProjectRecord::from_bytes(&bytes).unwrap();
            !record.git_auto_recovery && record.syncthing_files == Some(true)
        })
    })
    .await;
}

#[tokio::test]
async fn session_title_overrides_replicate_and_clear_without_changing_generated_titles() {
    use beep_fleet::session_titles;
    use beep_proto::{AgentStatus, MachineSession};

    let desktop = create("desktop").await;
    let other = join(&desktop, DeviceKind::Desktop, "other-desktop").await;
    let machine = desktop.id.to_string();
    let mut changes = other.mesh.changes();
    session_titles::save(
        &desktop.mesh,
        &machine,
        "codex",
        &SessionId::from("session-1"),
        "  My title  ",
    )
    .await
    .expect("title is saved");
    // The other desktop hears which session took which title, so an open view retitles in place.
    let retitled = timeout(LIMIT, async {
        loop {
            let key = changes.recv().await.expect("mesh changes");
            if let Some(retitled) = session_titles::changed(&other.mesh, &key) {
                break retitled;
            }
        }
    })
    .await
    .expect("title change is announced");
    assert_eq!(
        (
            retitled.machine_id.as_str(),
            retitled.agent.as_str(),
            retitled.session.as_ref(),
            retitled.title.as_deref()
        ),
        (machine.as_str(), "codex", "session-1", Some("My title"))
    );
    assert!(session_titles::changed(&other.mesh, &beep_mesh::Key::Flsh).is_none());
    assert_eq!(
        session_titles::get(
            &desktop.mesh,
            &machine,
            "claude",
            &SessionId::from("session-1")
        ),
        None
    );
    assert_eq!(
        session_titles::get(
            &desktop.mesh,
            "other-machine",
            "codex",
            &SessionId::from("session-1")
        ),
        None
    );
    assert!(
        session_titles::save(
            &desktop.mesh,
            &machine,
            "codex",
            &SessionId::from("session-1"),
            &"x".repeat(201)
        )
        .await
        .is_err()
    );
    assert!(
        session_titles::save(
            &desktop.mesh,
            &machine,
            "codex",
            &SessionId::from("session-1"),
            "two\nlines"
        )
        .await
        .is_err()
    );
    assert_eq!(
        session_titles::get(
            &desktop.mesh,
            &machine,
            "codex",
            &SessionId::from("session-1")
        )
        .as_deref(),
        Some("My title")
    );
    let generated = MachineSession {
        reply: None,
        changes: None,
        machine_id: machine.clone(),
        machine_name: "build-box".into(),
        id: "session-1".into(),
        agent: "codex".into(),
        title: Some("Generated title".into()),
        cwd: "/srv/project".into(),
        status: AgentStatus::Idle {},
        doing: None,
        started_at: UNIX_EPOCH,
        last_activity: UNIX_EPOCH,
        external: false,
        question: None,
        subagents: 0,
        model: None,
        assistant: false,
        account: None,
        persona: None,
        place: None,
        continues: None,
    };
    let mut refreshed = generated.clone();
    session_titles::apply(&other.mesh, &mut refreshed);
    assert_eq!(refreshed.title.as_deref(), Some("My title"));
    refreshed = generated.clone();
    refreshed.title = Some("A newer generated title".into());
    session_titles::apply(&other.mesh, &mut refreshed);
    assert_eq!(refreshed.title.as_deref(), Some("My title"));
    session_titles::save(
        &desktop.mesh,
        &machine,
        "codex",
        &SessionId::from("session-1"),
        " ",
    )
    .await
    .expect("override is cleared");
    until(&other.mesh, |mesh| {
        session_titles::get(mesh, &machine, "codex", &SessionId::from("session-1")).is_none()
    })
    .await;
    let mut refreshed = generated;
    session_titles::apply(&other.mesh, &mut refreshed);
    assert_eq!(refreshed.title.as_deref(), Some("Generated title"));
}

#[tokio::test]
async fn ignored_folder_selection_reaches_later_machines_and_preserves_pause_and_places() {
    use beep_proto::node::{mesh_state::ProjectRecord, sync::IgnoredSync};
    let studio = create("studio").await;
    let desk = desk(&studio, &[]).await;
    studio
        .mesh
        .set(
            Key::Project("example/repo".into()),
            Some(
                ProjectRecord {
                    url: "https://example.com/example/repo".into(),
                    ..ProjectRecord::default()
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    let key = Key::Sync("example/repo".into());
    let spec = SyncSpec {
        folders: [(studio.id, "/srv/project".into())].into(),
        paused: true,
        ..SyncSpec::default()
    };
    studio
        .mesh
        .set(key.clone(), Some(spec.to_bytes().unwrap()))
        .await
        .unwrap();
    assert_eq!(
        desk.project_ignored_sync("example/repo", None)
            .await
            .unwrap(),
        IgnoredSync::default()
    );
    let selection = IgnoredSync {
        enabled: true,
        folders: vec!["models".into()],
    };
    desk.project_ignored_sync("example/repo", Some(selection.clone()))
        .await
        .unwrap();
    let later = join(&studio, DeviceKind::Node, "build-box").await;
    until(&later.mesh, |mesh| {
        mesh.get(&key).is_some_and(|bytes| {
            let got = SyncSpec::from_bytes(&bytes).unwrap();
            got.ignored == selection && got.paused && got.folders == spec.folders
        })
    })
    .await;
    assert!(
        desk.project_ignored_sync(
            "example/repo",
            Some(IgnoredSync {
                enabled: true,
                folders: vec!["../outside".into()]
            })
        )
        .await
        .is_err()
    );
    desk.project_ignored_sync(
        "example/repo",
        Some(IgnoredSync {
            enabled: false,
            ..selection.clone()
        }),
    )
    .await
    .unwrap();
    until(&later.mesh, |mesh| {
        mesh.get(&key).is_some_and(|bytes| {
            let got = SyncSpec::from_bytes(&bytes).unwrap();
            !got.ignored.enabled && got.ignored.folders == selection.folders && got.paused
        })
    })
    .await;
}

#[tokio::test]
async fn concurrent_actions_loads_share_one_local_read() {
    let f = fleet().await;
    let (api, asked) = actions_host().await;
    let session = session_on_actions(&f, &api).await;
    asked.lock().expect("requests").clear();
    let (first, second) = tokio::join!(
        session.runs("acme/widget", false),
        session.runs("acme/widget", true)
    );
    assert_eq!(first.expect("first read"), second.expect("shared read"));
    let paths = asked.lock().expect("requests");
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.starts_with("/repos/acme/widget/actions/runs?"))
            .count(),
        1
    );
    assert!(
        paths
            .iter()
            .any(|path| path.starts_with("/repos/acme/widget/pulls?"))
    );
}

#[tokio::test]
async fn unreachable_local_actions_use_a_node() {
    let f = fleet().await;
    let (api, _) = actions_host().await;
    let node = Arc::new(FakeNode {
        prints: vec![(
            "gh api",
            r#"{"workflow_runs":[{"id":77,"name":"CI","status":"completed","conclusion":"success"}]}"#,
        )],
        ..FakeNode::default()
    });
    let studio = desk(&f.studio, &[(&f.build, &node)]).await.github_api(&api);
    studio
        .save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .expect("token");
    studio
        .import(ImportJob {
            repo: "acme/widget",
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .expect("import");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("unused port");
    let unreachable = format!("http://{}", listener.local_addr().expect("address"));
    drop(listener);
    let studio = studio.github_api(&unreachable);
    let runs = studio.runs("acme/widget").await.expect("fallback");
    assert_eq!(runs.runs.len(), 1);
    assert_eq!(runs.runs[0].run.id, 77);
    assert!(runs.hosts[0].error.is_none());
    assert!(
        node.ran
            .lock()
            .expect("commands")
            .iter()
            .any(|command| command.contains("gh api"))
    );
}

#[tokio::test]
async fn failed_actions_refresh_keeps_previously_loaded_runs() {
    let f = fleet().await;
    let (api, _, failing) = actions_host_controlled().await;
    let session = session_on_actions(&f, &api).await;
    let first = session
        .runs("acme/widget", false)
        .await
        .expect("initial runs");
    failing.store(true, Ordering::SeqCst);
    let failed = session
        .runs("acme/widget", true)
        .await
        .expect("host failure is reported");
    assert_eq!(failed.runs, first.runs);
    assert_eq!(failed.hosts[0].synced_at, first.hosts[0].synced_at);
    assert!(failed.hosts[0].error.is_some());
    failing.store(false, Ordering::SeqCst);
    let recovered = session
        .runs("acme/widget", true)
        .await
        .expect("recovered host");
    assert_eq!(recovered.runs.len(), first.runs.len());
    assert!(recovered.hosts[0].error.is_none());
}

#[tokio::test]
async fn project_rename_preserves_identity_checkouts_pins_and_sync() {
    use beep_proto::node::mesh_state::{Place, ProjectRecord};
    let dev = create("Desktop").await;
    let desk = desk(&dev, &[]).await;
    let key = Key::Project("acme/widget".into());
    let record = ProjectRecord {
        url: "https://github.com/acme/widget".into(),
        git_auto_recovery: true,
        ..ProjectRecord::default()
    };
    dev.mesh
        .set(key.clone(), Some(record.to_bytes().unwrap()))
        .await
        .unwrap();
    let place_key = Key::Project(format!("acme/widget/{}", dev.id));
    let place = Place {
        folder: "/srv/project".into(),
        imported_ms: 42,
        fork: None,
    };
    dev.mesh
        .set(place_key.clone(), Some(place.to_bytes().unwrap()))
        .await
        .unwrap();
    let sync_key = Key::Sync("acme/widget".into());
    let sync = SyncSpec {
        folders: [(dev.id, place.folder.clone())].into(),
        paused: true,
        ..SyncSpec::default()
    };
    dev.mesh
        .set(sync_key.clone(), Some(sync.to_bytes().unwrap()))
        .await
        .unwrap();
    desk.pin("acme/widget", true).await.unwrap();
    let pinned = dev.mesh.get(&Key::Pin("acme/widget".into()));
    desk.rename_project("acme/widget", " New project ", &[])
        .await
        .unwrap();
    let renamed = ProjectRecord::from_bytes(&dev.mesh.get(&key).unwrap()).unwrap();
    assert_eq!(renamed.display_name.as_deref(), Some("New project"));
    assert_eq!(renamed.url, record.url);
    assert!(renamed.git_auto_recovery);
    assert_eq!(dev.mesh.get(&place_key).unwrap(), place.to_bytes().unwrap());
    assert_eq!(dev.mesh.get(&sync_key).unwrap(), sync.to_bytes().unwrap());
    assert_eq!(dev.mesh.get(&Key::Pin("acme/widget".into())), pinned);
    let imported = desk.imported().await;
    assert_eq!(imported[0].project, "acme/widget");
    assert_eq!(imported[0].display_name.as_deref(), Some("New project"));
    for bad in ["", "  ", "name\nother"] {
        assert!(desk.rename_project("acme/widget", bad, &[]).await.is_err());
    }
    let other = ProjectRecord {
        display_name: Some("Taken".into()),
        ..record
    };
    dev.mesh
        .set(
            Key::Project("acme/other".into()),
            Some(other.to_bytes().unwrap()),
        )
        .await
        .unwrap();
    assert!(
        desk.rename_project("acme/widget", "taken", &[])
            .await
            .is_err()
    );
    assert!(
        desk.rename_project(
            "acme/widget",
            "New project",
            &[RepositoryRename {
                url: "https://github.com/acme/widget".into(),
                name: "../bad".into()
            }]
        )
        .await
        .is_err()
    );
    assert_eq!(
        ProjectRecord::from_bytes(&dev.mesh.get(&key).unwrap()).unwrap(),
        renamed
    );
}

#[tokio::test]
async fn hosted_project_rename_records_remote_updates_and_reimport_keeps_identity() {
    use beep_proto::node::mesh_state::ProjectRecord;
    let f = fleet().await;
    let node = Arc::new(FakeNode::default());
    let api = github().await;
    let desk = desk(&f.studio, &[(&f.build, &node)]).await.github_api(&api);
    let machine = f.build.id.to_string();
    let job = |repo| ImportJob {
        repo,
        machine_id: Some(machine.as_str()),
        folder: None,
        parent: None,
        worktrees: &[],
    };
    desk.import(job("acme/widget")).await.unwrap();
    let key = Key::Project("acme/widget".into());
    let failed = desk
        .rename_project(
            "acme/widget",
            "New name",
            &[RepositoryRename {
                url: "https://github.com/acme/widget".into(),
                name: "new".into(),
            }],
        )
        .await
        .unwrap();
    assert!(matches!(
        failed.as_slice(),
        [RepositoryRenameResult::Failed { .. }]
    ));
    let saved = ProjectRecord::from_bytes(&f.studio.mesh.get(&key).unwrap()).unwrap();
    assert_eq!(saved.display_name.as_deref(), Some("New name"));
    assert!(saved.remote_renames.is_empty());
    desk.save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .unwrap();
    desk.rename_project(
        "acme/widget",
        "New name",
        &[RepositoryRename {
            url: "https://github.com/acme/widget".into(),
            name: "new".into(),
        }],
    )
    .await
    .unwrap();
    let record = ProjectRecord::from_bytes(&f.studio.mesh.get(&key).unwrap()).unwrap();
    assert_eq!(record.url, "https://github.com/acme/new");
    assert_eq!(record.display_name.as_deref(), Some("New name"));
    assert_eq!(record.remote_renames.len(), 1);
    assert_eq!(
        record.remote_renames[0].from,
        "https://github.com/acme/widget"
    );
    assert_eq!(record.remote_renames[0].to, record.url);
    let reimported = desk.import(job("acme/new")).await.unwrap();
    assert_eq!(reimported.project, "acme/widget");
    assert_eq!(desk.imported().await.len(), 1);
    assert!(
        f.studio
            .mesh
            .get(&Key::Project("acme/new".into()))
            .is_none()
    );
}

#[tokio::test]
async fn repository_copies_rename_independently_and_retries_keep_successful_hosts() {
    use beep_proto::node::mesh_state::{Place, ProjectRecord};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let deny = Arc::new(AtomicBool::new(true));
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let denied = deny.clone();
    let seen = requests.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buffer = [0; 4096];
            let size = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..size]);
            let line = request.lines().next().unwrap_or_default().to_owned();
            let forbidden = line.starts_with("PATCH /forge/") && denied.load(Ordering::SeqCst);
            if line.starts_with("PATCH ") {
                seen.lock().unwrap().push(line.clone());
            }
            let body = if line.starts_with("GET /user/orgs") {
                r#"[{"login":"acme"}]"#
            } else if line.contains("/actions/runs") {
                r#"{"workflow_runs":[]}"#
            } else {
                r#"{"login":"octo"}"#
            };
            let status = if forbidden { "403 Forbidden" } else { "200 OK" };
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    let forge = format!("{api}/forge");
    let remotes = Box::leak(format!("@@dir /srv/widget\nremote.origin.url https://github.com/acme/widget.git\nremote.forge.url {forge}/team/mirror-copy.git\nremote.forge-ssh.url git@127.0.0.1:team/mirror-copy.git\nremote.fork.url https://github.com/fork/widget.git\nremote.unrelated.url {forge}/team/other.git\n").into_boxed_str());
    let f = fleet().await;
    let node = Arc::new(FakeNode {
        prints: vec![(
            "get-regexp",
            "@@dir /srv/widget\nremote.origin.url https://github.com/acme/widget.git\n",
        )],
        ..FakeNode::default()
    });
    let mirror = Arc::new(FakeNode {
        prints: vec![("get-regexp", remotes)],
        ..FakeNode::default()
    });
    let desk = desk(&f.studio, &[(&f.build, &node), (&f.laptop, &mirror)])
        .await
        .github_api(&api);
    desk.save_credential(&Service::GitHub, &SecretText::new(SECRET))
        .await
        .unwrap();
    desk.save_credential(
        &Service::Forgejo {
            url: forge.parse().unwrap(),
        },
        &SecretText::new("example-token"),
    )
    .await
    .unwrap();
    desk.import(ImportJob {
        repo: "acme/widget",
        machine_id: Some(&f.build.id.to_string()),
        folder: None,
        parent: None,
        worktrees: &[],
    })
    .await
    .unwrap();
    f.studio
        .mesh
        .set(
            Key::Project(format!("acme/widget/{}", f.laptop.id)),
            Some(
                Place {
                    folder: "/srv/widget".into(),
                    imported_ms: 0,
                    fork: None,
                }
                .to_bytes()
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    let targets = desk.rename_repositories("acme/widget").await.unwrap();
    assert_eq!(
        targets.len(),
        2,
        "SSH and HTTPS copies are deduplicated, forks and unrelated repos stay out"
    );
    assert_eq!(targets[0].url, "https://github.com/acme/widget");
    assert_eq!(targets[1].url, format!("{forge}/team/mirror-copy"));
    let choices = [
        RepositoryRename {
            url: targets[0].url.clone(),
            name: "renamed".into(),
        },
        RepositoryRename {
            url: targets[1].url.clone(),
            name: "mirrored".into(),
        },
    ];
    let results = desk
        .rename_project("acme/widget", "New name", &choices)
        .await
        .unwrap();
    assert!(matches!(
        results.as_slice(),
        [
            RepositoryRenameResult::Renamed { .. },
            RepositoryRenameResult::Failed { .. }
        ]
    ));
    let key = Key::Project("acme/widget".into());
    let saved = ProjectRecord::from_bytes(&f.studio.mesh.get(&key).unwrap()).unwrap();
    assert_eq!(saved.remote_renames.len(), 1);
    assert_eq!(saved.url, "https://github.com/acme/renamed");
    assert_eq!(saved.display_name.as_deref(), Some("New name"));
    deny.store(false, Ordering::SeqCst);
    let retried = desk
        .rename_project("acme/widget", "New name", &choices[1..])
        .await
        .unwrap();
    assert!(matches!(
        retried.as_slice(),
        [RepositoryRenameResult::Renamed { .. }]
    ));
    let saved = ProjectRecord::from_bytes(&f.studio.mesh.get(&key).unwrap()).unwrap();
    assert_eq!(saved.remote_renames.len(), 2);
    assert_eq!(saved.remote_renames[1].to, format!("{forge}/team/mirrored"));
    let repeated = desk
        .rename_project("acme/widget", "New name", &choices)
        .await
        .unwrap();
    assert!(
        repeated
            .iter()
            .all(|result| matches!(result, RepositoryRenameResult::Renamed { .. }))
    );
    let targets = desk.rename_repositories("acme/widget").await.unwrap();
    assert_eq!(targets[0].url, "https://github.com/acme/renamed");
    assert_eq!(targets[1].url, format!("{forge}/team/mirrored"));
    let runs = desk.runs("acme/widget").await.unwrap();
    assert_eq!(runs.hosts.len(), 2);
    assert!(runs.hosts.iter().any(|host| host.repo == "team/mirrored"));
    let imported = desk
        .import(ImportJob {
            repo: &format!("{forge}/team/mirrored"),
            machine_id: Some(&f.build.id.to_string()),
            folder: None,
            parent: None,
            worktrees: &[],
        })
        .await
        .unwrap();
    assert_eq!(imported.project, "acme/widget");
    assert_eq!(desk.imported().await.len(), 2);
    let calls = requests.lock().unwrap().clone();
    assert_eq!(
        calls
            .iter()
            .filter(|line| line.starts_with("PATCH /repos/"))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|line| line.starts_with("PATCH /forge/"))
            .count(),
        2
    );
    let forged = [RepositoryRename {
        url: "https://github.com/other/widget".into(),
        name: "renamed".into(),
    }];
    assert!(
        desk.rename_project("acme/widget", "Unwanted", &forged)
            .await
            .is_err()
    );
    assert_eq!(requests.lock().unwrap().len(), calls.len());
    server.abort();
}

#[tokio::test]
async fn project_sync_keeps_unset_projects_out_of_syncthing() {
    use beep_proto::node::mesh_state::ProjectRecord;
    let studio = create("studio").await;
    let desk = desk(&studio, &[]).await;
    for (name, syncthing_files) in [("example/unset", None), ("example/kept", Some(true))] {
        let record = ProjectRecord {
            url: format!("https://example.com/{name}"),
            syncthing_files,
            ..ProjectRecord::default()
        };
        studio
            .mesh
            .set(
                Key::Project(name.to_owned()),
                Some(record.to_bytes().unwrap()),
            )
            .await
            .unwrap();
    }

    assert_eq!(
        desk.project_syncthing_mode("example/unset", None)
            .await
            .unwrap(),
        Some(false)
    );
    assert_eq!(
        desk.project_syncthing_mode("example/kept", None)
            .await
            .unwrap(),
        Some(true)
    );

    desk.set_project_sync_enabled(false).await.unwrap();

    assert_eq!(
        desk.project_syncthing_mode("example/unset", None)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_named_agent_account_sits_beside_the_default_one() {
    let f = fleet().await;
    let studio = desk(&f.studio, &[]).await;
    studio
        .save_agent_token("claude", None, &SecretText::new("sk-ant-oat-personal-1111"))
        .await
        .expect("the default token is saved");
    studio
        .save_agent_token(
            "claude",
            Some("Work"),
            &SecretText::new("sk-ant-oat-work-2222"),
        )
        .await
        .expect("the work token is saved");
    studio
        .save_agent_token(
            "codex",
            Some("Work"),
            &SecretText::new("at-work-token-3333"),
        )
        .await
        .expect("a ChatGPT access token is saved");

    // A node reads the named ones as agent logins, apart from the env vars every process gets.
    until(&f.build.mesh, |m| m.scan("credential/").len() == 3).await;
    let mut logins: Vec<(String, String, String)> = f
        .build
        .mesh
        .scan("credential/")
        .iter()
        .filter_map(
            |(_, bytes)| match Credential::from_bytes(bytes).expect("a node reads it") {
                Credential::Agent(login) => Some((
                    login.agent.as_ref().to_owned(),
                    login.name.to_string(),
                    login.env.name,
                )),
                _ => None,
            },
        )
        .collect();
    logins.sort();
    assert_eq!(
        logins,
        [
            (
                "claude".into(),
                "Work".into(),
                "CLAUDE_CODE_OAUTH_TOKEN".into()
            ),
            ("codex".into(), "Work".into(), "CODEX_ACCESS_TOKEN".into()),
        ]
    );

    let accounts = studio.agent_accounts().await.expect("it lists");
    assert_eq!(
        accounts.claude.map(|token| token.last4).as_deref(),
        Some("1111")
    );
    let named: Vec<_> = accounts
        .named
        .iter()
        .map(|named| {
            (
                named.agent.as_ref(),
                named.name.as_ref(),
                named.token.last4.as_str(),
            )
        })
        .collect();
    assert_eq!(
        named,
        [("claude", "Work", "2222"), ("codex", "Work", "3333")]
    );

    studio
        .revoke_agent_token("claude", Some("work"))
        .await
        .expect("the work account goes, whatever its case");
    let accounts = studio.agent_accounts().await.expect("it lists");
    assert!(accounts.claude.is_some());
    assert_eq!(accounts.named.len(), 1);
    assert!(
        studio
            .save_agent_token("claude", Some("default"), &SecretText::new("sk-ant-oat-x"))
            .await
            .is_err()
    );
    // A Codex API key would lose to the machine's own sign-in.
    assert!(
        studio
            .save_agent_token(
                "codex",
                Some("Home"),
                &SecretText::new("sk-proj-example-1234")
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_change_feed_goes_to_one_follower_and_keeps_the_order_reads_finish() {
    use beep_fleet::{Changed, ProjectRead};

    let f = fleet().await;
    let node = widget_host();
    let (_peers, session, mut changes) = local_project(&f, &node, Duration::ZERO).await;
    assert!(session.changes().is_none(), "the feed was already taken");

    session
        .lists("acme/widget", false)
        .await
        .expect("a cold read");
    session
        .lists("acme/widget", false)
        .await
        .expect("a stale read");
    session.settled().await;
    session
        .runs("acme/widget", false)
        .await
        .expect("a cold read");
    session
        .runs("acme/widget", false)
        .await
        .expect("a stale read");
    session.settled().await;
    session.spend(false).await.expect("a cold spend");
    session.settled().await;
    // The same spend read again is not news. A node that spent more since is.
    session.spend(false).await.expect("a stale spend");
    session.settled().await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    node.later_usage
        .lock()
        .expect("later usage")
        .push(SessionsReply::SessionSpend {
            usage: SessionSpend {
                agent: "claude".into(),
                session: "later".into(),
                cwd: "/home/user/widget".into(),
                since: 0,
                historical: Vec::new(),
                hours: vec![HourSpend {
                    hour: now - now % 3600,
                    model: "claude-opus-4-1".into(),
                    input_tokens: 10,
                    cost_usd: 1.0,
                    billing: beep_proto::node::usage::Billing::Metered,
                    ..HourSpend::default()
                }],
            },
        });
    session.spend(false).await.expect("a stale spend");
    session.settled().await;

    let mut said = Vec::new();
    while let Ok(changed) = changes.try_recv() {
        said.push(changed);
    }
    let reads: Vec<_> = said
        .iter()
        .map(|changed| match changed {
            Changed::Project {
                project,
                read: ProjectRead::Lists(_),
            } => format!("lists {project}"),
            Changed::Project {
                project,
                read: ProjectRead::Runs(_),
            } => format!("runs {project}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(reads, ["lists acme/widget", "runs acme/widget", "Usage"]);
}
