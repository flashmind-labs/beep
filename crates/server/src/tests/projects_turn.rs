use super::*;
use crate::projects::testing::widget_project;
use crate::{
    projects::testing::FakeFleet,
    protocol::node::Capability,
    store::{NewTask, TestDb},
};
use beep_projects::model::{IssueHost, Roles};

#[tokio::test]
async fn context_lists_tasks_machines_and_issues() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = User::for_test(store, "alice");
    let mut fleet = FakeFleet::default();
    let (gpu, shell) = fleet.add(
        1,
        "gpu-box",
        &[Capability::Cuda, Capability::Blender],
        "/srv",
    );
    fleet.files.insert(
        "/srv/widget/.agents/rigger.md".into(),
        "---\nname: rigger\ndescription: Rigs characters\nneeds: [blender]\n---\nRig.\n".into(),
    );
    shell.answer(
        "issue list",
        0,
        r#"[{"number": 7, "state": "OPEN", "title": "Render the intro", "comments": []}]"#,
    );
    let project = widget_project(
        &user,
        gpu,
        Roles {
            issues: Some(IssueHost::Github),
            prs: Some(IssueHost::Github),
        },
        Visibility::Private,
    )
    .await;
    user.create_tasks(
        project.id,
        &[NewTask {
            title: "Fix the build".into(),
            body: String::new(),
            depends_on: None,
            needs: Vec::new(),
            start: None,
        }],
    )
    .await
    .unwrap();
    let project = user.project(project.id).await.unwrap().unwrap();
    let ctx = context_with(&fleet, &user, &project).await;
    assert!(ctx.contains("fix-the-build: planned"), "{ctx}");
    assert!(ctx.contains("gpu-box (cuda, blender): online"), "{ctx}");
    assert!(ctx.contains("#7 open: Render the intro"), "{ctx}");
    assert!(
        ctx.contains("- rigger: Rigs characters (needs blender)"),
        "{ctx}"
    );
    assert!(ctx.contains("- gpu-box: /srv/widget (home)"), "{ctx}");
    assert_eq!(shell.ran_with("issue list").len(), 1);
}

#[tokio::test]
async fn a_running_issue_create_is_never_cut_off() {
    let db = TestDb::new().await.unwrap();
    let user = Arc::new(User::for_test(db.store().await.unwrap(), "alice"));
    let mut fleet = FakeFleet::default();
    let (home, shell) = fleet.add(1, "build-box", &[], "/srv");
    shell.answer(
        "issue create",
        0,
        "https://github.com/acme/widget/issues/3\n",
    );
    let release = shell.block_on("issue create");
    let project = widget_project(
        &user,
        home,
        Roles {
            issues: Some(IssueHost::Github),
            prs: None,
        },
        Visibility::Private,
    )
    .await;
    user.create_tasks(
        project.id,
        &[NewTask {
            title: "Fix the build".into(),
            body: String::new(),
            depends_on: None,
            needs: Vec::new(),
            start: None,
        }],
    )
    .await
    .unwrap();
    let project = user.project(project.id).await.unwrap().unwrap();
    let fleet = Arc::new(fleet);
    // The create is still running when the budget is spent.
    let ctx = start_with(fleet.clone(), &user, &project, Duration::from_millis(10)).await;
    assert!(ctx.contains("fix-the-build: planned"), "{ctx}");
    release.notify_one();
    fleet.projects.mirrored(project.id).await;
    let tasks = user.project_tasks(project.id).await.unwrap();
    assert_eq!(tasks[0].issue.as_ref().map(|issue| issue.number), Some(3));
}
