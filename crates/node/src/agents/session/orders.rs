//! What streams order a session to do, and the questions it asks.

use super::{
    Answered, Command, MAX_ASKED, MAX_PLAN, MAX_QUEUED, MAX_QUEUED_BYTES,
    actor::{Actor, Asked, Reply, Waiting, agent_title},
};
use crate::agents::{
    acp::{self, Prompt},
    live::cut,
};
use agent_client_protocol::{
    Agent, ConnectionTo, Responder,
    schema::v1::{
        CancelNotification, RequestPermissionRequest, RequestPermissionResponse,
        SetSessionModeRequest,
    },
};
use anyhow::Result;
use beep_proto::ModelId;
use beep_proto::node::agent::{
    Alert, AlertKind, AnswerOption, Effort, EntryKind, MAX_QUESTION_LINE, OptionKind, Question,
    Status,
};

impl Actor {
    pub(super) fn command(
        &mut self,
        cx: &ConnectionTo<Agent>,
        command: Command,
    ) -> Result<Result<(), String>> {
        match command {
            Command::Prompt(prompt) => {
                if let Err(refused) = self.can_take(&prompt) {
                    return Ok(Err(refused));
                }
                if let Err(full) = queue_room(self.queued.iter().chain(&self.steers), &prompt) {
                    return Ok(Err(full));
                }
                self.queued.push_back(prompt);
                self.show_queued();
                if self.live.borrow().status() == Status::Idle {
                    self.next_prompt(cx);
                }
                Ok(Ok(()))
            }
            Command::Steer(prompt) => {
                if let Err(refused) = self.can_take(&prompt) {
                    return Ok(Err(refused));
                }
                if let Err(full) = queue_room(self.queued.iter().chain(&self.steers), &prompt) {
                    return Ok(Err(full));
                }
                self.steers.push_back(prompt);
                self.show_queued();
                if self.live.borrow().status() == Status::Idle {
                    self.next_prompt(cx);
                }
                Ok(Ok(()))
            }
            Command::Answer { question, reply } => self.answer(question, &reply),
            Command::DropSteers => {
                self.steers.clear();
                self.show_queued();
                Ok(Ok(()))
            }
            Command::Cancel => {
                self.queued.clear();
                self.steers.clear();
                self.show_queued();
                while let Some(asked) = self.asked.pop_front() {
                    asked.waiting.cancel()?;
                }
                if let Some(session) = &self.acp_session
                    && self.live.borrow().status() != Status::Idle
                {
                    cx.send_notification(CancelNotification::new(session.clone()))?;
                }
                self.show_question();
                self.stop_due();
                Ok(Ok(()))
            }
            Command::Close => Ok(Ok(())),
            Command::Ask { question, reply } => {
                let question = Question {
                    id: 0,
                    text: cut(&question.text, MAX_ASKED),
                    detail: question.detail.map(|detail| cut(&detail, MAX_ASKED)),
                    options: question.options,
                };
                self.put(question, Waiting::Node(reply), None);
                Ok(Ok(()))
            }
            Command::Withdraw => {
                let before = self.asked.len();
                self.asked.retain(
                    |asked| !matches!(&asked.waiting, Waiting::Node(reply) if reply.is_closed()),
                );
                if self.asked.len() != before {
                    self.live.send_modify(|live| {
                        live.push(
                            EntryKind::Question,
                            "Withdrawn: nobody waits for the answer",
                        );
                    });
                    self.show_question();
                }
                Ok(Ok(()))
            }
            Command::Configure { model, effort } => Ok(self.configure(cx, model, effort)),
            Command::SetMode(mode) => Ok(self.set_mode(cx, mode)),
            Command::StopTask(task) => Ok(self.stop_task(cx, task)),
        }
    }

    /// Asks the agent to stop `task`, one of its running background tasks
    /// that can be stopped. The task shows as stopped once the agent says so.
    fn stop_task(&mut self, cx: &ConnectionTo<Agent>, task: String) -> Result<(), String> {
        let Some(session) = self.acp_session.clone() else {
            return Err("the agent is still starting".into());
        };
        let live = self.live.borrow();
        let Some(known) = live.info.state.tasks.iter().find(|known| known.id == task) else {
            return Err(format!("no task {task} here"));
        };
        if known.state.ended() {
            return Err(format!("the task {task} has ended"));
        }
        if !known.can_stop {
            return Err(format!("the task {task} cannot be stopped"));
        }
        drop(live);
        let request = acp::stop_task(&session, &task).map_err(|e| e.to_string())?;
        self.send(cx, request, move |result| Reply::TaskStopped {
            task,
            result,
        });
        Ok(())
    }

