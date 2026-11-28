//! ACP v2 foreground lifecycle, independent of prompt acceptance.

use super::actor::{Accepting, Actor, Reply, Turn};
use crate::agents::acp::Protocol;
use agent_client_protocol::{
    Agent, ConnectionTo, Error, JsonRpcMessage, UntypedMessage,
    v1::schema::{PromptRequest, PromptResponse, SessionId, SetSessionModeRequest},
    v2::schema as v2,
};
use beep_proto::node::agent::{Status, StopReason};
use futures::FutureExt;
use serde_json::{Value, json};

impl Actor {
    pub(super) fn mode_request(
        &self,
        session: SessionId,
        mode: &str,
    ) -> Result<UntypedMessage, Error> {
        match self.offers.protocol {
            Protocol::V1 => {
                SetSessionModeRequest::new(session, mode.to_owned()).to_untyped_message()
            }
            Protocol::V2 => {
                let id = self.offers.mode_config.as_ref().ok_or_else(|| {
                    Error::invalid_params().data("the agent has no mode config option")
                })?;
                UntypedMessage::new(
                    "session/set_config_option",
                    json!({ "sessionId": session, "configId": id, "type": "id", "value": mode }),
                )
            }
        }
    }

    pub(super) fn send_prompt(
        &mut self,
        cx: &ConnectionTo<Agent>,
        request: PromptRequest,
        compact: bool,
    ) {
        if self.offers.protocol == Protocol::V1 {
            if compact {
                self.send(cx, request, Reply::Compacted);
            } else {
                self.send(cx, request, Reply::Prompt);
            }
            return;
        }

        if compact {
            self.sent += 1;
            self.turn = Some(Turn::new(self.sent));
        }
        self.accepting_v2 = Accepting::Waiting;
        self.foreground = false;
        let turn = self.sent;
        let request = request.to_untyped_message().and_then(|request| {
            v2::PromptRequest::parse_message(request.method(), request.params())
        });
        let answer = request.map(|request| cx.send_request(request).block_task());
        self.replies.push(
            async move {
                let result = match answer {
                    Ok(answer) => answer.await,
                    Err(error) => Err(error),
                };
                Reply::Accepted {
                    turn,
                    compact,
                    result,
                }
            }
            .boxed(),
        );
    }

    pub(super) fn v2_options(&mut self, cx: &ConnectionTo<Agent>, answer: &Value) {
        let offered = self.offers.protocol.offers(answer);
        self.offers.added(&offered);
        if let Some(model) = offered.current {
            self.model_is(Some(&model));
        }

        if let Some(modes) = offered.modes {
            self.live
                .send_modify(|live| live.info.modes = modes.available);
            self.mode_is(cx, modes.current);
        }
    }

    pub(super) fn v2_update(
        &mut self,
        cx: &ConnectionTo<Agent>,
        update: v2::SessionUpdate,
    ) -> anyhow::Result<()> {
        match update {
            v2::SessionUpdate::SessionInfoUpdate(_) if self.drops_title() => {}
            v2::SessionUpdate::StateUpdate(state) => self.v2_state(cx, state),
            v2::SessionUpdate::ConfigOptionUpdate(options) => {
                self.v2_options(cx, &json!({"configOptions": options.config_options}));
            }
            update => {
                self.live.send_modify(|live| live.apply_v2(&update));
                self.look_for_place();
                if let Some(stopping) = self.stopping() {
                    self.show_stopping(stopping);
                }
                self.subagents_moved();
            }
        }
        Ok(())
    }

    pub(super) fn v2_state(&mut self, cx: &ConnectionTo<Agent>, state: v2::StateUpdate) {
        if self.acp_session.is_none() {
            self.initial_v2_state = Some(state);
            return;
        }
        match state {
            v2::StateUpdate::Idle(idle) => {
                if self.foreground {
                    if matches!(self.accepting_v2, Accepting::Done) {
                        self.v2_idle(cx, idle);
                    } else {
                        self.accepting_v2 = Accepting::Idle(idle);
                    }
                }
            }
            state => {
                self.foreground = true;
                if self.turn.is_none() {
                    self.sent += 1;
                    self.turn = Some(Turn::new(self.sent));
                }
                self.live.send_modify(|live| {
                    live.info.state.status = match state {
                        v2::StateUpdate::RequiresAction(_) => Status::Asking,
                        _ => Status::Working,
                    }
                });
            }
        }
    }

    pub(super) fn v2_idle(&mut self, cx: &ConnectionTo<Agent>, idle: v2::IdleStateUpdate) {
        self.foreground = false;
        let by_user = self.turn.take().is_some_and(|turn| turn.by_user);
        self.running.clear();
        let (stop, error) = match idle.stop_reason {
            Some(v2::StopReason::EndTurn) => (StopReason::EndTurn, None),
            Some(v2::StopReason::Cancelled) => (StopReason::Cancelled, None),
            Some(v2::StopReason::MaxTokens) => (StopReason::MaxTokens, None),
            Some(v2::StopReason::MaxTurnRequests) => (StopReason::MaxTurnRequests, None),
            Some(v2::StopReason::Refusal) => (StopReason::Refusal, None),
            Some(v2::StopReason::Error(error)) => (
                StopReason::Failed,
                error.error.map(|error| error.message.to_string()),
            ),
            _ => (StopReason::Other, None),
        };
        let tokens = super::tokens::reported_tokens(
            idle.meta.as_ref(),
            idle.usage.as_ref().map(|usage| usage.total_tokens),
        );
        self.live.send_modify(|live| live.spent(tokens));
        if self.compactor.running() {
            let result = if stop == StopReason::EndTurn {
                Ok(PromptResponse::new(
                    agent_client_protocol::v1::schema::StopReason::EndTurn,
                ))
            } else {
                Err(Error::internal_error()
                    .data(error.unwrap_or_else(|| "compaction did not complete".into())))
            };
            self.compacted(cx, result);
        } else {
            self.turn_done(cx, stop, error, by_user);
        }
    }
}
