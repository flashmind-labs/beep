//! What streams order a session to do, and the questions it asks.

use super::{
    actor::{Actor, Asked, Reply, Waiting, agent_title},
    types::{Answered, Command, MAX_ASKED, MAX_PLAN, MAX_QUEUED, MAX_QUEUED_BYTES},
};
use crate::agents::{
    acp::{self, Prompt},
    live::Live,
};
use crate::text::cut;
use agent_client_protocol::{
    Agent, ConnectionTo, Responder,
    v1::schema::{CancelNotification, RequestPermissionResponse},
};
use anyhow::Result;
use beep_proto::ModelId;
use beep_proto::node::agent::{
    Alert, AlertKind, AnswerOption, Effort, EntryKind, MAX_QUESTION_LINE, OptionKind, Question,
    Status,
};
use beep_proto::watched;

/// What a prompt hears while its session moves.
const MOVING_REFUSAL: &str =
    "This session is moving to another machine. Send the message there once it has moved.";

impl Actor {
    pub(super) fn command(
        &mut self,
        cx: &ConnectionTo<Agent>,
        command: Command,
    ) -> Result<Result<(), String>> {
        match command {
            Command::RecoverCodexChildren(runs) => {
                self.live
                    .send_if_modified(|live| live.recover_codex_children(&runs));
                self.subagents_moved();
                Ok(Ok(()))
            }

            Command::Prompt(prompt) => self.enqueue(cx, prompt, false),
            Command::Steer(prompt) => self.enqueue(cx, prompt, true),
            Command::Answer { question, reply } => self.answer(cx, question, &reply),
            Command::DropSteers => {
                self.backlog.drop_steers();
                self.show_queued();
                Ok(Ok(()))
            }
            Command::Cancel { by_user } => {
                self.cancel(cx, by_user)?;
                Ok(Ok(()))
            }
            Command::StopAll { by_user } => {
                self.cancel(cx, by_user)?;
                self.stop_tasks(cx);
                Ok(Ok(()))
            }
            Command::Close { .. } | Command::ParentGone => Ok(Ok(())),
            Command::Moving { stop } => {
                self.moving = true;

                if stop {
                    // As a stop does: what waited behind the turn goes.
                    self.cancel(cx, false)?;
                }

                Ok(Ok(()))
            }
            Command::NotMoving => {
                self.moving = false;

                if self.can_prompt() {
                    self.next_prompt(cx);
                }

                Ok(Ok(()))
            }
            Command::Ask { question, reply } => {
                let question = Question {
                    id: 0,
                    text: question.text,
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
                    self.resume_after_question(cx)?;
                }

                Ok(Ok(()))
            }
            Command::Configure {
                model,
                effort,
                fast,
            } => Ok(self.configure(cx, model, effort, fast)),
            Command::SetMode(mode) => Ok(self.set_mode(cx, mode)),
            Command::StopTask(task) => Ok(self.stop_task(cx, task)),
            Command::SetDevice(device) => {
                self.live.send_if_modified(|live| {
                    let changed = live.info.device != device;
                    live.info.device = device;
                    changed
                });
                Ok(Ok(()))
            }
            Command::Sending(sent) => {
                self.live.send_if_modified(|live| live.sending(sent));
                Ok(Ok(()))
            }
            // These three are read through the watch but wake nobody.
            Command::Saw(marks) => {
                self.live.send_if_modified(|live| {
                    live.seen.extend(marks);
                    false
                });
                Ok(Ok(()))
            }
            Command::UserTurn(user) => {
                self.live.send_if_modified(|live| {
                    live.user_turn = user;
                    false
                });
                Ok(Ok(()))
            }
            Command::TakeUserTurn(reply) => {
                self.live.send_if_modified(|live| {
                    // The caller may have stopped waiting.
                    let _ = reply.send(std::mem::take(&mut live.user_turn));
                    false
                });
                Ok(Ok(()))
            }
        }
    }

    /// Whether `prompt` fits beside the queued prompts and the steers.
    fn room_for(&self, prompt: &Prompt) -> Result<(), String> {
        queue_room(self.backlog.prompts(), prompt)
    }

    /// Queues a prompt, or a steer, and starts it when the agent is idle.
    fn enqueue(
        &mut self,
        cx: &ConnectionTo<Agent>,
        prompt: Prompt,
        steer: bool,
    ) -> Result<Result<(), String>> {
        if self.moving {
            return Ok(Err(MOVING_REFUSAL.to_owned()));
        }

        // Words sent while a form asks are its answer, as the agent waits.
        if let Some(asked) = self.asked.front()
            && matches!(asked.waiting, Waiting::Form(_))
            && prompt.images.is_empty()
        {
            let question = asked.question.id;
            return self.answer(cx, question, &prompt.text);
        }

        let prompt = self.fitted(prompt);

        if let Err(full) = self.room_for(&prompt) {
            return Ok(Err(full));
        }

        self.backlog.push(prompt, steer);
        self.show_queued();

        if self.can_prompt() {
            self.next_prompt(cx);
        } else if steer && self.offers.takes_steering {
            self.steer_now(cx)?;
        }

        Ok(Ok(()))
    }

