use super::*;
use crate::projects::{
    model::{ProjectId, TaskId, TaskState},
    worker::ChosenPersona,
};
use beep_projects::model::TaskSlug;
use chrono::Utc;
use uuid::Uuid;

fn placement(agent: WorkerAgent) -> Placement {
    Placement {
        machine_name: "build-box".to_owned(),
        agent,
        persona: Some(ChosenPersona {
            name: "reviewer".to_owned(),
        }),
        base: "main".to_owned(),
    }
}

fn task() -> TaskRow {
    TaskRow {
        id: TaskId(Uuid::new_v4()),
        project: ProjectId(Uuid::new_v4()),
        slug: "fix-it".parse::<TaskSlug>().expect("a slug"),
        title: "Fix it".to_owned(),
        body: "Make it pass.".to_owned(),
        state: TaskState::Planned,
        depends_on: None,
        needs: Vec::new(),
        issue_host: None,
        issue: None,
        branch: None,
        pr_url: None,
        start: None,
        pr_seen: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        issue_foreign: false,
        issue_commented: false,
    }
}

#[test]
fn every_start_says_its_agent_runs_without_asking() {
    for name in WorkerAgent::VARIANTS {
        let agent: WorkerAgent = name.parse().expect("an agent");
        let card = start_card(&task(), "build-box", &placement(agent), &[]);
        assert!(
            card.fields.iter().any(|field| field.label == "Permissions"
                && field
                    .value
                    .starts_with(&format!("{name} runs commands without asking"))),
            "{:?}",
            card.fields
        );
    }
}
