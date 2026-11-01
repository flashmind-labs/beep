//! Workers started on real `beep-node`s over iroh, in a real git checkout,
//! with a scripted stand-in for the coding agent.

use super::{
    model::{
        InputFile, MachinePath, RepoName, Roles, StartRequest, TaskState, Visibility, WorkerState,
    },
    worker::{Started, start_task, stop_worker},
};
use crate::{
    nodes::{self, Online},
    protocol::DeviceKind,
    store::{After, CheckoutRow, NewKey, NewTask, ProjectRow, TaskRow},
    tools::{SendFileTool, machines::Machines},
    watchers::testing::{Harness, Scripted},
};
use beep_node::{
    agents::{AgentKind, Agents},
    files::Folders,
    link::{Offers, Protocol},
};
use flashmind::types::tool::{Tool, ToolContext};
use iroh::{Endpoint, EndpointId, SecretKey, endpoint::presets, protocol::Router};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{task::JoinHandle, time::timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const LIMIT: Duration = Duration::from_secs(30);

/// The stand-in agent, offered as `claude`.
fn fake_claude() -> AgentKind {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../node/testdata/fake_acp.py");
    AgentKind {
        name: "claude".into(),
        program: "python3".into(),
        args: vec!["-I".into(), script.into()],
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A bare `origin.git` in `tmp` and a clone of it at `tmp/widget` whose
/// origin reads as a hosted repository.
fn repo(tmp: &Path) -> PathBuf {
    let origin = tmp.join("origin.git");
    git(tmp, &["init", "--bare", "-b", "main", "origin.git"]);
    let widget = tmp.join("widget");
    git(tmp, &["clone", "origin.git", "widget"]);
    git(&widget, &["checkout", "-b", "main"]);
    fs::write(widget.join("README.md"), "widget\n").expect("a file");
    git(&widget, &["add", "README.md"]);
    git(&widget, &["commit", "-m", "First"]);
    git(&widget, &["push", "origin", "main"]);
    git(
        &widget,
        &[
            "config",
            "remote.origin.url",
            "https://example.com/acme/widget.git",
        ],
    );
    git(
        &widget,
        &[
            "config",
            &format!("url.{}.insteadOf", origin.display()),
            "https://example.com/acme/widget.git",
        ],
    );
    widget
}

/// A node serving a shell, the folder `tmp` and the stand-in agent.
struct TestNode {
    id: EndpointId,
    online: Online,
    tmp: PathBuf,
    widget: PathBuf,
    _dir: TempDir,
    _task: JoinHandle<anyhow::Result<()>>,
}

struct World {
    harness: Harness,
    router: Router,
}

impl World {
    async fn new() -> Self {
        let harness = Harness::new(Scripted::default()).await;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::generate())
            .bind()
            .await
            .expect("the server endpoint binds");
        let handler = nodes::NodeHandler::new(harness.app.clone(), endpoint.id());
        let router = Router::builder(endpoint)
            .accept(nodes::NODE_ALPN, handler)
            .spawn();
        Self { harness, router }
    }

    async fn node(&self, name: &'static str) -> TestNode {
        let key = SecretKey::generate();
        self.harness
            .alice
            .link_key(NewKey {
                endpoint: key.public(),
                name,
                kind: DeviceKind::Desktop {},
            })
            .await
            .expect("the key links");
        let dir = TempDir::new().expect("a temp folder");
        let tmp = dir.path().canonicalize().expect("the temp folder exists");
        let widget = repo(&tmp);
        fs::create_dir_all(tmp.join("in")).expect("a folder");
        let folders = Folders::new(std::slice::from_ref(&tmp));
        let offers = Offers {
            agents: Arc::new(Agents::new(vec![fake_claude()])),
            extra: vec![Arc::new(folders) as Arc<dyn Protocol>],
        };
        let node = Endpoint::builder(presets::N0)
            .secret_key(key.clone())
            .bind()
            .await
            .expect("the node endpoint binds");
        let addr = self.router.endpoint().addr();
        let task =
            tokio::spawn(async move { beep_node::link::connect(&node, addr, name, &offers).await });
        let id = key.public();
        let app = &self.harness.app;
        let alice = &self.harness.alice;
        // Registering and the machine's report are the server's own
        // business, with no signal to wait on, so this looks until both
        // are there.
        let online = timeout(LIMIT, async {
            loop {
                let reported = alice
                    .nodes()
                    .await
                    .expect("the nodes")
                    .into_iter()
                    .any(|row| row.endpoint == id.to_string() && row.env.is_some());
                if reported && let Some(online) = app.nodes.online(&id.to_string()) {
                    return online;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the node came online");
        assert!(online.offers("claude/1"), "{:?}", online.offers);
        TestNode {
            id,
            online,
            tmp,
            widget,
            _dir: dir,
            _task: task,
        }
    }

    /// The project with `nodes`' clones as its checkouts, the first home.
    async fn project(&self, nodes: &[&TestNode]) -> ProjectRow {
        let alice = &self.harness.alice;
        let project = alice
            .ensure_project(&RepoName::parse("acme/widget").expect("a repo"))
            .await
            .expect("the project");
        for (n, node) in nodes.iter().enumerate() {
            alice
                .add_checkout(
                    project.id,
                    &CheckoutRow {
                        machine: node.id,
                        path: path(&node.widget),
                        home: n == 0,
                    },
                )
                .await
                .expect("the checkout");
        }
        let roles = Roles {
            issues: None,
            prs: None,
        };
        alice
            .set_project_setup(project.id, &roles, Visibility::Private, "main")
            .await
            .expect("the setup");
        alice
            .project(project.id)
            .await
            .expect("the project")
            .expect("it exists")
    }

    async fn tasks(&self, project: &ProjectRow, tasks: &[NewTask]) -> Vec<TaskRow> {
        self.harness
            .alice
            .create_tasks(project.id, tasks)
            .await
            .expect("the tasks")
    }

    async fn task(&self, task: &TaskRow) -> TaskRow {
        self.harness
            .alice
            .project_task(task.id)
            .await
            .expect("the task")
            .expect("it exists")
    }

    async fn start(&self, project: &ProjectRow, task: &TaskRow, request: &StartRequest) -> Started {
        let task = self.task(task).await;
        start_task(
            &self.harness.app,
            &self.harness.alice,
            project,
            &task,
            request,
        )
        .await
        .expect("the start works out")
    }
}

fn path(dir: &Path) -> MachinePath {
    MachinePath::parse(&dir.to_string_lossy()).expect("an absolute path")
}

fn new_task(title: &str) -> NewTask {
    NewTask {
        title: title.to_owned(),
        body: "Make the widget spin.".to_owned(),
        depends_on: None,
        needs: Vec::new(),
        start: None,
    }
}

fn claude() -> StartRequest {
    StartRequest {
        machine: None,
        agent: Some(super::model::WorkerAgent::Claude),
        persona: None,
        inputs: Vec::new(),
    }
}

/// The text the stand-in agent echoed for the session's first prompt.
async fn first_reply(online: &Online, session: &str) -> String {
    let link = online
        .agent_session("claude", session)
        .await
        .expect("the session attaches");
    let mut seen = link.now();
    timeout(LIMIT, async {
        while seen.info.turns == 0 {
            seen = link.wait(seen.mark(), LIMIT).await;
        }
    })
    .await
    .expect("the first turn ended");
    seen.info.last_message
}

#[tokio::test]
async fn a_worker_starts_in_its_own_worktree() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    let tasks = world.tasks(&project, &[new_task("Spin it")]).await;
    let Started::Running(worker) = world.start(&project, &tasks[0], &claude()).await else {
        panic!("the worker did not start");
    };
    assert_eq!(worker.state, WorkerState::Running);
    let session = worker.session.clone().expect("a session");
    let worktree = node.widget.join(".worktrees/spin-it");
    assert_eq!(worker.folder, path(&worktree));
    assert_eq!(
        git_out(&worktree, &["branch", "--show-current"]),
        "beep/spin-it"
    );
    let exclude = fs::read_to_string(node.widget.join(".git/info/exclude")).expect("the exclude");
    assert!(
        exclude.lines().any(|line| line == ".worktrees/"),
        "{exclude}"
    );
    assert!(exclude.lines().any(|line| line == ".beep/"), "{exclude}");
    let task = world.task(&tasks[0]).await;
    assert_eq!(task.state, TaskState::Running);
    assert_eq!(task.branch.as_deref(), Some("beep/spin-it"));
    assert_eq!(task.start, None);
    assert_eq!(
        world.harness.app.projects.worker_of(&node.id, &session),
        Some(worker.id)
    );
    let reply = first_reply(&node.online, &session).await;
    assert!(
        reply.starts_with("Echo: You are working on one task"),
        "{reply}"
    );
    assert!(reply.contains("Make the widget spin."), "{reply}");
}

#[tokio::test]
async fn inputs_land_in_incoming() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    let tasks = world.tasks(&project, &[new_task("Spin it")]).await;
    let model = node.tmp.join("in/model.glb");
    fs::write(&model, "a model").expect("the input");
    let request = StartRequest {
        inputs: vec![InputFile {
            machine: None,
            path: model.to_string_lossy().into_owned(),
        }],
        ..claude()
    };
    let Started::Running(worker) = world.start(&project, &tasks[0], &request).await else {
        panic!("the worker did not start");
    };
    let landed = node
        .widget
        .join(".worktrees/spin-it/.beep/incoming/spin-it/model.glb");
    assert_eq!(
        fs::read_to_string(&landed).expect("the input landed"),
        "a model"
    );
    let reply = first_reply(&node.online, &worker.session.expect("a session")).await;
    assert!(
        reply.contains("The input files are in .beep/incoming/spin-it/ in the worktree."),
        "{reply}"
    );
}

#[tokio::test]
async fn a_fourth_start_queues() {
    let world = World::new().await;
    let (build, render) = (
        world.node("build box").await,
        world.node("render box").await,
    );
    let project = world.project(&[&build, &render]).await;
    let made = world
        .tasks(
            &project,
            &[
                new_task("One"),
                new_task("Two"),
                new_task("Three"),
                new_task("Four"),
            ],
        )
        .await;
    for task in &made[..3] {
        let started = world.start(&project, task, &claude()).await;
        assert!(matches!(started, Started::Running(_)), "{started:?}");
    }
    let Started::Queued(why) = world.start(&project, &made[3], &claude()).await else {
        panic!("a fourth worker started");
    };
    assert!(why.contains("already run"), "{why}");
    let fourth = world.task(&made[3]).await;
    assert_eq!(fourth.start, Some(claude()));
    assert_eq!(fourth.state, TaskState::Planned);
}

#[tokio::test]
async fn a_dependent_task_waits() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    let made = world
        .tasks(
            &project,
            &[
                new_task("A"),
                NewTask {
                    depends_on: Some(After::Earlier(0)),
                    ..new_task("B")
                },
            ],
        )
        .await;
    let Started::Queued(why) = world.start(&project, &made[1], &claude()).await else {
        panic!("b started before a");
    };
    assert_eq!(why, "waits for a");
    let b = world.task(&made[1]).await;
    assert_eq!(b.state, TaskState::Blocked);
    assert_eq!(b.start, Some(claude()));
    assert!(!node.widget.join(".worktrees/b").exists());
}

#[tokio::test]
async fn stop_closes_and_blocks() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    let tasks = world.tasks(&project, &[new_task("Spin it")]).await;
    let Started::Running(worker) = world.start(&project, &tasks[0], &claude()).await else {
        panic!("the worker did not start");
    };
    let session = worker.session.clone().expect("a session");
    first_reply(&node.online, &session).await;
    let (app, alice) = (&world.harness.app, &world.harness.alice);
    stop_worker(app, alice, &worker).await.expect("it stops");
    let stopped = alice
        .project_worker(worker.id)
        .await
        .expect("the worker")
        .expect("it exists");
    assert_eq!(stopped.state, WorkerState::Ended);
    assert_eq!(world.task(&tasks[0]).await.state, TaskState::Blocked);
    assert_eq!(app.projects.worker_of(&node.id, &session), None);
    let sessions = node.online.agent_sessions().await.expect("the sessions");
    // The node keeps an ended session's record, closed.
    assert!(
        sessions
            .iter()
            .filter(|info| info.id == session)
            .all(|info| info.state.status == crate::protocol::node::agent::Status::Ended),
        "{sessions:?}"
    );
    // Stopping again changes nothing.
    stop_worker(app, alice, &worker)
        .await
        .expect("it stops again");
    assert_eq!(world.task(&tasks[0]).await.state, TaskState::Blocked);
}

#[tokio::test]
async fn copy_outside_the_checkout_is_refused() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    let alice = &world.harness.alice;
    let checkouts = alice.checkouts(project.id).await.expect("the checkouts");
    let send = SendFileTool {
        user: alice.clone(),
        session_id: Uuid::new_v4(),
        workspace: None,
        machines: Some(Machines {
            app: world.harness.app.clone(),
            user: alice.clone(),
        }),
        paper: false,
        copy_into: Some(
            checkouts
                .into_iter()
                .map(|checkout| (checkout.machine, checkout.path))
                .collect(),
        ),
    };
    fs::write(node.tmp.join("in/model.glb"), "a model").expect("the input");
    fs::create_dir_all(node.tmp.join("out")).expect("a folder");
    let cancel = CancellationToken::new();
    let args = json!({
        "path": node.tmp.join("in/model.glb").to_string_lossy(),
        "from": "build-box",
        "to": "build-box",
        "dst_folder": node.tmp.join("out").to_string_lossy(),
    });
    let result = send
        .execute(ToolContext::new("copy", args, None, &cancel))
        .await
        .expect("a result");
    assert!(!result.is_success(), "{}", result.output());
    assert!(result.output().contains("outside"), "{}", result.output());
    assert!(!node.tmp.join("out/model.glb").exists());
}
