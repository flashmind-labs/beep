use super::*;
use crate::agents::compaction::{Policy, Route};
use futures::{SinkExt, sink, stream};
use serde_json::json;

fn actor() -> Actor {
    let (live, _) = watch::channel(Live::default());
    Actor {
        live,
        alerts: Arc::default(),
        node_stopping: Arc::default(),
        moving: false,
        agent: "test-agent".into(),
        cwd: PathBuf::from("/srv/project"),
        acp_session: Some(SessionId::new("session-test")),
        banner: None,
        reading_cost: false,
        protocol: acp::Protocol::V2,
        foreground: false,
        ready_for_prompts: true,
        initial_v2_state: None,
        compacting_v2: false,
        accepting_v2: false,
        idle_v2: None,
        takes_images: Some(false),
        takes_steering: false,
        steering: None,
        steer_after_turn: false,
        resume: None,
        earlier: Vec::new(),
        model: None,
        effort: None,
        fast: None,
        fast_option: None,
        thinking: Some(SessionConfigId::new("effort")),
        switch: Some(ModelSwitch::ConfigOption(SessionConfigId::new("model"))),
        mode_config: Some(SessionConfigId::new("mode")),
        bypass_mode: None,
        first_mode: None,
        left_plan_for: None,
        returning: None,
        compactor: Compactor::new(Route::Advertised, Policy::Off),
        queued: VecDeque::new(),
        steers: VecDeque::new(),
        sent: 0,
        context_turn: false,
        turn: None,
        held_end: None,
        stopping: None,
        by_user: false,
        stop_within: Duration::from_secs(1),
        running: HashSet::new(),
        brief: None,
        standing: None,
        mcp_servers: Vec::new(),
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
        assert_eq!(actor.stopping, None);
        assert!(actor.reply(&cx, Reply::StopDue { turn }).unwrap().is_none());
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
                actor.thinking.as_ref().map(ToString::to_string).as_deref(),
                Some("effort")
            );
            assert_eq!(
                actor.switch,
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
            actor.thinking.as_ref().map(ToString::to_string).as_deref(),
            Some("new-effort")
        );
        assert_eq!(
            actor.switch,
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
        actor.takes_steering = true;
        actor.send_prompt(
            &cx,
            acp::prompt(&session, &Prompt::from("look".to_owned()), None),
            true,
        );
        actor.by_user = true;
        actor
            .steers
            .push_back(Prompt::from("it's dark now".to_owned()));

        actor.steer_now(&cx).unwrap();

        assert!(actor.steering.is_none());
        assert_eq!(actor.steers.len(), 1);
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

    actor.live.send_modify(|live| live.replays = true);
    assert!(!actor.is_banner("a replayed answer"));
}
