use super::{follow::*, *};
use crate::{
    nodes::agent::follow::apply,
    protocol::node::agent::{AnswerOption, Event, OptionKind, Question, State, Status, StopReason},
};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::sync::{mpsc, watch};
use tokio_util::task::AbortOnDropHandle;

fn question(id: u32) -> Question {
    Question {
        id,
        text: "Edit notes.txt".into(),
        detail: None,
        options: vec![AnswerOption {
            id: "allow".into(),
            name: "Allow".into(),
            kind: OptionKind::AllowOnce,
        }],
    }
}

#[test]
fn a_wait_ends_on_a_turn_a_new_question_or_the_end() {
    let mut watched = Watched::default();
    apply(
        &mut watched,
        Event::State {
            state: State {
                status: Status::Working,
                ..State::default()
            },
        },
    );
    let mark = watched.mark();
    assert!(!watched.moved_on(mark));
    apply(
        &mut watched,
        Event::Question {
            question: question(1),
        },
    );
    assert!(watched.moved_on(mark));
    assert_eq!(watched.info.state.status, Status::Asking);

    let mark = watched.mark();
    assert!(
        !watched.moved_on(mark),
        "the same question again is not news"
    );
    apply(
        &mut watched,
        Event::State {
            state: State {
                status: Status::Working,
                ..State::default()
            },
        },
    );
    assert!(watched.info.question.is_none(), "an answered question goes");
    apply(
        &mut watched,
        Event::TurnDone {
            turn: 1,
            stop_reason: StopReason::EndTurn,
            last_message: "Done".into(),
        },
    );
    assert!(watched.moved_on(mark));
    assert_eq!(watched.info.last_message, "Done");

    let mark = watched.mark();
    apply(
        &mut watched,
        Event::Ended {
            reason: "closed".into(),
        },
    );
    assert!(watched.moved_on(mark));
    assert!(watched.settled());
}

#[tokio::test]
async fn a_waiting_call_keeps_reports_out_of_the_chat_until_it_returns() {
    let (state_tx, state) = watch::channel(Watched::default());
    let (orders, _orders) = mpsc::channel(1);
    let link = AgentLink {
        orders,
        state,
        waiting: Arc::default(),
        _task: AbortOnDropHandle::new(tokio::spawn(async {})),
    };
    let wait = link.wait(link.now().mark(), Duration::from_secs(600));
    tokio::pin!(wait);
    assert!(futures::poll!(&mut wait).is_pending());
    assert_eq!(link.waiting.load(Ordering::SeqCst), 1);
    state_tx.send_modify(|watched| {
        apply(
            watched,
            Event::TurnDone {
                turn: 1,
                stop_reason: StopReason::EndTurn,
                last_message: String::new(),
            },
        )
    });
    assert_eq!(wait.await.info.turns, 1);
    assert_eq!(link.waiting.load(Ordering::SeqCst), 0);
}

#[test]
fn reads_go_again_after_a_reconnect_and_writes_only_when_unsent() {
    use {Failure::*, Retry::*};
    assert_eq!(Unsent.retry(true), AfterReconnect);
    assert_eq!(Unsent.retry(false), AfterReconnect);
    assert_eq!(Dropped.retry(true), AfterReconnect);
    assert_eq!(Dropped.retry(false), No);
    assert_eq!(Silent.retry(true), IfReplaced);
    assert_eq!(Silent.retry(false), No);
    assert_eq!(Answered.retry(true), No);
    assert_eq!(Answered.retry(false), No);
}
