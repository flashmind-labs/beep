use super::*;
use crate::{
    permissions::{ApprovalPayload, Rules},
    projects::{
        model::{RepoName, Roles},
        testing::{FakeFleet, FakeShell},
    },
    protocol::PermissionLevel,
    store::{AppPermissionRow, TestDb},
};
use flashmind::core::CancellationToken;
use iroh::EndpointId;

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
            chat: Uuid::new_v4(),
            gate: Arc::new(ProjectGate::new(
                rules,
                approved.map(str::to_owned),
                self.project.visibility,
            )),
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
async fn local_only_projects_skip_issues() {
    let h = harness(Roles::default(), Visibility::Public).await;
    let args = json!({"tasks": [{"title": "Build", "body": "b"}], "start": true});
    let result = call(&h.plan(PermissionLevel::Allow {}), "c1", args).await;
    assert!(result.is_success(), "{}", result.output());
    assert!(h.shell.ran().is_empty());
    let rows = h.rows().await;
    assert!(rows[0].issue.is_none());
    assert!(rows[0].start.is_some());
}

#[tokio::test]
async fn merge_always_asks() {
    let h = harness(github(), Visibility::Private).await;
    let made = h
        .user
        .create_tasks(
            h.project.id,
            &[NewTask {
                title: "Build".into(),
                body: String::new(),
                depends_on: None,
                needs: Vec::new(),
            }],
        )
        .await
        .unwrap();
    let url = "https://github.com/acme/widget/pull/5";
    h.user.set_task_pr(made[0].id, url).await.unwrap();
    h.shell.answer(
        "pr view",
        0,
        r#"{"number": 5, "state": "OPEN", "url": "https://github.com/acme/widget/pull/5",
            "statusCheckRollup": [{"conclusion": "SUCCESS"}], "reviewDecision": "APPROVED"}"#,
    );
    let args = json!({"action": "merge", "task": "build"});
    let result = call(
        &h.tasks_tool(PermissionLevel::Allow {}, None),
        "c1",
        args.clone(),
    )
    .await;
    let card = approval(&result);
    assert_eq!(card.action, "Merge pull request");
    let checks = card.fields.iter().find(|field| field.label == "Checks");
    assert_eq!(checks.unwrap().value, "passing");
    assert!(card.fields.iter().any(|field| field.value == url));
    assert!(h.shell.ran_with("pr merge").is_empty());

    let result = call(
        &h.tasks_tool(PermissionLevel::Ask {}, Some("c1")),
        "c1",
        args,
    )
    .await;
    assert!(result.is_success(), "{}", result.output());
    assert_eq!(h.shell.ran_with("pr merge 5").len(), 1);
    assert_eq!(h.rows().await[0].state, TaskState::Done);
}

#[tokio::test]
async fn drop_closes_as_not_planned() {
    let h = harness(github(), Visibility::Private).await;
    h.shell
        .answer("issue create", 0, &format!("{ISSUE_URL}8\n"));
    call(
        &h.plan(PermissionLevel::Allow {}),
        "c1",
        plan_of(&["Build"]),
    )
    .await;
    let args = json!({"action": "drop", "task": "build"});
    let asked = call(
        &h.tasks_tool(PermissionLevel::Ask {}, None),
        "c2",
        args.clone(),
    )
    .await;
    assert_eq!(approval(&asked).action, "Drop task");
    let result = call(&h.tasks_tool(PermissionLevel::Allow {}, None), "c3", args).await;
    assert!(result.is_success(), "{}", result.output());
    let closed = h.shell.ran_with("issue close 8");
    assert_eq!(closed.len(), 1);
    assert!(
        closed[0].contains("--reason 'not planned'"),
        "{}",
        closed[0]
    );
    assert_eq!(h.rows().await[0].state, TaskState::Dropped);
}

#[tokio::test]
async fn setup_rejects_unshared_checkout() {
    let h = harness(github(), Visibility::Private).await;
    let setup = SetupTool {
        tools: h.tools(PermissionLevel::Ask {}, None),
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
        tools: h.tools(PermissionLevel::Ask {}, None),
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