    /// Switches the session to `mode`, one it offers. It shows at once, and
    /// goes back if the agent refuses.
    fn set_mode(&mut self, cx: &ConnectionTo<Agent>, mode: String) -> Result<(), String> {
        let Some(session) = self.acp_session.clone() else {
            return Err("the agent is still starting; switch once it is ready".into());
        };
        let live = self.live.borrow();
        if !live.info.modes.iter().any(|offered| offered.id == mode) {
            return Err(format!(
                "{} here has no mode {mode}",
                agent_title(&self.agent)
            ));
        }
        let was = live.info.mode.clone();
        drop(live);
        let request = SetSessionModeRequest::new(session, mode.clone());
        self.send(cx, request, move |result| Reply::ModeSet {
            was,
            result: result.map(drop),
        });
        self.mode_is(cx, mode);
        Ok(())
    }

    /// The session is in `mode` now. Once an approved plan took it out of plan
    /// mode, a mode other than the one it left plan mode for is switched back.
    pub(super) fn mode_is(&mut self, cx: &ConnectionTo<Agent>, mode: String) {
        let mut back = None;
        if mode != acp::PLAN_MODE {
            self.left_plan_for = Some(mode.clone());
            back = self.returning.take().filter(|back| *back != mode);
        }
        self.live.send_if_modified(|live| {
            let changed = live.info.mode.as_deref() != Some(mode.as_str());
            live.info.mode = Some(mode);
            changed
        });
        if let Some(back) = back
            && let Err(e) = self.set_mode(cx, back)
        {
            log::warn!("the session did not go back after its plan: {e}");
        }
    }

    /// Puts a plan the agent wants to go ahead with to the user, as a question
    /// with two answers an app of any age can show, while the session is in
    /// plan mode. Gives the responder back for any other request.
    pub(super) fn plan_ready(
        &mut self,
        request: &RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) -> Option<Responder<RequestPermissionResponse>> {
        if self.live.borrow().info.mode.as_deref() != Some(acp::PLAN_MODE) {
            return Some(responder);
        }
        let Some(plan) = acp::plan_of(request) else {
            return Some(responder);
        };
        let question = self.live.borrow().question(0, request);
        let answers = acp::allow_option(&question.options)
            .zip(question.options.iter().find(|option| option.kind.rejects()));
        let Some((allow, reject)) = answers else {
            return Some(responder);
        };
        let answer = |option: &AnswerOption, name: &str, kind| AnswerOption {
            id: option.id.clone(),
            name: name.to_owned(),
            kind,
        };
        let options = vec![
            answer(allow, "Approve and start", OptionKind::AllowOnce),
            answer(reject, "Keep planning", OptionKind::RejectOnce),
        ];
        let question = Question {
            id: 0,
            text: "Plan ready".to_owned(),
            detail: Some(cut(&plan, MAX_QUESTION_LINE)).filter(|detail| !detail.is_empty()),
            options,
        };
        self.put(
            question,
            Waiting::Agent(responder),
            Some(cut(&plan, MAX_PLAN)),
        );
        None
    }

    /// Switches the running session to `model` and `effort`, which the
    /// agent applies from its next turn. Refused, sending nothing, when it
    /// cannot switch what is asked.
    fn configure(
        &mut self,
        cx: &ConnectionTo<Agent>,
        model: Option<ModelId>,
        effort: Option<Effort>,
    ) -> Result<(), String> {
        let Some(session) = self.acp_session.clone() else {
            return Err("the agent is still starting; switch once it is ready".into());
        };
        let title = agent_title(&self.agent);
        let mut requests = Vec::new();
        if let Some(model) = model {
            let Some(switch) = &self.switch else {
                return Err(format!(
                    "{title} here cannot switch models in a running session"
                ));
            };
            let request = acp::switch_model(&session, switch, model.as_str())
                .map_err(|e| format!("{title} could not switch to {model}: {e}"))?;
            requests.push((request, model.to_string()));
        }
        if let Some(effort) = effort {
            let Some(id) = &self.thinking else {
                return Err(format!("{title} here cannot change how hard it thinks"));
            };
            let request = acp::set_effort(&session, id, effort)
                .map_err(|e| format!("{title} could not think at {}: {e}", effort.as_str()))?;
            requests.push((request, format!("thinking {}", effort.as_str())));
        }
        if requests.is_empty() {
            return Err("nothing to switch".into());
        }
        for (request, what) in requests {
            self.send(cx, request, move |result| Reply::Reconfigured {
                what,
                result: result.map(drop),
            });
        }
        Ok(())
    }

