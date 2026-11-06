use super::{
    super::{gate::ProjectGate, issues, model::TaskState},
    *,
};
use crate::{
    account::user::User,
    apps::permissions::{ApprovalPayload, Rules},
    projects::testing::{FakeFleet, FakeShell},
    protocol::{Approval, PermissionLevel, node::Capability},
    store::{AppPermissionRow, CheckoutRow, NewTask, ProjectRow, TaskRow, TestDb},
};
use beep_projects::model::{IssueHost, MachinePath, Visibility};
use beep_projects::model::{RepoName, Roles};
use flashmind::{
    core::CancellationToken,
    types::tool::{Tool, ToolContext, ToolResult},
};
use iroh::EndpointId;
use serde_json::{Value, json};
use std::sync::Arc;

const ISSUE_URL: &str = "https://github.com/acme/widget/issues/";

struct Harness {
    _db: TestDb,
    user: Arc<User>,
    project: ProjectRow,
    fleet: Arc<FakeFleet>,
    shell: Arc<FakeShell>,
    home: EndpointId,
}

fn github() -> Roles {
    Roles {
        issues: Some(IssueHost::Github),
        prs: Some(IssueHost::Github),
    }
}

/// A project checked out at `/srv/widget` on `build-box`, its home.
async fn harness(roles: Roles, visibility: Visibility) -> Harness {
    let db = TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");
    let mut fleet = FakeFleet::default();
    let (home, shell) = fleet.add(1, "build-box", &[], "/srv");
    fleet.add(2, "gpu-box", &[Capability::Cuda], "/data");
    let project = user
        .ensure_project(&RepoName::parse("acme/widget").unwrap())
        .await
        .unwrap();
    user.add_checkout(
        project.id,
        &CheckoutRow {
            machine: home,
            path: MachinePath::parse("/srv/widget").unwrap(),
            home: true,
        },
    )
    .await
    .unwrap();
    user.set_project_setup(project.id, &roles, visibility, "main")
        .await
        .unwrap();
    let project = user.project(project.id).await.unwrap().unwrap();
    Harness {
        _db: db,
        user,
        project,
        fleet: Arc::new(fleet),
        shell,
        home,
    }
}

impl Harness {
    fn tools(&self, level: PermissionLevel, approved: Option<&str>) -> Arc<ProjectTools> {
        let rules = Rules::from_rows(vec![AppPermissionRow {
            app: "project".into(),
            level,
            allowed: Vec::new(),
        }]);
        Arc::new(ProjectTools {
            fleet: self.fleet.clone(),
            user: self.user.clone(),
            project: self.project.id,
            gate: Arc::new(ProjectGate::new(rules, approved.map(str::to_owned))),
        })
    }

    fn plan(&self, level: PermissionLevel) -> PlanTool {
        PlanTool {
            tools: self.tools(level, None),
        }
    }

    fn tasks_tool(&self, level: PermissionLevel, approved: Option<&str>) -> TasksTool {
        TasksTool {
            tools: self.tools(level, approved),
        }
    }

    async fn rows(&self) -> Vec<TaskRow> {
        self.user.project_tasks(self.project.id).await.unwrap()
    }
}

async fn call(tool: &dyn Tool, call_id: &str, args: Value) -> ToolResult {
    let cancel = CancellationToken::new();
    tool.execute(ToolContext::new(call_id, args, None, &cancel))
        .await
        .expect("the tool runs")
}

fn approval(result: &ToolResult) -> Approval {
    let payload = result.payload().expect("the call parks on a card");
    payload
        .as_any()
        .downcast_ref::<ApprovalPayload>()
        .expect("an approval card")
        .approval
        .clone()
}

fn plan_of(titles: &[&str]) -> Value {
    let tasks: Vec<Value> = titles
        .iter()
        .map(|title| json!({"title": title, "body": "Make it pass."}))
        .collect();
    json!({ "tasks": tasks })
}