    /// Stops the main agent's turn (`session/cancel`) and drops the prompts
    /// queued behind it and the open questions. Steers stay for the next
    /// prompt when the user stopped it (see `Command::Cancel`). What the
    /// agent runs apart from its turn, its subagents and background
    /// commands, is left to the agent: Claude Code spares them, as the
    /// session declares a stop for each task (`acp::new_session`).
    fn cancel(&mut self, cx: &ConnectionTo<Agent>, by_user: bool) -> Result<()> {
        self.backlog.cancel(by_user);
        self.by_user |= by_user && self.turn.is_some();
        self.show_queued();

        while let Some(asked) = self.asked.pop_front() {
            asked.waiting.cancel()?;
        }

        if let Some(session) = &self.acp_session
            && watched::read_sent(&self.live, Live::status) != Status::Idle
        {
            cx.send_notification(CancelNotification::new(session.clone()))?;
        }

        self.show_question();
        self.stop_due();
        Ok(())
    }

    /// Asks the agent to stop each of its background tasks that still runs
    /// and can be stopped, for Stop all.
    fn stop_tasks(&mut self, cx: &ConnectionTo<Agent>) {
        let running: Vec<String> = watched::read_sent(&self.live, |live| {
            live.info
                .state
                .tasks
                .iter()
                .filter(|task| task.can_stop && !task.state.ended())
                .map(|task| task.id.clone())
                .collect()
        });

        for task in running {
            if let Err(e) = self.stop_task(cx, task) {
                log::debug!("a background task was not stopped: {e}");
            }
        }
    }

    /// Asks the agent to stop `task`, one of its running background tasks
    /// that can be stopped. The task shows as stopped once the agent says so.
    fn stop_task(&mut self, cx: &ConnectionTo<Agent>, task: String) -> Result<(), String> {
        let Some(session) = self.acp_session.clone() else {
            return Err("the agent is still starting".into());
        };
        let known = watched::read_sent(&self.live, |live| {
            live.info
                .state
                .tasks
                .iter()
                .find(|known| known.id == task)
                .map(|known| (known.state.ended(), known.can_stop))
        });
        let Some((ended, can_stop)) = known else {
            return Err(format!("no task {task} here"));
        };

        if ended {
            return Err(format!("the task {task} has ended"));
        }

        if !can_stop {
            return Err(format!("the task {task} cannot be stopped"));
        }

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
        let (offered, was) = watched::read_sent(&self.live, |live| {
            (
                live.info.modes.iter().any(|offered| offered.id == mode),
                live.info.mode.clone(),
            )
        });

        if !offered {
            return Err(format!(
                "{} here has no mode {mode}",
                agent_title(&self.agent)
            ));
        }

        let request = self
            .mode_request(session, &mode)
            .map_err(|error| error.to_string())?;
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
        plan: Option<String>,
        asked: &Question,
        responder: Responder<RequestPermissionResponse>,
    ) -> Option<Responder<RequestPermissionResponse>> {
        if watched::read_sent(&self.live, |live| {
            live.info.mode.as_deref() != Some(acp::PLAN_MODE)
        }) {
            return Some(responder);
        }

        let Some(plan) = plan else {
            return Some(responder);
        };
        let answers = acp::allow_option(&asked.options)
            .zip(asked.options.iter().find(|option| option.kind.rejects()));
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
            Waiting::Agent(Box::new(responder)),
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
        fast: Option<bool>,
    ) -> Result<(), String> {
        let Some(session) = self.acp_session.clone() else {
            return Err("the agent is still starting; switch once it is ready".into());
        };
        let title = agent_title(&self.agent);
        let mut requests = Vec::new();

        if let Some(model) = model {
            let Some(switch) = &self.offers.switch else {
                return Err(format!(
                    "{title} here cannot switch models in a running session"
                ));
            };
            let request = acp::switch_model(&session, switch, model.as_ref())
                .map_err(|e| format!("{title} could not switch to {model}: {e}"))?;
            requests.push((request, model.to_string(), Some(model), None, None));
        }

        if let Some(effort) = effort {
            let Some(id) = &self.offers.thinking else {
                return Err(format!("{title} here cannot change how hard it thinks"));
            };
            let request = acp::set_effort(&session, id, effort)
                .map_err(|e| format!("{title} could not think at {}: {e}", effort.as_ref()))?;
            requests.push((
                request,
                format!("thinking {}", effort.as_ref()),
                None,
                Some(effort),
                None,
            ));
        }

        if let Some(fast) = fast {
            let id =
                self.offers.fast_option.as_ref().ok_or_else(|| {
                    format!("{title} here does not offer fast mode for this model")
                })?;
            let request = acp::set_fast(&session, id, fast).map_err(|e| e.to_string())?;
            requests.push((
                request,
                format!("fast mode {}", if fast { "on" } else { "off" }),
                None,
                None,
                Some(fast),
            ));
        }

        if requests.is_empty() {
            return Err("nothing to switch".into());
        }

        for (request, what, model, effort, fast) in requests {
            self.send(cx, request, move |result| Reply::Reconfigured {
                what,
                model,
                effort,
                fast,
                result: result.map(drop),
            });
        }

        Ok(())
    }

