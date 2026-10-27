//! Runs a research's team in the background, one round at a time: the
//! round's scouts in parallel, then the critic's review, then the lead, who
//! may add scouts for the gaps and briefs the writer, then the writer's
//! draft (or revision, after the first round) and one round of fixes. When
//! it ends, the lead tells the user in the research chat.

use super::{App, Draws, Message, run::register_web_tools};
use crate::{
    agents::AgentId,
    metering,
    protocol::DeviceKind,
    repeat,
    research::{
        AddScoutsTool, Entry, MAX_TEAM_SCOUTS, Member, Plan, ResearchConfig, ResearchMoreTool,
        RoleModel, Scout, Seat, StartResearchTool, Status, StopResearchTool, TEAM_TOOLS, TeamTools,
        team_tools,
    },
    store::{CreateRun, ResearchRow},
    user::User,
};
use anyhow::{Context, Result};
use chrono::Utc;
use flashmind::{
    core::{Agent, CancellationToken, Conversation},
    types::{AgentEvent, AgentInput, ToolRegistry, tool::Tool},
};
use futures::{StreamExt, future::join_all};
use secrecy::ExposeSecret;
use std::{
    collections::HashMap,
    fmt::Write,
    sync::{Arc, Mutex, atomic::Ordering},
    time::Duration,
};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// The most teams the server runs at once, across users.
const MAX_TEAMS: usize = 4;
/// The longest one member's turn may take.
const MEMBER_RUN: Duration = Duration::from_secs(10 * 60);
/// How long a member's turn may go without an event.
const IDLE_TIMEOUT: Duration = Duration::from_secs(150);
/// The longest final reply a member posts to the room.
const MAX_SAID: usize = 4000;
/// The draft the result turn reads, at most.
const MAX_DRAFT_READ: usize = 40_000;

const SCOUT_PROMPT: &str = include_str!("../../prompts/research/scout.md");
const CRITIC_PROMPT: &str = include_str!("../../prompts/research/critic.md");
const WRITER_PROMPT: &str = include_str!("../../prompts/research/writer.md");
const COORDINATOR_PROMPT: &str = include_str!("../../prompts/research/coordinator.md");
/// The lead's prompt in the research chat.
pub(super) const LEAD_PROMPT: &str = include_str!("../../prompts/research/lead.md");

/// What the result turn's message shows in the chat.
const FINISHED_LABEL: &str = "Research finished";

/// One member's turn.
struct Turn<'a> {
    research: &'a ResearchRow,
    plan: &'a Plan,
    member: Member,
    model: &'a RoleModel,
    /// Its conversation so far, empty on its first turn.
    conversation: Conversation,
    task: String,
    /// Tools for this turn only, besides its role's.
    extra: Vec<Arc<dyn Tool>>,
}

impl<'a> Turn<'a> {
    /// A member's first turn.
    fn new(
        research: &'a ResearchRow,
        plan: &'a Plan,
        member: Member,
        model: &'a RoleModel,
        task: String,
    ) -> Self {
        Self {
            research,
            plan,
            member,
            model,
            conversation: Conversation::new(),
            task,
            extra: Vec::new(),
        }
    }
}

/// Gives the lead's turn its tools: starting and stopping the team, and
/// reading what it found.
pub(super) fn register_lead(
    registry: &mut ToolRegistry,
    app: &Arc<App>,
    user: &Arc<User>,
    id: Uuid,
) {
    registry.register(Arc::new(StartResearchTool {
        app: app.clone(),
        user: user.clone(),
        research: id,
    }));
    registry.register(Arc::new(ResearchMoreTool {
        app: app.clone(),
        user: user.clone(),
        research: id,
    }));
    registry.register(Arc::new(StopResearchTool {
        app: app.clone(),
        research: id,
    }));
    let seat = Seat {
        store: app.store.clone(),
        research: id,
        member: Member::Lead,
    };
    let TeamTools(tools) = team_tools(seat);
    for tool in tools {
        registry.register(tool);
    }
}

