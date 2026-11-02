//! Workers started on real `beep-node`s over iroh, in a real git checkout,
//! with a scripted stand-in for the coding agent.

use super::{
    model::{
        InputFile, IssueHost, MachinePath, RepoName, Roles, StartRequest, TaskState, Visibility,
        WorkerState,
    },
    turn::{REPLAN_LABEL, WORKER_LABEL},
    worker::{Started, start_task, stop_worker},
};
use crate::protocol::node::{
    LinkRx, LinkTx,
    agent::{
        Event as AgentEvent, Request as AgentRequest, SessionInfo, State as AgentState,
        Status as AgentStatus,
    },
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
    link::{Offers, Protocol, Session as LinkSession},
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
        env: Default::default(),
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
    key: SecretKey,
    name: &'static str,
    online: Online,
    tmp: PathBuf,
    widget: PathBuf,
    _dir: TempDir,
    task: JoinHandle<anyhow::Result<()>>,
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
            provision: None,
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
            key,
            name,
            online,
            tmp,
            widget,
            _dir: dir,
            task,
        }
    }

    /// Drops `node`'s connection and connects it again offering `offers`,
    /// as a node restarted with other sessions would. Waits until the server
    /// has the new connection.
    async fn reconnect(&self, node: &mut TestNode, offers: Offers) {
        node.task.abort();
        let old = node.online.connection.stable_id();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(node.key.clone())
            .bind()
            .await
            .expect("the node endpoint binds");
        let addr = self.router.endpoint().addr();
        let name = node.name;
        node.task =
            tokio::spawn(
                async move { beep_node::link::connect(&endpoint, addr, name, &offers).await },
            );
        let app = &self.harness.app;
        let id = node.id.to_string();
        node.online = timeout(LIMIT, async {
            loop {
                if let Some(online) = app.nodes.online(&id)
                    && online.connection.stable_id() != old
                {
                    return online;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the node came back");
    }

    /// The project with `nodes`' clones as its checkouts, the first home.
    async fn project(&self, nodes: &[&TestNode]) -> ProjectRow {
        let roles = Roles {
            issues: None,
            prs: None,
        };
        self.project_with(nodes, &roles).await
    }

    async fn project_with(&self, nodes: &[&TestNode], roles: &Roles) -> ProjectRow {
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
        alice
            .set_project_setup(project.id, roles, Visibility::Private, "main")
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
        base: None,
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
    // The echo agent can finish its first turn before the worker is read.
    assert!(
        matches!(worker.state, WorkerState::Running | WorkerState::Waiting),
        "{:?}",
        worker.state
    );
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
async fn a_fix_starts_from_a_branch_not_pushed_yet() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    git(&node.widget, &["checkout", "-b", "login-copy"]);
    fs::write(node.widget.join("copy.txt"), "Sign in\n").expect("a file");
    git(&node.widget, &["add", "copy.txt"]);
    git(&node.widget, &["commit", "-m", "Copy"]);
    git(&node.widget, &["checkout", "main"]);
    let local = git_out(&node.widget, &["rev-parse", "login-copy"]);
    let tasks = world.tasks(&project, &[new_task("Fix it")]).await;
    let request = StartRequest {
        base: Some("login-copy".to_owned()),
        ..claude()
    };
    let Started::Running(_) = world.start(&project, &tasks[0], &request).await else {
        panic!("the worker did not start");
    };
    let worktree = node.widget.join(".worktrees/fix-it");
    assert_eq!(git_out(&worktree, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        git_out(&worktree, &["branch", "--show-current"]),
        "beep/fix-it"
    );
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

/// The labels and texts posted to project chats once `done` holds.
async fn posted_until(
    world: &World,
    done: impl Fn(&[(String, String)]) -> bool,
) -> Vec<(String, String)> {
    timeout(LIMIT, world.harness.app.projects.posted_until(done))
        .await
        .expect("the reports came")
}

fn count(posted: &[(String, String)], label: &str, holds: &str) -> usize {
    posted
        .iter()
        .filter(|(posted, text)| posted == label && text.contains(holds))
        .count()
}

/// A stand-in `gh` in `tmp/bin` that lists one open pull request.
fn fake_gh(tmp: &Path) -> PathBuf {
    let bin = tmp.join("bin");
    fs::create_dir_all(&bin).expect("a folder");
    let gh = bin.join("gh");
    let pr = r#"[{"number":7,"url":"https://example.com/acme/widget/pull/7","state":"OPEN","title":"Spin it","body":"Spins the widget."}]"#;
    let script =
        format!("#!/bin/sh\ncase \"$1 $2\" in\n\"pr list\") echo '{pr}' ;;\n*) exit 1 ;;\nesac\n");
    fs::write(&gh, script).expect("the stand-in");
    let made = Command::new("chmod")
        .arg("+x")
        .arg(&gh)
        .status()
        .expect("chmod runs");
    assert!(made.success());
    bin
}

#[tokio::test]
async fn turn_done_moves_to_review_and_reports() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let bin = fake_gh(&node.tmp);
    world
        .harness
        .app
        .projects
        .set_shell_prefix(&format!("PATH={}:\"$PATH\"; export PATH", bin.display()));
    let roles = Roles {
        issues: None,
        prs: Some(IssueHost::Github),
    };
    let project = world.project_with(&[&node], &roles).await;
    let tasks = world.tasks(&project, &[new_task("Spin it")]).await;
    let Started::Running(worker) = world.start(&project, &tasks[0], &claude()).await else {
        panic!("the worker did not start");
    };
    let posted = posted_until(&world, |posted| {
        count(posted, WORKER_LABEL, "finished turn 1") > 0
    })
    .await;
    assert_eq!(
        count(&posted, WORKER_LABEL, "finished turn 1"),
        1,
        "{posted:?}"
    );
    let (_, report) = &posted[0];
    assert!(
        report.contains("Pull request: https://example.com/acme/widget/pull/7"),
        "{report}"
    );
    assert!(
        report.contains("Echo: You are working on one task"),
        "{report}"
    );
    let task = world.task(&tasks[0]).await;
    assert_eq!(task.state, TaskState::Review);
    assert_eq!(
        task.pr_url.as_deref(),
        Some("https://example.com/acme/widget/pull/7")
    );
    let worker = world
        .harness
        .alice
        .project_worker(worker.id)
        .await
        .expect("the worker")
        .expect("it exists");
    assert_eq!(worker.state, WorkerState::Waiting);
    assert_eq!(worker.turns_seen, 1);
}

#[tokio::test]
async fn inside_edits_are_answered_without_the_user() {
    let world = World::new().await;
    let node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    let task = NewTask {
        body: "Please edit notes.txt.".to_owned(),
        ..new_task("Spin it")
    };
    let tasks = world.tasks(&project, &[task]).await;
    let Started::Running(_) = world.start(&project, &tasks[0], &claude()).await else {
        panic!("the worker did not start");
    };
    // Nothing waits on the session, so its turn comes back as a report.
    let posted = posted_until(&world, |posted| {
        count(posted, WORKER_LABEL, "finished turn 1") > 0
    })
    .await;
    assert_eq!(
        count(&posted, WORKER_LABEL, "Its last message:\nEdited notes.txt"),
        1,
        "{posted:?}"
    );
    assert!(
        world
            .harness
            .app
            .subagents
            .approvals
            .pending_all("alice")
            .is_empty(),
        "a card was shown"
    );
    assert_eq!(count(&posted, WORKER_LABEL, "asks"), 0, "{posted:?}");
}

/// A `claude/1` that only has the session `kept`, which finished a turn
/// while the server was away.
struct Restarted {
    kept: SessionInfo,
}

impl Protocol for Restarted {
    fn name(&self) -> &str {
        "claude/1"
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> LinkSession {
        let kept = self.kept.clone();
        Box::pin(async move {
            let Some(AgentRequest::Attach { session }) = rx.recv::<AgentRequest>().await? else {
                return Ok(());
            };
            if session != kept.id {
                let reason = format!("there is no session {session}");
                return tx.send(&AgentEvent::Refused { reason }).await;
            }
            tx.send(&AgentEvent::Attached {
                session: Box::new(kept),
            })
            .await?;
            while rx.recv::<AgentRequest>().await?.is_some() {
                tx.send(&AgentEvent::Ack { error: None }).await?;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn reconnect_reattaches_and_fails_lost_sessions() {
    let world = World::new().await;
    let mut node = world.node("build box").await;
    let project = world.project(&[&node]).await;
    let tasks = world
        .tasks(&project, &[new_task("Kept"), new_task("Lost")])
        .await;
    let mut workers = Vec::new();
    for task in &tasks {
        let Started::Running(worker) = world.start(&project, task, &claude()).await else {
            panic!("the worker did not start");
        };
        workers.push(*worker);
    }
    posted_until(&world, |posted| {
        count(posted, WORKER_LABEL, "finished turn 1") == 2
    })
    .await;
    let kept_session = workers[0].session.clone().expect("a session");
    let lost_session = workers[1].session.clone().expect("a session");
    let kept = SessionInfo {
        id: kept_session.clone(),
        agent: "claude".into(),
        cwd: workers[0].folder.to_string(),
        state: AgentState {
            status: AgentStatus::Idle,
            ..AgentState::default()
        },
        last_message: "Finished while away.".into(),
        turns: 2,
        ..SessionInfo::default()
    };
    let offers = Offers {
        agents: Arc::default(),
        extra: vec![Arc::new(Restarted { kept }) as Arc<dyn Protocol>],
        provision: None,
    };
    world.reconnect(&mut node, offers).await;
    let posted = posted_until(&world, |posted| {
        count(posted, REPLAN_LABEL, "is gone") == 1
            && count(posted, WORKER_LABEL, "Finished while away.") == 1
    })
    .await;
    assert_eq!(
        count(
            &posted,
            REPLAN_LABEL,
            "The worker for lost on build-box is gone"
        ),
        1,
        "{posted:?}"
    );
    let alice = &world.harness.alice;
    let lost = alice
        .project_worker(workers[1].id)
        .await
        .expect("the worker")
        .expect("it exists");
    assert_eq!(lost.state, WorkerState::Failed);
    assert_eq!(world.task(&tasks[1]).await.state, TaskState::Blocked);
    let app = &world.harness.app;
    assert_eq!(app.projects.worker_of(&node.id, &lost_session), None);
    assert_eq!(
        app.projects.worker_of(&node.id, &kept_session),
        Some(workers[0].id)
    );
    let kept = alice
        .project_worker(workers[0].id)
        .await
        .expect("the worker")
        .expect("it exists");
    assert_eq!(kept.state, WorkerState::Waiting);
    assert_eq!(kept.turns_seen, 2);
}

#[tokio::test]
async fn add_branch_worktree_runs_on_chosen_node() {
    let world = World::new().await;
    let build = world.node("build box").await;
    let render = world.node("render box").await;
    // A branch only on render box's origin, not in its checkout.
    git(&render.widget, &["checkout", "-b", "beep/spike"]);
    git(&render.widget, &["commit", "--allow-empty", "-m", "Spike"]);
    git(&render.widget, &["push", "origin", "beep/spike"]);
    git(&render.widget, &["checkout", "main"]);
    git(&render.widget, &["branch", "-D", "beep/spike"]);
    let folder = render.widget.to_string_lossy().into_owned();
    let added = super::branches::add_worktree(&render.online, &folder, "beep/spike", "main")
        .await
        .expect("the worktree is added");
    let worktree = render.widget.join(".worktrees/beep-spike");
    assert_eq!(added.name, "beep/spike");
    assert_eq!(
        added.worktree.as_deref(),
        Some(worktree.to_string_lossy().as_ref())
    );
    assert_eq!(added.ahead, 1);
    assert_eq!(
        git_out(&worktree, &["branch", "--show-current"]),
        "beep/spike"
    );
    assert!(!build.widget.join(".worktrees").exists());
    let again = super::branches::add_worktree(&render.online, &folder, "beep/spike", "main").await;
    assert_eq!(
        again.expect_err("a second worktree is refused"),
        "beep/spike already has a worktree there."
    );
}