    /// Keep images until initialization tells us whether the agent takes them.
    fn fitted(&self, mut prompt: Prompt) -> Prompt {
        if self.offers.takes_images == Some(false) {
            prompt.images.clear();
        }

        prompt
    }

    /// Allows a permission request the agent sent despite running without
    /// prompts, as `acp::allow_option` picks. Gives the responder back when
    /// the request is to be asked after all: a choice for the user, or a
    /// request that offers no way to allow.
    pub(super) fn allow(
        &mut self,
        question: &Question,
        responder: Responder<RequestPermissionResponse>,
    ) -> Result<Option<Responder<RequestPermissionResponse>>> {
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
        question: Question,
        responder: Responder<RequestPermissionResponse>,
    ) {
        self.put(question, Waiting::Agent(Box::new(responder)), None);
    }

    /// Puts the next field of `form` to the user.
    pub(super) fn ask_form(&mut self, form: acp::Form) {
        self.put(form.question().clone(), Waiting::Form(Box::new(form)), None);
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

    fn answer(
        &mut self,
        cx: &ConnectionTo<Agent>,
        question: u32,
        reply: &str,
    ) -> Result<Result<(), String>> {
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

        if matches!(asked.waiting, Waiting::Form(_)) {
            // A form takes any words as the answer, and asks its next field.
            let Some(Asked {
                waiting: Waiting::Form(form),
                ..
            }) = self.asked.pop_front()
            else {
                return Ok(Ok(()));
            };
            match form.answer(reply, |options, reply| choose(options, reply).cloned())? {
                acp::Answer::Next(form) => {
                    self.answered(reply.trim());
                    self.ask_form(*form);
                }
                acp::Answer::Sent(name) => {
                    self.answered(&name);
                    self.resume_after_question(cx)?;
                }
            }
            return Ok(Ok(()));
        }

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
            self.answered(&name);
            self.resume_after_question(cx)?;
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
            && let Err(full) = self.room_for(prompt)
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
            (*responder).respond(answer)?;
        }
        if approves_plan {
            self.returning = self
                .left_plan_for
                .clone()
                .or_else(|| self.full_mode().map(str::to_owned));
        }

        if let Some(prompt) = follow_up {
            self.backlog.push_front(prompt);
            self.show_queued();
        }

        self.answered(&name);
        self.resume_after_question(cx)?;
        Ok(Ok(()))
    }

    fn answered(&mut self, name: &str) {
        self.live.send_modify(|live| {
            live.push(EntryKind::Question, &format!("Answered: {name}"));
        });
        self.show_question();
    }

    /// Sends a prompt or steer that waited for the open questions, once the
    /// last one is answered or withdrawn.
    fn resume_after_question(&mut self, cx: &ConnectionTo<Agent>) -> Result<()> {
        if !self.asked.is_empty() {
            return Ok(());
        }

        if self.can_prompt() {
            if self.backlog.has_prompts() {
                self.next_prompt(cx);
            }
        } else if self.offers.takes_steering {
            self.steer_now(cx)?;
        }

        Ok(())
    }

    /// Shows the first waiting question, or none, in the state.
    fn show_question(&mut self) {
        let question = self.asked.front().map(|asked| asked.question.clone());
        let plan = self.asked.front().and_then(|asked| asked.plan.clone());
        let shown =
            watched::read_sent(&self.live, |live| live.info.question.as_ref().map(|q| q.id));

        if let Some(asks) = question.as_ref()
            && Some(asks.id) != shown
        {
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
        // Child results reach the parent through family updates.
        if matches!(kind, AlertKind::TurnEnded { .. })
            && watched::read_sent(&self.live, |live| live.info.parent.is_some())
        {
            return;
        }

        let named = watched::read_sent(&self.live, |live| {
            live.agent_session.is_some().then(|| {
                (
                    live.info.id.clone(),
                    live.info.title.clone(),
                    live.info.cwd.clone(),
                )
            })
        });
        // Startup failures are returned to the caller, without publishing an unnamed session.
        let Some((session, title, cwd)) = named else {
            return;
        };

        self.node.alerts.record(Alert {
            session,
            agent: self.agent.clone(),
            title,
            cwd,
            kind,
        });
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
        .or_else(|| options.iter().find(|option| option.kind.as_ref() == reply))
        .or_else(|| match reply.as_str() {
            "allow" | "yes" | "y" | "approve" | "ok" => by_kind(OptionKind::AllowOnce),
            "always" => by_kind(OptionKind::AllowAlways),
            "reject" | "deny" | "no" | "n" => by_kind(OptionKind::RejectOnce),
            _ => None,
        })
}