    /// Refuses a prompt with images the agent does not take, rather than
    /// send it without them.
    fn can_take(&self, prompt: &Prompt) -> Result<(), String> {
        if prompt.images.is_empty() {
            return Ok(());
        }
        match self.takes_images {
            Some(true) => Ok(()),
            Some(false) => Err(format!(
                "{} here does not take images",
                agent_title(&self.agent)
            )),
            None => {
                Err("the agent is still starting; send the images again once it is ready".into())
            }
        }
    }

    /// Allows a permission request the agent sent despite running without
    /// prompts, as `acp::allow_option` picks. Gives the responder back when
    /// the request is to be asked after all: a choice for the user, or a
    /// request that offers no way to allow.
    pub(super) fn allow(
        &mut self,
        request: &RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) -> Result<Option<Responder<RequestPermissionResponse>>> {
        let question = self.live.borrow().question(0, request);
        let Some(option) = acp::allow_option(&question.options) else {
            return Ok(Some(responder));
        };
        responder.respond(acp::selected(&option.id))?;
        self.live.send_modify(|live| {
            live.push(EntryKind::Question, &format!("Allowed: {}", question.text));
        });
        Ok(None)
    }

    pub(super) fn ask(
        &mut self,
        request: &RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) {
        let question = self.live.borrow().question(0, request);
        self.put(question, Waiting::Agent(responder), None);
    }

    /// Shows `question` once those before it are answered, numbered next.
    fn put(&mut self, mut question: Question, waiting: Waiting, plan: Option<String>) {
        question.id = self.next_question;
        self.next_question += 1;
        self.live.send_modify(|live| {
            let detail = question
                .detail
                .as_deref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default();
            live.push(
                EntryKind::Question,
                &format!("Asked: {}{detail}", question.text),
            );
        });
        self.asked.push_back(Asked {
            question,
            waiting,
            plan,
        });
        self.show_question();
    }

    fn answer(&mut self, question: u32, reply: &str) -> Result<Result<(), String>> {
        let Some(asked) = self.asked.front() else {
            return Ok(Err("there is no open question".into()));
        };
        if asked.question.id != question {
            return Ok(Err(format!(
                "question {question} is not open; question {} is",
                asked.question.id
            )));
        }
        let options = &asked.question.options;
        if matches!(asked.waiting, Waiting::Node(_)) {
            // The node's own question takes any words as the answer.
            let answered = match choose(options, reply) {
                Some(option) => Answered::Picked(option.clone()),
                None => Answered::Said(reply.trim().to_owned()),
            };
            let name = match &answered {
                Answered::Picked(option) => option.name.clone(),
                Answered::Said(text) => cut(text, MAX_ASKED),
            };
            if let Some(Asked {
                waiting: Waiting::Node(sender),
                ..
            }) = self.asked.pop_front()
            {
                // The call that asked may have given up; the answer stands.
                let _ = sender.send(answered);
            }
            self.live.send_modify(|live| {
                live.push(EntryKind::Question, &format!("Answered: {name}"));
            });
            self.show_question();
            return Ok(Ok(()));
        }
        let (chosen, follow_up) = match choose(options, reply) {
            Some(option) => (Some(option), None),
            // Anything else declines and goes to the agent as its next prompt.
            None => (
                options
                    .iter()
                    .find(|option| option.kind == OptionKind::RejectOnce)
                    .or_else(|| options.iter().find(|option| option.kind.rejects())),
                Some(reply.to_owned()),
            ),
        };
        let approves_plan = asked.plan.is_some() && chosen.is_some_and(|o| !o.kind.rejects());
        let follow_up = follow_up.map(Prompt::from);
        if let Some(prompt) = &follow_up
            && let Err(full) = queue_room(&self.queued, prompt)
        {
            return Ok(Err(full));
        }
        let answer = match chosen {
            Some(option) => acp::selected(&option.id),
            None => acp::cancelled(),
        };
        let name = chosen
            .map_or("declined", |option| option.name.as_str())
            .to_owned();
        if let Some(Asked {
            waiting: Waiting::Agent(responder),
            ..
        }) = self.asked.pop_front()
        {
            responder.respond(answer)?;
        }
        if approves_plan {
            self.returning = self
                .left_plan_for
                .clone()
                .or_else(|| self.bypass_mode.map(str::to_owned));
        }
        if let Some(prompt) = follow_up {
            self.queued.push_front(prompt);
            self.show_queued();
        }
        self.live.send_modify(|live| {
            live.push(EntryKind::Question, &format!("Answered: {name}"));
        });
        self.show_question();
        Ok(Ok(()))
    }

