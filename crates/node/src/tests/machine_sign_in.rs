use super::*;

#[tokio::test]
async fn unknown_tools_and_idle_ones_say_so() {
    let refused = answer(Request::Start {
        tool: "gemini".into(),
    })
    .await;
    assert!(matches!(refused, State::Failed { .. }), "{refused:?}");
    let idle = answer(Request::State {
        tool: "opencode".into(),
    })
    .await;
    assert_eq!(idle, State::Idle {});
    let cancelled = answer(Request::Cancel {
        tool: "opencode".into(),
    })
    .await;
    assert_eq!(cancelled, State::Idle {});
}
