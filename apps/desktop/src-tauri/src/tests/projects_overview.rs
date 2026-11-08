use super::*;
use beep_proto::ProjectTaskState;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn a_card_keeps_the_counts_and_names_the_latest_state() {
    let at = UNIX_EPOCH + Duration::from_secs(10);
    let card = OverviewCard::from(ProjectOverview {
        project_id: Uuid::nil(),
        name: "acme/widget".into(),
        tasks: TaskCounts {
            planned: 2,
            running: 1,
            blocked: 0,
            review: 1,
            done: 7,
        },
        workers: 1,
        latest: Some(ProjectTask {
            id: Uuid::nil(),
            slug: "login-copy".into(),
            title: "Login screen copy".into(),
            body: String::new(),
            state: ProjectTaskState::Review {},
            depends_on: None,
            issue_number: None,
            issue_url: None,
            branch: None,
            pr_url: Some("https://example.com/pr/41".into()),
            created_at: at,
            updated_at: at,
        }),
    });
    let json = serde_json::to_value(&card).expect("a card serializes");
    assert_eq!(json["tasks"]["done"], 7);
    assert_eq!(json["latest"]["state"], "review");
    assert_eq!(json["latest"]["updated_at"], 10_000);
}
