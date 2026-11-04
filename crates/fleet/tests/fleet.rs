//! Desktops and machines on loopback, relays off. A desktop calls a machine through a fake node
//! that answers `status/1`, `shell/1` and `agents/1`; the desktops and the machines are real
//! members of one mesh. Waits ride on `updates()` and outer timeouts; nothing sleeps.

use async_trait::async_trait;
use beep_fleet::{Desk, FleetError, ImportJob, Peers, project_id};
use beep_mesh::{
    DeviceKind, JOIN_ALPN, Key, Mesh, PEER_ALPN, PeerClient, SessionFn, transport_config,
};
use beep_proto::{
    CredentialKind, SecretText, WorkDiff,
    node::{
        EnvReport, LinkRx, LinkTx, Open, Opened, ToolInfo,
        agent::{DiffAsk, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
        mesh_state::{Credential, StateValue},
        shell::{self, Event, Request},
        status::Report,
        usage::{HourSpend, PlanUsage, SessionSpend},
    },
};
use iroh::{Endpoint, EndpointId, SecretKey, endpoint::presets, protocol::Router};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
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
            updates.borrow_and_update();
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
    /// Commands the shell ran.
    ran: Mutex<Vec<String>>,
    /// What a command prints, by a word it contains.
    prints: Vec<(&'static str, &'static str)>,
    /// What `FollowUsage` says.
    usage: Vec<SessionsReply>,
    /// The tools its `status/1` reports.
    tools: Vec<&'static str>,
    /// The diffs it was asked for.
    diffed: Mutex<Vec<DiffAsk>>,
}

impl FakeNode {
    fn session(self: &Arc<Self>) -> SessionFn {
        let node = self.clone();
        Arc::new(move |tx, rx| {
            let node = node.clone();
            Box::pin(async move { node.serve(tx, rx).await })
        })
    }