impl App {
    /// What the lead is told about its research each turn.
    pub(super) async fn research_context(
        &self,
        research: &ResearchRow,
        config: &ResearchConfig,
    ) -> Result<String> {
        let mut ctx = String::from("## Research\n");
        let _ = writeln!(ctx, "Topic: {}", research.topic);
        let _ = writeln!(ctx, "Status: {}", research.status.as_str());
        if let Some(plan) = &research.plan {
            let _ = writeln!(ctx, "Round: {}", plan.round());
        }
        let model = |model: &RoleModel| {
            let id = model.model.to_string();
            match self.models.name_of(&model.model) {
                Some(name) => format!("{name} ({id})"),
                None => id,
            }
        };
        match &research.plan {
            Some(plan) => {
                let _ = writeln!(ctx, "The team:");
                for seat in plan.team(&config.lead) {
                    let round = match seat.round {
                        0 => String::new(),
                        n => format!(" (round {n})"),
                    };
                    let _ = writeln!(
                        ctx,
                        "- {}{round}: {} on {}",
                        seat.member,
                        seat.role,
                        model(seat.model)
                    );
                }
                let _ = writeln!(ctx, "Scout models for a follow-up:");
                let _ = writeln!(ctx, "- {} (default)", model(&config.scout));
                for other in &config.scout_choices {
                    let other = RoleModel {
                        model: other.clone(),
                        thinking: config.scout.thinking,
                    };
                    let _ = writeln!(ctx, "- {}", model(&other));
                }
            }
            None => {
                let _ = writeln!(ctx, "Default scout model: {}", model(&config.scout));
                for other in &config.scout_choices {
                    let other = RoleModel {
                        model: other.clone(),
                        thinking: config.scout.thinking,
                    };
                    let _ = writeln!(ctx, "Other scout model: {}", model(&other));
                }
                let _ = writeln!(ctx, "Critic: {}", model(&config.critic));
                let _ = writeln!(ctx, "Writer: {}", model(&config.writer));
            }
        }
        if research.status == Status::Done {
            let sources = self.store.research_sources(research.id).await?.len();
            let _ = writeln!(ctx, "The paper is written, citing {sources} sources.");
        }
        Ok(ctx)
    }

    /// Starts the team on `plan`'s newest round. `false` when the team is
    /// working already.
    pub async fn start_team(
        self: &Arc<Self>,
        user: &Arc<User>,
        id: Uuid,
        plan: Plan,
    ) -> Result<bool> {
        let cancel = CancellationToken::new();
        {
            let mut teams = self.teams.lock().await;
            if teams.len() >= MAX_TEAMS && !teams.contains_key(&id) {
                anyhow::bail!("too many researches are running; try again later");
            }
            if teams.contains_key(&id) || !self.store.approve_research(id, &plan).await? {
                return Ok(false);
            }
            teams.insert(id, cancel.clone());
        }
        let (app, user) = (self.clone(), user.clone());
        tokio::spawn(async move {
            let work = async {
                metering::turn_agent(AgentId::Research);
                app.run_team(&user, id, &plan, &cancel).await
            };
            let ended = metering::in_turn(work).await;
            app.teams.lock().await.remove(&id);
            app.team_ended(&user, id, ended).await;
        });
        Ok(true)
    }

    /// Stops a research's team. Says whether it was working.
    pub async fn stop_team(&self, id: Uuid) -> Result<bool> {
        let stopped = self.store.stop_research(id).await?;
        if let Some(cancel) = self.teams.lock().await.get(&id) {
            cancel.cancel();
        }
        Ok(stopped)
    }

    /// Stops every team, as the server shuts down. Their researches stay
    /// running in the store, so the next start marks them interrupted.
    pub(super) async fn stop_teams(&self) {
        for cancel in self.teams.lock().await.values() {
            cancel.cancel();
        }
    }