#[tokio::test]
async fn three_tasks_show_a_plan_card() {
    let h = harness(github(), Visibility::Private).await;
    let args = json!({"tasks": [
        {"title": "Build", "body": "b"},
        {"title": "Render", "body": "r", "after": 1, "machine": "gpu-box"},
        {"title": "Ship", "body": "s", "after": 2},
    ]});
    let result = call(&h.plan(PermissionLevel::Allow {}), "c1", args).await;
    let card = approval(&result);
    assert_eq!(card.app, "Project");
    assert_eq!(card.action, "Create 3 tasks");
    let fields: Vec<(&str, &str)> = card
        .fields
        .iter()
        .map(|field| (field.label.as_str(), field.value.as_str()))
        .collect();
    assert_eq!(
        fields,
        [
            ("1. Build", "machine picked at start"),
            ("2. Render", "gpu-box, after 1"),
            ("3. Ship", "machine picked at start, after 2"),
        ]
    );
    assert!(h.shell.ran().is_empty());
    assert!(h.rows().await.is_empty());

    // Approved, the same call makes the tasks in order.
    let approved = PlanTool {
        tools: h.tools(PermissionLevel::Ask {}, Some("c1")),
    };
    h.shell
        .answer("issue create", 0, &format!("{ISSUE_URL}9\n"));
    let args = json!({"tasks": [
        {"title": "Build", "body": "b"},
        {"title": "Render", "body": "r", "after": 1},
        {"title": "Ship", "body": "s", "after": 2},
    ]});
    let result = call(&approved, "c1", args).await;
    assert!(result.is_success(), "{}", result.output());
    let rows = h.rows().await;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].depends_on, Some(rows[0].id));
    assert_eq!(rows[2].depends_on, Some(rows[1].id));
}

#[tokio::test]
async fn two_tasks_on_a_private_repo_follow_the_level() {
    let h = harness(github(), Visibility::Private).await;
    let asked = call(
        &h.plan(PermissionLevel::Ask {}),
        "c1",
        plan_of(&["Build", "Test"]),
    )
    .await;
    assert_eq!(approval(&asked).action, "Create 2 tasks");
    assert!(h.shell.ran().is_empty());

    h.shell.answer("Build", 0, &format!("{ISSUE_URL}11\n"));
    h.shell.answer("Test", 0, &format!("{ISSUE_URL}12\n"));
    let result = call(
        &h.plan(PermissionLevel::Allow {}),
        "c2",
        plan_of(&["Build", "Test"]),
    )
    .await;
    assert!(result.is_success(), "{}", result.output());
    let created = h.shell.ran_with("issue create");
    assert_eq!(created.len(), 2);
    assert!(
        created
            .iter()
            .all(|command| command.contains("-R acme/widget"))
    );
    let numbers: Vec<Option<u64>> = h
        .rows()
        .await
        .iter()
        .map(|task| task.issue.as_ref().map(|issue| issue.number))
        .collect();
    assert_eq!(numbers, [Some(11), Some(12)]);
    assert!(h.fleet.shells[&h.home].ran_with("issue create").len() == 2);
}

#[tokio::test]
async fn public_repo_issues_always_ask() {
    let h = harness(github(), Visibility::Public).await;
    let result = call(
        &h.plan(PermissionLevel::Allow {}),
        "c1",
        plan_of(&["Build"]),
    )
    .await;
    assert_eq!(approval(&result).action, "Create 1 task");
    assert!(h.shell.ran().is_empty());
}

#[tokio::test]
async fn leaky_issue_text_runs_nothing() {
    let h = harness(github(), Visibility::Private).await;
    let args = json!({"tasks": [{"title": "Build", "body": "Logs are in /home/sam/logs"}]});
    let result = call(&h.plan(PermissionLevel::Allow {}), "c1", args).await;
    assert!(!result.is_success());
    assert!(result.output().contains("/home/sam"), "{}", result.output());
    assert!(h.shell.ran().is_empty());
    assert!(h.rows().await.is_empty());
}

