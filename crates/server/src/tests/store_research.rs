use super::*;
use crate::{
    research::{RoleModel, Scout},
    store::TestDb,
};
use flashmind::types::ReasoningLevel;

fn plan() -> Plan {
    let model = RoleModel {
        model: "openrouter:deepseek/deepseek-v4.1-flash"
            .parse()
            .expect("a model id"),
        thinking: ReasoningLevel::Low,
    };
    Plan {
        scouts: vec![
            Scout {
                question: "Who ships first?".into(),
                model: model.clone(),
                round: 1,
            },
            Scout {
                question: "What does it cost?".into(),
                model: model.clone(),
                round: 1,
            },
        ],
        critic: model.clone(),
        writer: model,
    }
}

#[tokio::test]
async fn a_research_runs_through_its_statuses() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = UserKey::for_test("alice");
    let chat = Uuid::new_v4();
    let made = store
        .start_research(&user, chat, " EV batteries ")
        .await
        .unwrap();
    assert_eq!(made.topic, "EV batteries");
    assert_eq!(made.status, Status::Planning);
    // Starting again keeps the same research.
    let again = store.start_research(&user, chat, "other").await.unwrap();
    assert_eq!(again.id, made.id);

    assert!(store.approve_research(made.id, &plan()).await.unwrap());
    assert!(!store.approve_research(made.id, &plan()).await.unwrap());
    let row = store.research_in(&user, chat).await.unwrap().unwrap();
    assert_eq!(row.status, Status::Researching);
    assert_eq!(row.plan, Some(plan()));

    assert!(store.stop_research(made.id).await.unwrap());
    // The team moving on after a stop changes nothing.
    assert!(
        !store
            .move_research(made.id, Status::Researching, Status::Reviewing)
            .await
            .unwrap()
    );
    let row = store.research(made.id).await.unwrap().unwrap();
    assert_eq!(row.status, Status::Stopped);
    assert!(row.finished_at.is_some());
    // A stopped research can start again.
    assert!(store.approve_research(made.id, &plan()).await.unwrap());
    // And a finished one starts a follow-up round.
    assert!(
        store
            .move_research(made.id, Status::Researching, Status::Done)
            .await
            .unwrap()
    );
    let mut more = plan();
    more.scouts.push(Scout {
        question: "And next year?".into(),
        model: more.critic.clone(),
        round: 2,
    });
    assert!(store.approve_research(made.id, &more).await.unwrap());
    let row = store.research(made.id).await.unwrap().unwrap();
    assert_eq!(row.plan.map(|plan| plan.round()), Some(2));
    assert!(row.finished_at.is_none());
    store.interrupt_researches().await.unwrap();
    let row = store.research(made.id).await.unwrap().unwrap();
    assert_eq!(row.status, Status::Interrupted);
}
