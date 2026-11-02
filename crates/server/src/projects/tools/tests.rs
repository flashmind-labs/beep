use super::*;
use crate::{
    permissions::{ApprovalPayload, Rules},
    projects::{
        gate::GatedProcess,
        model::{IssueRef, RepoName, Roles},
        testing::{FakeFleet, FakeShell, RecordingTool},
    },
    protocol::PermissionLevel,
    store::{AppPermissionRow, IssueAuthor, TestDb, TrackedIssue},
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
    assert!(
        result.output().contains("cannot start workers yet"),
        "{}",
        result.output()
    );
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
                start: None,
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

/// A task tracking issue #12, which `author` opened.
async fn tracked(h: &Harness, author: IssueAuthor) -> TaskRow {
    let task = NewTask {
        title: "Crash on start".into(),
        body: "From issue #12 on the host.".into(),
        depends_on: None,
        needs: Vec::new(),
        start: None,
    };
    let issue = IssueRef {
        number: 12,
        url: format!("{ISSUE_URL}12"),
    };
    match h
        .user
        .track_issue(h.project.id, &task, &IssueHost::Github, &issue, author)
        .await
        .unwrap()
    {
        TrackedIssue::Made(task) => task,
        TrackedIssue::Already(_) => panic!("a new task"),
    }
}

#[tokio::test]
async fn someone_elses_issue_gets_one_approved_comment_and_nothing_else() {
    let h = harness(github(), Visibility::Private).await;
    tracked(&h, IssueAuthor::Other).await;
    // Updating the task leaves the issue as it is.
    let update = json!({"action": "update", "task": "crash-on-start", "body": "New plan."});
    let result = call(&h.tasks_tool(PermissionLevel::Allow {}, None), "c0", update).await;
    assert!(result.is_success(), "{}", result.output());
    assert!(
        result.output().contains("stays as it is"),
        "{}",
        result.output()
    );
    // A comment always asks, even under Allow.
    let comment = json!({"action": "comment", "task": "crash-on-start", "body": "Fixed in #5."});
    let asked = call(
        &h.tasks_tool(PermissionLevel::Allow {}, None),
        "c1",
        comment.clone(),
    )
    .await;
    let card = approval(&asked);
    assert_eq!(card.action, "Comment on issue");
    assert!(
        card.fields
            .iter()
            .any(|field| field.value == "Fixed in #5.")
    );
    assert!(h.shell.ran().is_empty(), "{:?}", h.shell.ran());
    let posted = call(
        &h.tasks_tool(PermissionLevel::Ask {}, Some("c1")),
        "c1",
        comment.clone(),
    )
    .await;
    assert!(posted.is_success(), "{}", posted.output());
    // A second one is refused, approved or not.
    let again = call(
        &h.tasks_tool(PermissionLevel::Ask {}, Some("c2")),
        "c2",
        comment,
    )
    .await;
    assert!(!again.is_success(), "{}", again.output());
    let ran = h.shell.ran();
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert!(ran[0].contains("gh issue comment 12 "), "{}", ran[0]);
}

#[tokio::test]
async fn the_users_own_tracked_issue_is_edited_as_before() {
    let h = harness(github(), Visibility::Private).await;
    tracked(&h, IssueAuthor::User).await;
    let update = json!({"action": "update", "task": "crash-on-start", "body": "New plan."});
    let result = call(&h.tasks_tool(PermissionLevel::Allow {}, None), "c0", update).await;
    assert!(result.is_success(), "{}", result.output());
    assert_eq!(h.shell.ran_with("issue edit 12").len(), 1);
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
    assert_eq!(
        saved_as("exec", &json!({"command": "cargo test"}), project).as_deref(),
        Some("EXEC")
    );
    assert_eq!(
        saved_as("exec", &json!({"command": "gh pr merge 5"}), project),
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
            gate: Arc::new(ProjectGate::new(rules, None, Visibility::Private)),
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
async fn host_cli_through_exec_always_asks() {
    let h = harness(github(), Visibility::Private).await;
    let inner = Arc::new(RecordingTool::new("exec"));
    let exec = GatedExec {
        exec: inner.clone(),
        tools: h.tools(PermissionLevel::Allow {}, None),
    };
    let ran = call(&exec, "c1", json!({"command": "cargo test"})).await;
    assert_eq!(ran.output(), "ran");
    for command in [
        "gh pr merge 5 -R acme/widget --squash",
        "cd /srv/widget && /usr/bin/tea pr merge 5",
        "echo hi; teacli issue close 3",
    ] {
        let parked = call(&exec, "c2", json!({ "command": command })).await;
        assert_eq!(approval(&parked).action, "Run a command", "{command}");
    }
    let leaky = json!({"command": "gh issue create -t Fix -b 'see /home/sam/notes'"});
    let refused = call(&exec, "c3", leaky).await;
    assert!(!refused.is_success());
    assert!(
        refused.output().contains("/home/sam"),
        "{}",
        refused.output()
    );
    assert_eq!(inner.calls(), 1);
}

#[tokio::test]
async fn pushing_the_default_branch_through_exec_always_asks() {
    let h = harness(github(), Visibility::Private).await;
    let inner = Arc::new(RecordingTool::new("exec"));
    let exec = GatedExec {
        exec: inner.clone(),
        tools: h.tools(PermissionLevel::Allow {}, None),
    };
    for command in [
        "git push origin main",
        "cd /srv/widget && git push -u origin HEAD:main",
        "git -C /srv/widget push origin beep/x:refs/heads/main",
        "git push --all",
        "git push",
        "sh -c 'git push origin main'",
    ] {
        let parked = call(&exec, "c1", json!({ "command": command })).await;
        assert_eq!(approval(&parked).action, "Run a command", "{command}");
        assert_eq!(
            saved_as("exec", &json!({ "command": command }), &h.project),
            None,
            "{command}"
        );
    }
    assert_eq!(inner.calls(), 0);
    // The task's own branch follows the level.
    let own = call(
        &exec,
        "c2",
        json!({"command": "git push origin beep/fix-it"}),
    )
    .await;
    assert_eq!(own.output(), "ran");
    assert_eq!(
        saved_as(
            "exec",
            &json!({"command": "git push origin beep/fix-it"}),
            &h.project
        )
        .as_deref(),
        Some("EXEC")
    );
}

#[tokio::test]
async fn process_stop_asks_first() {
    let h = harness(github(), Visibility::Private).await;
    let inner = Arc::new(RecordingTool::new("process"));
    let process = GatedProcess {
        process: inner.clone(),
        gate: h.tools(PermissionLevel::Ask {}, None).gate.clone(),
    };
    let listed = call(&process, "c1", json!({"action": "list"})).await;
    assert_eq!(listed.output(), "ran");
    let stop = call(&process, "c2", json!({"action": "stop", "id": 4})).await;
    assert_eq!(approval(&stop).action, "Stop a background command");
    assert_eq!(inner.calls(), 1);
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