    async fn run_team(
        self: &Arc<Self>,
        user: &Arc<User>,
        id: Uuid,
        plan: &Plan,
        cancel: &CancellationToken,
    ) -> Result<Status> {
        let research = self
            .store
            .research(id)
            .await?
            .context("the research is gone")?;
        let research = &research;
        let round = plan.round();
        let mut plan = plan.clone();
        let scout_task = |question: &str| {
            format!(
                "Research this question for the team: {question}\n\nKeep every source you rely \
                 on with add_source and record each finding with add_note, citing them. Post \
                 what the others need to the room. End with a short summary of what you found \
                 and how sure you are."
            )
        };

        // Every scout of the round on its own question at once; at most
        // `MAX_SCOUTS`.
        let asked: Vec<(Member, Scout)> = plan
            .scouts_of(round)
            .map(|(member, scout)| (member, scout.clone()))
            .collect();
        let scouting = asked.iter().map(|(member, scout)| {
            let task = scout_task(&scout.question);
            self.run_member(
                user,
                Turn::new(research, &plan, *member, &scout.model, task),
                cancel,
            )
        });
        let mut scouts: HashMap<Member, Conversation> = HashMap::new();
        for ((member, _), ran) in asked.iter().zip(join_all(scouting).await) {
            match ran {
                Ok(conversation) => {
                    scouts.insert(*member, conversation);
                }
                Err(error) => log::warn!("research: {member} failed: {error:#}"),
            }
        }
        if cancel.is_cancelled() {
            return Ok(Status::Stopped);
        }
        if scouts.is_empty() {
            anyhow::bail!("every scout failed");
        }
        if !self
            .store
            .move_research(id, Status::Researching, Status::Reviewing)
            .await?
        {
            return Ok(Status::Stopped);
        }

        // The critic challenges weak findings; each challenged scout answers once.
        let since = self.last_entry(id).await?;
        let critic = self
            .run_member(
                user,
                Turn::new(
                    research,
                    &plan,
                    Member::Critic,
                    &plan.critic,
                    "Review the team's notes with read_notes, the newest round's first. For \
                     each finding that is weak, unsourced or contradicted, post a challenge to \
                     the scout who made it with post_message, naming them in `to`. End with a \
                     short verdict on how solid the notes are."
                        .to_owned(),
                ),
                cancel,
            )
            .await?;
        let challenges = self.challenges(id, since).await?;
        let answering = asked.iter().filter_map(|(member, scout)| {
            let said = challenges.get(member)?;
            let conversation = scouts.remove(member)?;
            let task = format!(
                "The critic challenged your findings:\n{said}\n\nCheck them. Fix or add notes \
                 with sources where they are right, and answer in the room with post_message \
                 where they are not."
            );
            Some(self.run_member(
                user,
                Turn {
                    conversation,
                    ..Turn::new(research, &plan, *member, &scout.model, task)
                },
                cancel,
            ))
        });
        for answered in join_all(answering.collect::<Vec<_>>()).await {
            if let Err(error) = answered {
                log::warn!("research: a scout's answer failed: {error:#}");
            }
        }
        if cancel.is_cancelled() {
            return Ok(Status::Stopped);
        }

        // The lead reads where the round stands, puts scouts on what it
        // left open and briefs the writer.
        let lead = self.models.research().map(|config| config.lead.clone());
        let lead = lead.context("research is not set up")?;
        let added = Arc::new(Mutex::new(Vec::new()));
        let room = MAX_TEAM_SCOUTS.saturating_sub(plan.scouts.len());
        let mut extra: Vec<Arc<dyn Tool>> = Vec::new();
        if room > 0 {
            extra.push(Arc::new(AddScoutsTool {
                app: self.clone(),
                round,
                room,
                added: added.clone(),
            }));
        }
        let questions: String = asked
            .iter()
            .map(|(member, scout)| format!("- {member}: {}\n", scout.question))
            .collect();
        let task = format!(
            "Round {round} of the research. Its questions:\n{questions}\nRead the notes and \
             the room. If a gap that matters to the answer is left and a search could close \
             it, add scouts for it with add_scouts. Then brief the writer with post_message, \
             `to` writer: what the paper must answer, what to lead with and what to leave out."
        );
        self.run_member(
            user,
            Turn {
                extra,
                ..Turn::new(research, &plan, Member::Lead, &lead, task)
            },
            cancel,
        )
        .await?;
        let added = match added.lock() {
            Ok(mut added) => std::mem::take(&mut *added),
            Err(_) => anyhow::bail!("the added scouts are poisoned"),
        };
        if !added.is_empty() {
            let first = plan.scouts.len();
            plan.scouts.extend(added);
            self.store.set_research_plan(id, &plan).await?;
            let filling = plan.scouts[first..].iter().enumerate().map(|(i, scout)| {
                let task = scout_task(&scout.question);
                let member = Member::Scout(first + i + 1);
                self.run_member(
                    user,
                    Turn::new(research, &plan, member, &scout.model, task),
                    cancel,
                )
            });
            for filled in join_all(filling.collect::<Vec<_>>()).await {
                if let Err(error) = filled {
                    log::warn!("research: a scout on a gap failed: {error:#}");
                }
            }
        }
        if cancel.is_cancelled()
            || !self
                .store
                .move_research(id, Status::Reviewing, Status::Writing)
                .await?
        {
            return Ok(Status::Stopped);
        }

        // The writer drafts, or revises the paper after the first round,
        // the critic reviews it, the writer fixes it.
        let task = if round == 1 {
            format!(
                "Write the paper on: {}\n\nRead the notes with read_notes and the room with \
                 read_room, the lead's brief first, then save the whole paper with write_draft.",
                research.topic
            )
        } else {
            format!(
                "The team researched follow-up questions:\n{questions}\nRead the paper with \
                 read_draft, the new notes with read_notes and the lead's brief in the room. \
                 Revise the paper to answer them, keeping what still holds, and save the \
                 whole of it with write_draft."
            )
        };
        let writer = self
            .run_member(
                user,
                Turn::new(research, &plan, Member::Writer, &plan.writer, task),
                cancel,
            )
            .await?;
        let since = self.last_entry(id).await?;
        self.run_member(
            user,
            Turn {
                conversation: critic,
                ..Turn::new(
                    research,
                    &plan,
                    Member::Critic,
                    &plan.critic,
                    "Read the draft with read_draft. Post the few fixes that matter most to \
                     the writer with post_message, `to` writer. Skip style nitpicks."
                        .to_owned(),
                )
            },
            cancel,
        )
        .await?;
        if let Some(fixes) = self.challenges(id, since).await?.get(&Member::Writer) {
            self.run_member(
                user,
                Turn {
                    conversation: writer,
                    ..Turn::new(
                        research,
                        &plan,
                        Member::Writer,
                        &plan.writer,
                        format!(
                            "The critic asks for these fixes:\n{fixes}\n\nRevise the paper and \
                             save the whole of it again with write_draft."
                        ),
                    )
                },
                cancel,
            )
            .await?;
        }
        if cancel.is_cancelled() {
            return Ok(Status::Stopped);
        }
        let written = self.store.research(id).await?;
        if written.is_none_or(|row| row.draft.trim().is_empty()) {
            anyhow::bail!("the writer saved no paper");
        }
        if !self
            .store
            .move_research(id, Status::Writing, Status::Done)
            .await?
        {
            return Ok(Status::Stopped);
        }
        Ok(Status::Done)
    }

