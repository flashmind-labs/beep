use super::*;
use crate::agents::compaction::{Policy, Route};
use futures::{SinkExt, sink, stream};
use serde_json::json;

fn actor() -> Actor {
    let (live, _) = watch::channel(Live::default());
    Actor {
        live,
        node: Node {
            alerts: Arc::default(),
            checkouts: Arc::default(),
            stopping: Arc::default(),
            running: None,
        },
        moving: false,
        agent: "test-agent".into(),
        acp_session: Some(SessionId::new("session-test")),
        banner: None,
        replays: false,
        reading_cost: false,
        offers: Offers {
            takes_images: Some(false),
            thinking: Some(SessionConfigId::new("effort")),
            switch: Some(ModelSwitch::ConfigOption(SessionConfigId::new("model"))),
            mode_config: Some(SessionConfigId::new("mode")),
            ..Offers::new(acp::Protocol::V2)
        },
        foreground: false,
        ready_for_prompts: true,
        initial_v2_state: None,
        accepting_v2: Accepting::Done,
        left_plan_for: None,
        returning: None,
        compactor: Compactor::new(Route::Advertised, Policy::Off),
        backlog: Backlog::default(),
        sent: 0,
        context_turn: false,
        kept_title: false,
        turn: None,
        held_end: None,
        stop_within: Duration::from_secs(1),
        running: HashSet::new(),
        brief: None,
        standing: None,
        asked: VecDeque::new(),
        next_question: 1,
        stderr: VecDeque::new(),
        replies: FuturesUnordered::new(),
    }
}