    /// Shows the first waiting question, or none, in the state.
    fn show_question(&mut self) {
        let question = self.asked.front().map(|asked| asked.question.clone());
        let plan = self.asked.front().and_then(|asked| asked.plan.clone());
        let shown = self.live.borrow().info.question.as_ref().map(|q| q.id);
        if let Some(asks) = question.as_ref().filter(|q| Some(q.id) != shown) {
            self.alert(AlertKind::Asks {
                question: asks.clone(),
            });
        }
        self.live.send_if_modified(|live| {
            let status = match (&question, live.status()) {
                (Some(_), _) => Status::Asking,
                (None, Status::Asking) => Status::Working,
                (None, status) => status,
            };
            let changed = live.info.question != question || live.info.state.status != status;
            live.info.question = question;
            live.info.plan_ready = plan;
            live.info.state.status = status;
            changed
        });
    }

    /// Has `kind` wait for the server, about this session as it stands.
    pub(super) fn alert(&self, kind: AlertKind) {
        let live = self.live.borrow();
        // The id the session is listed under now: once the agent gives its
        // own, `list` shows that one, and a notice must name what it finds.
        let alert = Alert {
            session: live.info.id.clone(),
            agent: self.agent.clone(),
            title: live.info.title.clone(),
            cwd: live.info.cwd.clone(),
            kind,
        };
        drop(live);
        self.alerts.record(alert);
    }
}

/// Refuses a prompt the queue has no room for: too many waiting already,
/// or too many bytes with this one.
pub(super) fn queue_room<'a>(
    queued: impl IntoIterator<Item = &'a Prompt>,
    prompt: &Prompt,
) -> Result<(), String> {
    let mut count = 0usize;
    let waiting = queued.into_iter().fold(prompt.size(), |n, queued| {
        count += 1;
        n.saturating_add(queued.size())
    });
    if count >= MAX_QUEUED {
        return Err(format!("{MAX_QUEUED} prompts already wait for this turn"));
    }
    if waiting > MAX_QUEUED_BYTES {
        return Err(format!(
            "Too much already waits for this turn (over {} MB); send this once it is done",
            MAX_QUEUED_BYTES >> 20
        ));
    }
    Ok(())
}

/// The option `reply` names: by id, number, name or kind, or plain yes/no.
pub(super) fn choose<'a>(options: &'a [AnswerOption], reply: &str) -> Option<&'a AnswerOption> {
    let reply = reply.trim().to_lowercase();
    let by_kind = |kind: OptionKind| options.iter().find(|option| option.kind == kind);
    options
        .iter()
        .find(|option| option.id.to_lowercase() == reply)
        .or_else(|| {
            let n: usize = reply.parse().ok()?;
            options.get(n.checked_sub(1)?)
        })
        .or_else(|| {
            options
                .iter()
                .find(|option| option.name.to_lowercase() == reply)
        })
        .or_else(|| options.iter().find(|option| option.kind.as_str() == reply))
        .or_else(|| match reply.as_str() {
            "allow" | "yes" | "y" | "approve" | "ok" => by_kind(OptionKind::AllowOnce),
            "always" => by_kind(OptionKind::AllowAlways),
            "reject" | "deny" | "no" | "n" => by_kind(OptionKind::RejectOnce),
            _ => None,
        })
}