#[tokio::test]
async fn failed_issue_keeps_the_task() {
    let h = harness(github(), Visibility::Private).await;
    h.shell.answer("issue create", 1, "HTTP 502");
    let result = call(
        &h.plan(PermissionLevel::Allow {}),
        "c1",
        plan_of(&["Build"]),
    )
    .await;
    assert!(result.is_success(), "{}", result.output());
    assert!(result.output().contains("No issue yet for build"));
    let rows = h.rows().await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].issue.is_none());

    h.shell
        .answer("issue create", 0, &format!("{ISSUE_URL}4\n"));
    let filed = issues::mirror_missing(h.fleet.as_ref(), &h.user, &h.project)
        .await
        .unwrap();
    assert_eq!(filed, 1);
    assert_eq!(h.rows().await[0].issue.as_ref().unwrap().number, 4);
}

#[tokio::test]
async fn setup_rejects_unshared_checkout() {
    let h = harness(github(), Visibility::Private).await;
    let setup = SetupTool {
        tools: h.tools(PermissionLevel::Allow {}, None),
    };
    let args = json!({"action": "checkout", "in": "gpu-box", "path": "/srv/widget"});
    let result = call(&setup, "c1", args).await;
    assert!(!result.is_success());
    assert!(result.output().contains("not in a folder gpu-box shares"));
    assert!(h.fleet.shells.values().all(|shell| shell.ran().is_empty()));
    assert_eq!(h.user.checkouts(h.project.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn setup_reads_the_checkout() {
    let h = harness(Roles::default(), Visibility::Public).await;
    let gpu = h.fleet.machines[1].id;
    let shell = &h.fleet.shells[&gpu];
    shell.answer(
        "config --get-regexp",
        0,
        "== remotes\nremote.origin.url git@github.com:acme/widget.git\n== config\nissues=\n\
         prs=\n== head\norigin/trunk\n",
    );
    shell.answer("repo view", 0, r#"{"visibility": "PRIVATE"}"#);
    let setup = SetupTool {
        tools: h.tools(PermissionLevel::Allow {}, None),
    };
    let args = json!({"action": "checkout", "in": "gpu-box", "path": "/data/widget"});
    let result = call(&setup, "c1", args).await;
    assert!(result.is_success(), "{}", result.output());
    let project = h.user.project(h.project.id).await.unwrap().unwrap();
    assert_eq!(project.roles, github());
    assert_eq!(project.visibility, Visibility::Private);
    assert_eq!(project.default_branch, "trunk");
    assert_eq!(shell.ran_with("label create").len(), 1);
    let checkouts = h.user.checkouts(h.project.id).await.unwrap();
    // The first checkout stays the home one.
    assert_eq!(checkouts.iter().filter(|checkout| checkout.home).count(), 1);
    assert!(
        checkouts
            .iter()
            .any(|checkout| checkout.machine == gpu && !checkout.home)
    );
}

async fn make_tasks(h: &Harness, titles: &[&str]) -> Vec<TaskRow> {
    let tasks: Vec<NewTask> = titles
        .iter()
        .map(|title| NewTask {
            title: (*title).into(),
            body: String::new(),
            depends_on: None,
            needs: Vec::new(),
            start: None,
        })
        .collect();
    h.user.create_tasks(h.project.id, &tasks).await.unwrap()
}

#[tokio::test]
async fn always_is_saved_per_action() {
    let private = harness(github(), Visibility::Private).await;
    let project = &private.project;
    let tasks = |action: &str| json!({"action": action, "task": "build"});
    assert_eq!(
        saved_as("project_tasks", &tasks("drop"), project).as_deref(),
        Some("PROJECT_TASKS_DROP")
    );
    assert_eq!(
        saved_as("project_tasks", &tasks("update"), project).as_deref(),
        Some("PROJECT_TASKS_UPDATE")
    );
    assert_eq!(saved_as("project_tasks", &tasks("merge"), project), None);
    assert_eq!(
        saved_as("project_plan", &plan_of(&["Build"]), project).as_deref(),
        Some("PROJECT_PLAN")
    );
    assert_eq!(
        saved_as("project_plan", &plan_of(&["A", "B", "C"]), project),
        None
    );
    // Workers run without permission prompts, so every start asks.
    for agent in ["claude", "flsh"] {
        let start = json!({"action": "start", "task": "build", "agent": agent});
        assert_eq!(saved_as("project_worker", &start, project), None);
    }
    let public = harness(github(), Visibility::Public).await;
    assert_eq!(
        saved_as("project_plan", &plan_of(&["Build"]), &public.project),
        None
    );

    // Allowing drops always leaves updates asking.
    make_tasks(&private, &["Build"]).await;
    let rules = Rules::from_rows(vec![AppPermissionRow {
        app: "project".into(),
        level: PermissionLevel::Ask {},
        allowed: vec!["PROJECT_TASKS_DROP".into()],
    }]);
    let tool = TasksTool {
        tools: Arc::new(ProjectTools {
            gate: Arc::new(ProjectGate::new(rules, None)),
            ..(*private.tools(PermissionLevel::Ask {}, None)).clone()
        }),
    };
    let update = json!({"action": "update", "task": "build", "title": "Build it"});
    assert!(call(&tool, "c1", update).await.is_interrupt());
    let dropped = call(&tool, "c2", tasks("drop")).await;
    assert!(dropped.is_success(), "{}", dropped.output());
    assert_eq!(private.rows().await[0].state, TaskState::Dropped);
}

#[tokio::test]
async fn setup_checkout_asks_first() {
    let h = harness(github(), Visibility::Private).await;
    let setup = SetupTool {
        tools: h.tools(PermissionLevel::Ask {}, None),
    };
    let args = json!({"action": "checkout", "in": "gpu-box", "path": "/data/widget"});
    let parked = call(&setup, "c1", args).await;
    assert_eq!(approval(&parked).action, "Add a checkout");
    assert!(h.fleet.shells.values().all(|shell| shell.ran().is_empty()));
    let shown = call(&setup, "c2", json!({"action": "show"})).await;
    assert!(shown.is_success());
}

#[tokio::test]
async fn checkout_paths_are_private() {
    let h = harness(github(), Visibility::Private).await;
    let args = json!({"tasks": [{"title": "Build", "body": "Output lands in /srv/widget/out"}]});
    let result = call(&h.plan(PermissionLevel::Allow {}), "c1", args).await;
    assert!(!result.is_success());
    assert!(
        result.output().contains("/srv/widget"),
        "{}",
        result.output()
    );
    assert!(h.rows().await.is_empty());
}

#[tokio::test]
async fn after_cycles_are_refused() {
    let h = harness(Roles::default(), Visibility::Private).await;
    make_tasks(&h, &["A", "B", "C"]).await;
    let tool = h.tasks_tool(PermissionLevel::Allow {}, None);
    let after = |task: &str, after: &str| json!({"action": "update", "task": task, "after": after});
    assert!(call(&tool, "c1", after("b", "a")).await.is_success());
    assert!(call(&tool, "c2", after("c", "b")).await.is_success());
    let cycle = call(&tool, "c3", after("a", "c")).await;
    assert!(!cycle.is_success());
    assert!(cycle.output().contains("wait"), "{}", cycle.output());
    assert_eq!(h.rows().await[0].depends_on, None);
}

fn reply_tool(h: &Harness, level: PermissionLevel, approved: Option<&str>) -> reply::ReplyTool {
    reply::ReplyTool {
        tools: h.tools(level, approved),
    }
}

fn pr_41(last: &str) -> String {
    crate::projects::threads::tests::pr_41(last)
}

#[tokio::test]
async fn reply_tool_asks_and_posts_after_approval() {
    let h = harness(github(), Visibility::Public).await;
    h.shell.answer("pr view 41", 0, &pr_41("Shorter?"));
    h.shell.answer("api user", 0, "alice-dev\n");
    let args = json!({"repo": "acme/widget", "number": 41, "body": "Shortened it in 3f2c1a0."});
    // Even under Allow, a reply asks.
    let asked = call(
        &reply_tool(&h, PermissionLevel::Allow {}, None),
        "c1",
        args.clone(),
    )
    .await;
    let card = approval(&asked);
    assert_eq!(card.app, "Project");
    assert_eq!(card.action, "Post reply");
    let value = |label: &str| {
        card.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.clone())
    };
    assert_eq!(value("Reply").as_deref(), Some("Shortened it in 3f2c1a0."));
    assert_eq!(
        value("Thread").as_deref(),
        Some("PR #41: Login screen copy")
    );
    assert_eq!(value("Last from them").as_deref(), Some("sam: Shorter?"));
    assert!(h.shell.ran_with(" comment ").is_empty());
    // Approved, it posts once on the pull request.
    let posted = call(
        &reply_tool(&h, PermissionLevel::Ask {}, Some("c1")),
        "c1",
        args.clone(),
    )
    .await;
    assert!(posted.is_success(), "{}", posted.output());
    let ran = h.shell.ran_with(" comment ");
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert!(ran[0].contains("gh pr comment 41 "), "{}", ran[0]);
    // The user's own PR takes more replies.
    let again = call(
        &reply_tool(&h, PermissionLevel::Ask {}, Some("c2")),
        "c2",
        args,
    )
    .await;
    assert!(again.is_success(), "{}", again.output());
    // Never saved as Always.
    assert_eq!(saved_as(reply::REPLY, &json!({}), &h.project), None);
}

#[tokio::test]
async fn reply_tool_refuses_leaky_text() {
    let h = harness(github(), Visibility::Public).await;
    h.shell.answer("pr view 41", 0, &pr_41("Logs?"));
    let args =
        json!({"repo": "acme/widget", "number": 41, "body": "The log is at /home/sam/run.log"});
    let result = call(
        &reply_tool(&h, PermissionLevel::Allow {}, Some("c1")),
        "c1",
        args,
    )
    .await;
    assert!(!result.is_success());
    assert!(result.output().contains("/home/sam"), "{}", result.output());
    assert!(h.shell.ran().is_empty(), "{:?}", h.shell.ran());
    // Another repository is refused too.
    let elsewhere = json!({"repo": "acme/other", "number": 41, "body": "Done."});
    let result = call(
        &reply_tool(&h, PermissionLevel::Allow {}, Some("c2")),
        "c2",
        elsewhere,
    )
    .await;
    assert!(!result.is_success());
    assert!(h.shell.ran().is_empty());
}

#[tokio::test]
async fn someone_elses_thread_gets_one_reply() {
    let h = harness(github(), Visibility::Public).await;
    // The PR's author is alice-dev; the CLI is signed in as someone else.
    h.shell.answer("pr view 41", 0, &pr_41("Logs?"));
    h.shell.answer("api user", 0, "bea\n");
    let args = json!({"repo": "acme/widget", "number": 41, "body": "Here they are."});
    let first = call(
        &reply_tool(&h, PermissionLevel::Ask {}, Some("c1")),
        "c1",
        args.clone(),
    )
    .await;
    assert!(first.is_success(), "{}", first.output());
    let second = call(
        &reply_tool(&h, PermissionLevel::Ask {}, Some("c2")),
        "c2",
        args,
    )
    .await;
    assert!(!second.is_success());
    assert!(
        second.output().contains("one reply at most"),
        "{}",
        second.output()
    );
    assert_eq!(h.shell.ran_with(" comment ").len(), 1);
}