    /// The newest entry's number, so later reads see only what follows.
    async fn last_entry(&self, id: Uuid) -> Result<i64> {
        let entries = self.store.research_entries(id, 0).await?;
        Ok(entries.last().map_or(0, |row| row.seq))
    }

    /// What the critic said to each member since entry `since`, one
    /// message per line.
    async fn challenges(&self, id: Uuid, since: i64) -> Result<HashMap<Member, String>> {
        let mut out: HashMap<Member, String> = HashMap::new();
        for row in self.store.research_entries(id, since).await? {
            if row.member == Member::Critic
                && let Entry::Said {
                    to: Some(to),
                    content,
                } = row.entry
            {
                let said = out.entry(to).or_default();
                let _ = writeln!(said, "- {content}");
            }
        }
        Ok(out)
    }

    /// Runs one member's turn and returns its conversation. Its tool calls
    /// go to its thread as steps and its final reply to the room.
    async fn run_member(
        &self,
        user: &User,
        turn: Turn<'_>,
        cancel: &CancellationToken,
    ) -> Result<Conversation> {
        let Turn {
            research,
            plan,
            member,
            model,
            mut conversation,
            task,
            extra,
        } = turn;
        let seat = Seat {
            store: self.store.clone(),
            research: research.id,
            member,
        };
        let mut registry = ToolRegistry::new();
        let TeamTools(tools) = team_tools(seat);
        for tool in tools {
            registry.register(tool);
        }
        if let Member::Scout(_) = member {
            if let Some(search) = &self.tools.search {
                registry.register(search.clone());
            }
            if let Some(key) = &self.tools.firecrawl_key {
                register_web_tools(&mut registry, key.expose_secret());
            }
        }
        for tool in extra {
            registry.register(tool);
        }
        let refusals = repeat::guard_tool_calls(&mut registry);

        let mut llm = self
            .llm
            .with_model(model.model.clone())
            .with_user(user.id())
            .with_reasoning(model.thinking)
            .with_session_id(research.id);
        if let Some(routing) = self.models.routing(&model.model) {
            llm.provider_preferences = Some(routing.clone());
        }
        let mut agent = Agent::builder(self.provider.clone())
            .llm(llm)
            .tools(registry)
            .allow_parallel_tool_calls(true)
            .build()
            .await;
        conversation.set_system(match member {
            Member::Scout(_) => SCOUT_PROMPT,
            Member::Critic => CRITIC_PROMPT,
            Member::Writer => WRITER_PROMPT,
            Member::Lead => COORDINATOR_PROMPT,
        });
        let input = AgentInput::User {
            content: task,
            context: Some(self.team_context(research, plan, member)),
            parts: None,
        };

        let cancel = cancel.child_token();
        let mut reply = String::new();
        // What each running call does, by call id, for its step.
        let mut running: HashMap<String, String> = HashMap::new();
        {
            let stream = agent.start(&mut conversation, cancel.clone(), input, None);
            tokio::pin!(stream);
            let deadline = sleep(MEMBER_RUN);
            tokio::pin!(deadline);
            loop {
                let event = tokio::select! {
                    _ = &mut deadline => {
                        cancel.cancel();
                        anyhow::bail!("{member} ran out of time");
                    }
                    _ = cancel.cancelled() => break,
                    event = timeout(IDLE_TIMEOUT, stream.next()) => match event {
                        Ok(event) => event,
                        Err(_) => {
                            cancel.cancel();
                            anyhow::bail!("{member} went quiet");
                        }
                    },
                };
                match event {
                    Some(AgentEvent::TextDelta(delta)) => reply.push_str(&delta),
                    Some(AgentEvent::ToolStart {
                        name,
                        id,
                        humanized,
                    }) if !TEAM_TOOLS.contains(&name.as_str()) => {
                        running.insert(
                            id,
                            if humanized.is_empty() {
                                name
                            } else {
                                humanized
                            },
                        );
                    }
                    Some(AgentEvent::ToolResult { id, success, .. }) => {
                        if let Some(label) = running.remove(&id) {
                            let step = Entry::Step { label, success };
                            self.store
                                .add_research_entry(research.id, member, &step)
                                .await?;
                        }
                        if refusals.exhausted() {
                            cancel.cancel();
                            anyhow::bail!("{member} kept repeating a call");
                        }
                    }
                    Some(AgentEvent::Done(_)) => break,
                    Some(AgentEvent::Error(error)) => anyhow::bail!("{member} failed: {error}"),
                    None => break,
                    _ => {}
                }
            }
        }
        if cancel.is_cancelled() {
            return Ok(conversation);
        }
        let said = reply.trim();
        if !said.is_empty() {
            let content: String = said.chars().take(MAX_SAID).collect();
            let entry = Entry::Said { to: None, content };
            self.store
                .add_research_entry(research.id, member, &entry)
                .await?;
        }
        Ok(conversation)
    }