    fn prints_for(&self, command: &str) -> &'static str {
        self.prints
            .iter()
            .find(|(word, _)| command.contains(word))
            .map_or("BEEP_IMPORT: cloned\n", |(_, out)| *out)
    }

    async fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) {
        let Ok(Some(open)) = rx.recv::<Open>().await else {
            return;
        };
        if tx.send(&Opened::Ready {}).await.is_err() {
            return;
        }
        match open.protocol.as_str() {
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
                    let text = self.prints_for(&command).to_owned();
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
                let Ok(Some(SessionsRequest::FollowUsage { .. })) = first else {
                    return;
                };
                for reply in &self.usage {
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

/// The desktop's hold on the mesh: real links to the other desktops, and the fake nodes standing
/// in for the machines' own sessions.
struct Peering {
    mesh: Mesh,
    me: EndpointId,
    nodes: HashMap<EndpointId, SessionFn>,
    /// Machines that never answer, to show a call does not wait on them.
    stuck: Vec<EndpointId>,
    /// While set, the mesh is not there.
    broken: AtomicBool,
    /// The mesh never finishes opening.
    never: bool,
}

#[async_trait]
impl Peers for Peering {
    async fn mesh(&self) -> Result<Mesh, FleetError> {
        if self.never {
            std::future::pending::<()>().await;
        }
        if self.broken.load(Ordering::Relaxed) {
            return Err(FleetError::NoMesh("gone".into()));
        }
        Ok(self.mesh.clone())
    }

    fn me(&self) -> EndpointId {
        self.me
    }

    async fn client(&self, machine: EndpointId) -> Result<PeerClient, FleetError> {
        if self.stuck.contains(&machine) {
            std::future::pending::<()>().await;
        }
        Ok(match self.nodes.get(&machine) {
            Some(session) => PeerClient::local(session.clone()),
            None => PeerClient::over(self.mesh.clone(), machine),
        })
    }
}

fn peering(dev: &Dev, nodes: &[(&Dev, &Arc<FakeNode>)], stuck: &[&Dev]) -> Arc<Peering> {
    Arc::new(Peering {
        mesh: dev.mesh.clone(),
        me: dev.id,
        nodes: nodes
            .iter()
            .map(|(dev, node)| (dev.id, node.session()))
            .collect(),
        stuck: stuck.iter().map(|dev| dev.id).collect(),
        broken: AtomicBool::new(false),
        never: false,
    })
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
        .save_credential(&CredentialKind::GitHub {}, &SecretText::new(SECRET), true)
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
            hours: vec![HourSpend {
                hour: now - now % 3600,
                model: "claude-opus-4-1".into(),
                input_tokens: tokens,
                cost_usd: cost,
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
            at,
        },
    };
    let build = Arc::new(FakeNode {
        usage: vec![spend("a", 1.5, 100), plan(now - 60, 10.0)],
        ..FakeNode::default()
    });
    let mac = Arc::new(FakeNode {
        usage: vec![spend("b", 2.0, 50), plan(now, 40.0)],
        ..FakeNode::default()
    });
    let studio = desk(&f.studio, &[(&f.build, &build), (&second, &mac)])
        .await
        .settle(Duration::from_millis(300));
    let report = studio.usage_report().await.expect("usage reads");
    assert_eq!(report.today.total.cost_usd, 3.5);
    assert_eq!(report.today.tokens, 150);
    assert_eq!(report.week.total.cost_usd, 3.5);
    assert_eq!(report.today.harnesses.len(), 1);
    assert_eq!(report.today.harnesses[0].tokens, 150);
    // The freshest plan reading of an agent wins, and names the machine that read it.
    assert_eq!(report.limits.len(), 1);
    assert_eq!(report.limits[0].machine_name, "mac-box");
    assert_eq!(report.limits[0].windows[0].used_percent, 40.0);
    let view = studio.usage().await.expect("the view reads");
    assert_eq!(view.week.cost_usd, 3.5);
    let bill = studio.spend().await.expect("the bill reads");
    assert_eq!(bill.today.cost_usd, 3.5);
    assert_eq!(bill.today.metered_tokens, 150);
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
    let first = studio
        .save_instructions("- Write plainly.", None, "")
        .await
        .expect("instructions are saved");
    assert_eq!(first.version, 1);
    let stale = studio.save_instructions("again", None, "").await;
    assert!(stale.is_err(), "a save from an old base is refused");
    until(&f.laptop.mesh, |m| {
        !m.scan("instructions/").is_empty()
            && m.get(&Key::Harness("codex".into())).is_some()
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
    let config = beep_proto::node::harness::HarnessConfig::from_bytes(&bytes).expect("reads");
    assert_eq!(config.env[0].value.expose(), "proxy-secret-1");
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
        .save_credential(&CredentialKind::GitHub {}, &SecretText::new(SECRET), true)
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
            &CredentialKind::Forgejo {
                url: "http://127.0.0.1:9".into(),
            },
            &SecretText::new("forge-token-5521"),
            true,
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
        |_| {},
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
        .save_credential(&CredentialKind::GitHub {}, &SecretText::new(SECRET), true)
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
    let one = studio.run("acme/widget", 77).await.expect("the run reads");
    assert_eq!(one.jobs.len(), 1);
    assert!(one.run.progress.is_some(), "a running run shows its step");
}

/// The project `acme/widget` checked out on this Mac, whose host answers from `node`, and a
/// session on it that cannot reach the other machines at all.
async fn local_project(
    f: &Fleet,
    node: &Arc<FakeNode>,
    fresh: Duration,
) -> (Arc<Peering>, Arc<beep_fleet::Session>, Arc<AtomicUsize>) {
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
    let changed = Arc::new(AtomicUsize::new(0));
    let heard = changed.clone();
    let session = beep_fleet::Session::new(
        peers.clone(),
        beep_fleet::Options {
            fresh,
            ..beep_fleet::Options::default()
        },
        move |_| {
            heard.fetch_add(1, Ordering::Relaxed);
        },
    )
    .expect("a session");
    (peers, session, changed)
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
    let (peers, session, changed) = local_project(&f, &node, Duration::ZERO).await;
    session
        .lists("acme/widget", false)
        .await
        .expect("a cold read");
    // Stale at once; the mesh goes away, so the read behind it fails.
    peers.broken.store(true, Ordering::Relaxed);
    session
        .lists("acme/widget", false)
        .await
        .expect("the last read");
    session.settled().await;
    assert_eq!(changed.load(Ordering::Relaxed), 0, "a failure is not news");
    peers.broken.store(false, Ordering::Relaxed);
    session
        .lists("acme/widget", false)
        .await
        .expect("the last read");
    session.settled().await;
    assert_eq!(changed.load(Ordering::Relaxed), 1);
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
    let mut peers = Arc::try_unwrap(peering(&f.studio, &[], &[]))
        .ok()
        .expect("one owner");
    peers.never = true;
    let session = beep_fleet::Session::new(
        Arc::new(peers),
        beep_fleet::Options {
            mesh_wait: Duration::from_millis(100),
            ..beep_fleet::Options::default()
        },
        |_| {},
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

/// A server that lists one credential with no value, as a real one does.
struct LastFour;

#[async_trait]
impl beep_fleet::migrate::Source for LastFour {
    async fn account(&self) -> beep_fleet::Result<String> {
        Ok("account".into())
    }
    async fn imports(&self) -> beep_fleet::Result<Vec<beep_proto::ImportedProject>> {
        Ok(Vec::new())
    }
    async fn sync(&self) -> beep_fleet::Result<Vec<beep_proto::ProjectSync>> {
        Ok(Vec::new())
    }
    async fn groups(&self) -> beep_fleet::Result<Vec<beep_proto::ProjectGroup>> {
        Ok(Vec::new())
    }
    async fn on_top(&self) -> beep_fleet::Result<Vec<String>> {
        Ok(Vec::new())
    }
    async fn credentials(&self) -> beep_fleet::Result<Vec<beep_proto::Credential>> {
        Ok(vec![beep_proto::Credential {
            id: "7".into(),
            kind: CredentialKind::ApiKey {
                provider: "anthropic".into(),
            },
            last4: "9f3a".into(),
            account: None,
            env_var: "ANTHROPIC_API_KEY".into(),
            to_machines: true,
            created_at: SystemTime::UNIX_EPOCH,
            unreadable: false,
        }])
    }
    async fn harness(
        &self,
        agent: &str,
    ) -> beep_fleet::Result<Vec<beep_proto::MachineHarnessSettings>> {
        if agent != "codex" {
            return Ok(Vec::new());
        }
        Ok(vec![beep_proto::MachineHarnessSettings {
            machine_id: SecretKey::generate().public().to_string(),
            machine_name: "box".into(),
            settings: beep_proto::HarnessSettings {
                enabled: true,
                program: String::new(),
                home: String::new(),
                args: Vec::new(),
                env: vec![beep_proto::HarnessEnv {
                    name: "OPENAI_API_KEY".into(),
                    value: None,
                }],
            },
        }])
    }
    async fn flsh(&self) -> beep_fleet::Result<beep_proto::FlshSettings> {
        Ok(beep_proto::FlshSettings::default())
    }
    async fn instructions(&self) -> beep_fleet::Result<Vec<beep_proto::InstructionsVersion>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn a_moved_credential_lists_as_one_to_enter_again_until_it_is_saved() {
    let studio = create("Studio").await;
    beep_fleet::migrate::run(&studio.mesh, &LastFour)
        .await
        .expect("the account moves");
    let desk = desk(&studio, &[]).await;
    let listed = desk.credentials();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].unreadable);
    assert_eq!(listed[0].last4, "9f3a");
    assert!(matches!(listed[0].kind, CredentialKind::ApiKey { .. }));
    // Entering the token again drops its record, so nodes may take the mesh's value.
    assert_eq!(studio.mesh.scan("unmoved/credential/").len(), 1);
    desk.save_credential(
        &listed[0].kind,
        &SecretText::new("sk-ant-newtokenvalue"),
        true,
    )
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
