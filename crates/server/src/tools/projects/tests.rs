use super::*;
use crate::{
    projects::testing::machine_id,
    store::{NewImport, NewNode, NewTask},
    watchers::testing::{Harness, Scripted},
};
use flashmind::core::CancellationToken;
use std::time::UNIX_EPOCH;

/// A fixed "now", an hour after the items and sessions below moved.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn minutes_ago(minutes: u64) -> SystemTime {
    now() - Duration::from_secs(minutes * 60)
}

async fn machine(user: &User, seed: u8, name: &str) -> String {
    let endpoint = machine_id(seed);
    user.node_connected(NewNode {
        endpoint,
        name,
        version: "0.1.0",
    })
    .await
    .expect("machine linked");
    endpoint.to_string()
}

async fn import(user: &User, project: &str, endpoint: &str, folder: &str) {
    user.save_import(NewImport {
        project,
        endpoint,
        folder,
        url: &format!("https://github.com/{project}"),
    })
    .await
    .expect("imported");
}

fn item(repo: &str, number: u64, kind: GitItemKind) -> GitItem {
    GitItem {
        host: "github.com".into(),
        repo: repo.into(),
        number,
        kind,
        title: format!("Item {number}"),
        url: format!("https://github.com/{repo}/pull/{number}"),
        updated_at: minutes_ago(120),
        authored: false,
        assigned: false,
        review_requested: false,
        draft: false,
        review: GitReview::None {},
        checks_failing: false,
        comments: 0,
    }
}

fn session(endpoint: &str, machine: &str, agent: &str, cwd: &str) -> MachineSession {
    MachineSession {
        machine_id: endpoint.into(),
        machine_name: machine.into(),
        id: format!("{agent}-1"),
        agent: agent.into(),
        title: Some("Fix the login page".into()),
        cwd: cwd.into(),
        status: AgentStatus::Working {},
        doing: None,
        started_at: minutes_ago(30),
        last_activity: minutes_ago(4),
        external: false,
        question: None,
    }
}

/// Alice's two projects on two machines; gives the build box's and the mac's keys.
async fn two_projects(alice: &User) -> (String, String) {
    let build = machine(alice, 1, "build-box").await;
    let mac = machine(alice, 2, "mac").await;
    import(alice, "acme/widget", &build, "/srv/widget").await;
    import(alice, "acme/widget", &mac, "/Users/me/widget").await;
    import(alice, "acme/gadget", &mac, "/Users/me/gadget").await;
    let widget = alice
        .projects()
        .await
        .expect("listed")
        .into_iter()
        .find(|row| row.name.to_string() == "acme/widget")
        .expect("widget is a project");
    alice
        .create_tasks(
            widget.id,
            &[NewTask {
                title: "Spin it".into(),
                body: "Make the widget spin.".into(),
                depends_on: None,
                needs: Vec::new(),
                start: None,
            }],
        )
        .await
        .expect("task made");
    (build, mac)
}

fn widget_items() -> Vec<GitItem> {
    let mut asks_review = item("acme/widget", 12, GitItemKind::PullRequest {});
    asks_review.review_requested = true;
    let mut own = item("acme/widget", 13, GitItemKind::PullRequest {});
    own.authored = true;
    own.review = GitReview::Approved {};
    own.checks_failing = true;
    let mut issue = item("acme/widget", 3, GitItemKind::Issue {});
    issue.assigned = true;
    issue.comments = 4;
    let elsewhere = item("other/thing", 1, GitItemKind::PullRequest {});
    vec![asks_review, own, issue, elsewhere]
}

#[tokio::test]
async fn list_gives_each_project_with_machines_counts_and_sessions() {
    let harness = Harness::new(Scripted::default()).await;
    let alice = &harness.alice;
    let (build, mac) = two_projects(alice).await;
    let live = Live {
        items: widget_items(),
        sessions: vec![
            session(&mac, "mac", "claude", "/Users/me/gadget/.worktrees/login"),
            session(&build, "build-box", "codex", "/srv/elsewhere"),
            // The same folder on another machine is not this project.
            session(&build, "build-box", "codex", "/Users/me/gadget"),
        ],
        notes: Vec::new(),
    };
    let projects = load(alice).await.expect("loaded");
    let text = render_list(&projects, &live, now());

    assert!(text.starts_with("2 projects."), "{text}");
    assert!(
        text.contains(
            "- acme/gadget (https://github.com/acme/gadget)\n  on: mac /Users/me/gadget\n  \
             open: 0 PRs, 0 issues, 0 tasks\n  session: claude on mac \"Fix the login page\" \
             (working, last activity 4 min ago) in /Users/me/gadget/.worktrees/login\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "- acme/widget (https://github.com/acme/widget)\n  on: build-box /srv/widget; \
             mac /Users/me/widget\n  open: 2 PRs (2 waiting on the user), 1 issue, 1 task\n"
        ),
        "{text}"
    );
    assert!(
        text.ends_with("2 more coding sessions outside these projects' folders."),
        "{text}"
    );
}

#[tokio::test]
async fn show_names_the_projects_when_one_is_not_found() {
    let harness = Harness::new(Scripted::default()).await;
    let alice = &harness.alice;
    two_projects(alice).await;
    import(alice, "other/widget", "laptop", "/home/user/widget").await;
    let projects = load(alice).await.expect("loaded");
    assert_eq!(
        find(&projects, "widget").map(|p| p.name.to_string()),
        Err(
            "Several projects are called widget. Give org/repo. Projects: acme/gadget, \
             acme/widget, other/widget."
                .into()
        )
    );
    assert_eq!(
        find(&projects, "OTHER/widget").map(|p| p.name.to_string()),
        Ok("other/widget".into())
    );
    // An import on a machine since unlinked lists no place.
    assert!(
        find(&projects, "other/widget")
            .expect("found")
            .places
            .is_empty()
    );

    let tool = ProjectsTool {
        app: harness.app.clone(),
        user: alice.clone(),
    };
    let cancel = CancellationToken::new();
    let missing = tool
        .execute(ToolContext::new(
            "show",
            json!({"action": "show", "project": "nothing"}),
            None,
            &cancel,
        ))
        .await
        .expect("the tool runs");
    assert!(!missing.is_success());
    let listed = tool
        .execute(ToolContext::new(
            "list",
            json!({"action": "list"}),
            None,
            &cancel,
        ))
        .await
        .expect("the tool runs");
    assert!(listed.is_success());
}
