use super::*;
use crate::{
    account::user::UserKey,
    store::{CreateRun, FinishedTurn, RunStatus, TestDb, test_chat},
};
use flashmind::core::{Conversation, ConversationEntry};

fn tool(call_id: &str, description: &str) -> Step {
    Step {
        sent_at: Utc::now(),
        body: StepBody::Tool {
            call_id: call_id.to_owned(),
            name: "recall".to_owned(),
            output: "tea".to_owned(),
            success: true,
            description: description.to_owned(),
        },
    }
}

#[tokio::test]
async fn tool_steps_keep_what_they_showed() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let chat = test_chat("s1");
    let CreateRun::Created(run) = store
        .create_run(&alice, chat, "req-1", "What do I like?")
        .await
        .unwrap()
    else {
        panic!("the run was not created");
    };
    let steps = [
        Step {
            sent_at: Utc::now(),
            body: StepBody::User {
                content: "What do I like?".to_owned(),
                turn: TurnRecord::default(),
            },
        },
        tool("a", "Recalling memories"),
        // Saved without one, as every step was before descriptions.
        tool("b", ""),
    ];
    let mut context = Conversation::new();
    context.add(ConversationEntry::user("What do I like?"));
    store
        .finish(
            &alice,
            &run,
            Some(FinishedTurn {
                context: &context,
                prompt_tokens: Some(0),
                steps: &steps,
                stands_alone: false,
                carried: TurnRecord::default(),
            }),
            RunStatus::Completed,
        )
        .await
        .unwrap();

    let labels = store.tool_labels(chat).await.unwrap();
    assert_eq!(
        labels,
        [ToolLabel {
            call_id: "a".to_owned(),
            description: "Recalling memories".to_owned(),
        }]
    );
    let described: Vec<String> = store
        .chat_messages(chat)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|message| match message.body {
            StoredBody::Tool { description, .. } => Some(description),
            _ => None,
        })
        .collect();
    assert_eq!(described, ["Recalling memories", ""]);
}