    /// What every member is told about the research and the team.
    fn team_context(&self, research: &ResearchRow, plan: &Plan, member: Member) -> String {
        let mut ctx = String::new();
        let _ = writeln!(ctx, "Today is {}.", Utc::now().format("%A %-d %B %Y"));
        let _ = writeln!(ctx, "Research topic: {}", research.topic);
        let _ = writeln!(ctx, "You are {} ({member}).", member.name());
        let _ = writeln!(ctx, "The team:");
        if let Some(config) = self.models.research() {
            for seat in plan.team(&config.lead) {
                let _ = writeln!(ctx, "- {}: {}", seat.member, seat.role);
            }
        }
        ctx
    }

    /// Says how a team ended: the lead writes the result in the chat when
    /// it finished, or a line on why it did not.
    async fn team_ended(self: &Arc<Self>, user: &Arc<User>, id: Uuid, ended: Result<Status>) {
        if self.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        let status = match ended {
            Ok(status) => status,
            Err(error) => {
                log::warn!("research failed: {error:#}; research={id}");
                for from in [Status::Researching, Status::Reviewing, Status::Writing] {
                    match self.store.move_research(id, from, Status::Failed).await {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(error) => log::warn!("research: unable to mark failed: {error:#}"),
                    }
                }
                Status::Failed
            }
        };
        if status == Status::Stopped {
            return;
        }
        let research = match self.store.research(id).await {
            Ok(Some(research)) => research,
            Ok(None) => return,
            Err(error) => {
                log::warn!("research: unable to read: {error:#}; research={id}");
                return;
            }
        };
        let round = research.plan.as_ref().map_or(1, Plan::round);
        let message: String = match status {
            Status::Done if round > 1 => {
                let draft: String = research.draft.chars().take(MAX_DRAFT_READ).collect();
                format!(
                    "The team finished follow-up round {round} and revised the paper:\n\n\
                     {draft}\n\nAnswer the follow-up the user asked in a few sentences, \
                     leading with the answer, and say the paper in the research window is \
                     updated. If they asked for more while the team worked, call \
                     research_more for it."
                )
            }
            Status::Done => {
                let draft: String = research.draft.chars().take(MAX_DRAFT_READ).collect();
                format!(
                    "The team finished the research. Their paper:\n\n{draft}\n\nTell the user \
                     the conclusion in a few sentences, leading with the answer, and say the \
                     full paper is in the research window. If they asked for more while the \
                     team worked, call research_more for it."
                )
            }
            _ => "The research team failed partway. Tell the user in one or two sentences, \
                  and offer to start it again."
                .to_owned(),
        };
        let message = || Message {
            session_id: research.chat,
            request_id: Uuid::new_v4(),
            message: message.clone(),
            cron_id: None,
            agent: None,
            scheduled: Some(FINISHED_LABEL.to_owned()),
            always_sent: true,
            woken: None,
            images: Vec::new(),
            files: Vec::new(),
            answer: None,
            model: None,
            thinking: None,
            origin: DeviceKind::Phone {},
            platform: None,
            draws: Draws::default(),
            finished_research: true,
        };
        // The user may be talking to the lead; wait for that turn to end.
        for _ in 0..60 {
            match self.submit(user, message()).await {
                Ok(CreateRun::Conflict) => sleep(Duration::from_secs(5)).await,
                Ok(_) => return,
                Err(error) => {
                    log::warn!("research: unable to post the result: {error:#}; research={id}");
                    return;
                }
            }
        }
        log::warn!("research: the chat stayed busy; result not posted; research={id}");
    }
}