async fn with_actor(test: impl AsyncFnOnce(Actor, ConnectionTo<Agent>)) {
    let outgoing = sink::drain::<String>().sink_map_err(|error| match error {});
    let transport =
        agent_client_protocol::Lines::new(outgoing, stream::pending::<std::io::Result<String>>());
    Builder::new(Client)
        .connect_with(transport, async |cx| {
            test(actor(), cx).await;
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn rejected_v2_compaction_cannot_arm_an_idle_stop_timer() {
    with_actor(async |mut actor, cx| {
        let session = actor.acp_session.clone().unwrap();
        actor.send_prompt(
            &cx,
            acp::prompt(&session, &Prompt::from("/compact".to_owned()), None),
            true,
        );
        assert!(actor.turn.is_some());
        let turn = actor.sent;
        assert!(
            actor
                .reply(
                    &cx,
                    Reply::Accepted {
                        turn,
                        compact: true,
                        result: Err(agent_client_protocol::Error::invalid_params()),
                    }
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(watched::read_sent(&actor.live, Live::status), Status::Idle);
        assert!(actor.turn.is_none());
        let waiting = actor.replies.len();
        actor.stop_due();
        assert_eq!(actor.replies.len(), waiting);
        assert_eq!(actor.stopping(), None);
        assert!(actor.reply(&cx, Reply::StopDue { turn }).unwrap().is_none());
    })
    .await;
}

#[tokio::test]
async fn a_rejected_v2_compaction_after_a_stop_clears_the_stop() {
    with_actor(async |mut actor, cx| {
        let session = actor.acp_session.clone().unwrap();
        actor.send_prompt(
            &cx,
            acp::prompt(&session, &Prompt::from("/compact".to_owned()), None),
            true,
        );
        actor.stop_due();
        assert_eq!(actor.stopping(), Some(Stopping::User));
        let turn = actor.sent;
        actor
            .reply(
                &cx,
                Reply::Accepted {
                    turn,
                    compact: true,
                    result: Err(agent_client_protocol::Error::invalid_params()),
                },
            )
            .unwrap();
        assert!(actor.turn.is_none());
        assert_eq!(actor.stopping(), None);

        // The next turn starts with nothing held back from the stopped one.
        actor.send_prompt(
            &cx,
            acp::prompt(&session, &Prompt::from("go on".to_owned()), None),
            true,
        );
        let next = actor.turn.as_ref().unwrap();
        assert_eq!(next.stopping, None);
        assert!(!next.steer_after);
    })
    .await;
}

#[tokio::test]
async fn incomplete_v2_options_preserve_working_configuration_controls() {
    with_actor(async |mut actor, cx| {
        for answer in [
            json!({}),
            json!({"configOptions":[]}),
            json!({"configOptions":[
                {"category":"model", "type":"select"},
                {"category":"mode", "type":"select"},
                {"category":"thought_level", "type":"select"}
            ]}),
        ] {
            actor.v2_options(&cx, &answer);
            assert_eq!(
                actor
                    .offers
                    .thinking
                    .as_ref()
                    .map(ToString::to_string)
                    .as_deref(),
                Some("effort")
            );
            assert_eq!(
                actor.offers.switch,
                Some(ModelSwitch::ConfigOption(SessionConfigId::new("model")))
            );
            let request = actor
                .mode_request(SessionId::new("session-test"), "plan")
                .unwrap();
            assert_eq!(request.params()["configId"], "mode");
        }
        assert!(
            actor
                .command(
                    &cx,
                    Command::Configure {
                        model: Some(ModelId::new("other")),
                        effort: Some(Effort::High),
                        fast: None,
                    }
                )
                .unwrap()
                .is_ok()
        );
    })
    .await;
}

#[tokio::test]
async fn a_valid_v2_option_replaces_only_its_own_selector() {
    with_actor(async |mut actor, cx| {
        actor.v2_options(&cx, &json!({"configOptions":[{
            "configId":"new-effort", "name":"Effort", "category":"thought_level", "type":"select",
            "currentValue":"high", "options":[{"value":"high", "name":"High"}]
        }]}));
        assert_eq!(
            actor
                .offers
                .thinking
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("new-effort")
        );
        assert_eq!(
            actor.offers.switch,
            Some(ModelSwitch::ConfigOption(SessionConfigId::new("model")))
        );
        assert!(
            actor
                .mode_request(SessionId::new("session-test"), "plan")
                .is_ok()
        );
    })
    .await;
}

#[tokio::test]
async fn recovery_context_cannot_rename_a_v2_conversation() {
    with_actor(async |mut actor, cx| {
        actor
            .live
            .send_modify(|live| live.info.title = Some("Original request".into()));
        actor.context_turn = true;
        let update = || {
            serde_json::from_value(
                json!({"sessionUpdate":"session_info_update", "title":"Recovery context"}),
            )
            .unwrap()
        };
        actor.v2_update(&cx, update()).unwrap();
        assert_eq!(
            watched::read_sent(&actor.live, |live| live.info.title.clone()).as_deref(),
            Some("Original request")
        );
        actor.context_turn = false;
        actor.v2_update(&cx, update()).unwrap();
        assert_eq!(
            watched::read_sent(&actor.live, |live| live.info.title.clone()).as_deref(),
            Some("Recovery context")
        );
    })
    .await;
}

#[tokio::test]
async fn a_steer_waits_for_the_turn_the_user_stopped() {
    with_actor(async |mut actor, cx| {
        let session = actor.acp_session.clone().unwrap();
        actor.offers.takes_steering = true;
        actor.send_prompt(
            &cx,
            acp::prompt(&session, &Prompt::from("look".to_owned()), None),
            true,
        );
        actor
            .command(&cx, Command::Cancel { by_user: true })
            .unwrap()
            .unwrap();
        actor
            .backlog
            .push(Prompt::from("it's dark now".to_owned()), true);

        actor.steer_now(&cx).unwrap();

        assert!(!actor.backlog.is_steering());
        assert_eq!(actor.backlog.waiting(), 1);
    })
    .await;
}

#[tokio::test]
async fn taking_steers_back_also_takes_the_one_sent_to_the_agent() {
    with_actor(async |mut actor, cx| {
        // The reply, and whether the adapter still takes steers after it.
        let replies = [
            (Ok(json!({"outcome": "promptRequired"})), true),
            (Ok(json!({"outcome": "failed"})), false),
            (Err(agent_client_protocol::Error::internal_error()), false),
        ];

        for (reply, still_steers) in replies {
            actor.offers.takes_steering = true;
            actor
                .backlog
                .begin_steer(Prompt::from("old text".to_owned()));
            actor.show_queued();
            assert_eq!(
                watched::read_sent(&actor.live, |live| live.info.state.queued_messages.clone()),
                ["old text"]
            );

            actor.command(&cx, Command::DropSteers).unwrap().unwrap();

            assert_eq!(
                watched::read_sent(&actor.live, |live| live.info.state.queued),
                0
            );

            // The adapter could not inject it: it must not come back as a prompt.
            actor.steered(&cx, reply).unwrap();
            assert!(!actor.backlog.has_prompts());
            assert!(!actor.backlog.is_steering());
            assert!(actor.turn.is_none());
            assert_eq!(actor.offers.takes_steering, still_steers);
        }
    })
    .await;
}

#[test]
fn a_dropped_server_tool_call_does_not_fail_the_turn() {
    let failed = |message: &str, kind: Option<&str>| {
        let mut e = agent_client_protocol::Error::internal_error();
        e.message = message.to_owned();
        e.data = kind.map(|kind| json!({"errorKind": kind}));
        e
    };
    let stuck = |ids: &str| {
        failed(
            &format!(
                "Internal error: Claude ended the turn without returning results for tool calls: {ids}"
            ),
            Some("incomplete_tool_call"),
        )
    };

    assert!(lost_server_tools(&stuck("srvtoolu_01a")));
    assert!(lost_server_tools(&stuck("srvtoolu_01a, srvtoolu_01b")));
    assert!(!lost_server_tools(&stuck("srvtoolu_01a, toolu_01c")));
    assert!(!lost_server_tools(&stuck("")));
    assert!(!lost_server_tools(&failed(
        "Internal error: srvtoolu_01a",
        None
    )));
}

#[test]
fn the_adapter_banner_is_left_out_once() {
    let mut actor = actor();
    actor.banner = Some("pi v1.0.0".into());

    assert!(actor.is_banner("pi v1.0.0"));
    assert!(!actor.is_banner("pi v1.0.0"));
    assert!(!actor.is_banner("an answer"));

    actor.acp_session = None;
    assert!(actor.is_banner("pi v1.0.0"));

    actor.replays = true;
    assert!(!actor.is_banner("a replayed answer"));
}

#[tokio::test]
async fn a_steer_during_an_autonomous_turn_goes_out_as_a_prompt() {
    with_actor(async |mut actor, cx| {
        actor.offers.protocol = acp::Protocol::V1;
        actor.offers.takes_steering = true;
        actor
            .live
            .send_modify(|live| live.info.state.status = Status::Idle);

        let spawned = |id: &str| {
            Event::Task(acp::AsyncTask::Spawned {
                async_task_id: id.into(),
                name: "Claude is working".into(),
                task_type: "turn".into(),
                description: String::new(),
                can_stop: true,
                output_file_path: None,
                tool_call_id: None,
            })
        };
        let ended = |id: &str| {
            Event::Task(acp::AsyncTask::StateUpdate {
                async_task_id: id.into(),
                state: "completed".into(),
                summary: None,
                output_file_path: None,
                tool_call_id: None,
            })
        };

        actor.event(&cx, spawned("turn-1")).unwrap();
        assert_eq!(
            watched::read_sent(&actor.live, Live::status),
            Status::Working
        );
        assert!(actor.turn.is_none());

        actor
            .command(&cx, Command::Steer("where are the keys?".to_owned().into()))
            .unwrap()
            .unwrap();
        assert!(actor.turn.is_some(), "sent as a prompt");
        assert!(!actor.backlog.has_prompts());
        assert!(!actor.backlog.is_steering());
        assert_eq!(
            watched::read_sent(&actor.live, |live| live.info.state.queued),
            0
        );

        // End that prompt and the autonomous turn; test a steer held behind an open question.
        actor
            .reply(
                &cx,
                Reply::Prompt(Ok(PromptResponse::new(
                    agent_client_protocol::v1::schema::StopReason::EndTurn,
                ))),
            )
            .unwrap();
        actor.event(&cx, ended("turn-1")).unwrap();
        assert_eq!(watched::read_sent(&actor.live, Live::status), Status::Idle);

        actor.event(&cx, spawned("turn-2")).unwrap();
        let (reply, _answer) = oneshot::channel();
        actor
            .command(
                &cx,
                Command::Ask {
                    question: super::super::Asking {
                        text: "Proceed?".into(),
                        detail: None,
                        options: Vec::new(),
                    },
                    reply,
                },
            )
            .unwrap()
            .unwrap();
        actor
            .command(&cx, Command::Steer("after question".to_owned().into()))
            .unwrap()
            .unwrap();
        assert!(actor.turn.is_none(), "waits while the question is open");
        assert_eq!(actor.backlog.waiting(), 1);

        let id = watched::read_sent(&actor.live, |live| live.info.question.as_ref().unwrap().id);
        actor
            .command(
                &cx,
                Command::Answer {
                    question: id,
                    reply: "yes".into(),
                },
            )
            .unwrap()
            .unwrap();
        assert!(actor.turn.is_some(), "sent once the question clears");
        assert!(!actor.backlog.has_prompts());

        // End the prompt, hold a steer directly, and verify task completion drains it too.
        actor
            .reply(
                &cx,
                Reply::Prompt(Ok(PromptResponse::new(
                    agent_client_protocol::v1::schema::StopReason::EndTurn,
                ))),
            )
            .unwrap();
        actor
            .backlog
            .push(Prompt::from("held until task ends".to_owned()), true);
        actor.show_queued();
        actor.event(&cx, ended("turn-2")).unwrap();
        assert!(
            actor.turn.is_some(),
            "autonomous turn end drains waiting steers"
        );
        assert!(!actor.backlog.has_prompts());
    })
    .await;
}

#[tokio::test]
async fn a_steer_during_v1_compaction_waits_for_compaction_to_finish() {
    with_actor(async |mut actor, cx| {
        actor.offers.protocol = acp::Protocol::V1;
        actor.offers.takes_steering = true;
        actor.compactor = Compactor::new(Route::Advertised, Policy::AtTokens(100));
        actor.live.send_modify(|live| {
            live.info.context = Some(beep_proto::AgentContext {
                used: 150,
                size: 200,
            });
            live.commands.push(beep_proto::node::agent::AgentCommand {
                name: "compact".into(),
                description: String::new(),
                hint: String::new(),
            });
        });

        actor.turn_done(&cx, StopReason::EndTurn, None, false);
        assert!(actor.compactor.running());
        assert!(actor.turn.is_none());

        actor
            .command(&cx, Command::Steer("after compact".to_owned().into()))
            .unwrap()
            .unwrap();
        assert!(actor.turn.is_none(), "waits while /compact runs");
        assert_eq!(actor.backlog.waiting(), 1);

        actor
            .reply(
                &cx,
                Reply::Compacted(Ok(PromptResponse::new(
                    agent_client_protocol::v1::schema::StopReason::EndTurn,
                ))),
            )
            .unwrap();
        assert!(!actor.compactor.running());
        assert!(actor.turn.is_some(), "sent once /compact finishes");
        assert!(!actor.backlog.has_prompts());
    })
    .await;
}

#[tokio::test]
async fn a_failed_reply_for_an_echoed_steer_after_a_stop_keeps_steering_on() {
    with_actor(async |mut actor, cx| {
        actor.offers.takes_steering = true;
        actor
            .backlog
            .begin_steer(Prompt::from("make it blue".to_owned()));
        actor.backlog.steering_mut().unwrap().shown = true;

        actor
            .command(&cx, Command::Cancel { by_user: false })
            .unwrap()
            .unwrap();
        actor
            .steered(&cx, Err(agent_client_protocol::Error::internal_error()))
            .unwrap();

        assert!(actor.offers.takes_steering);
        assert!(!actor.backlog.has_prompts());
    })
    .await;
}
