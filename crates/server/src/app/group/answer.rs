//! When Beep answers in a group, how it answers and what it learns there.

use super::{
    super::{
        App, group_reminders,
        group_tools::{self, GroupRun},
        learning::MEMORY_THRESHOLD,
        run::register_web_tools,
    },
    CALL_CONTEXT, CALL_CRITERIA, CALL_QUESTION, CALL_THRESHOLD, FAILED, HISTORY, IDLE_TIMEOUT,
    LEARN_CONTEXT, MAX_RUN, MEMORY_CRITERIA, MEMORY_QUESTION, NEEDS_APPROVAL, SETTLE, call_state,
    learn_text, said,
};
use crate::{
    account::user::User,
    apps::markdown,
    apps::permissions::{self, Approved, Rules},
    memory::leak::{Gate, Said, Secret},
    memory::memories::{Reading, ReadingKind, profile_facts},
    memory::skills::{self, GROUP_INDEX_HEADER},
    store::{JoinedGroup, SkillOwner},
    tools::{
        group_memory::{self, Member, Secrets, shareable},
        skills::SkillTool,
    },
};
use anyhow::Result;
use flashmind::{
    core::{Agent, CancellationToken, Conversation, ConversationEntry},
    tools::time::TimeTool,
    types::{AgentEvent, AgentInput, ToolRegistry},
};
use futures::StreamExt;
use secrecy::ExposeSecret;
use std::{
    fmt::Write,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::time::{sleep, timeout};

impl App {
    /// Whether Beep should answer unmentioned message `seq`, once the sender settles.
    pub(super) async fn called_without_mention(&self, joined: &JoinedGroup, seq: i64) -> bool {
        let Some(decision) = self.decision() else {
            return false;
        };
        sleep(SETTLE).await;
        let group_id = joined.group.id;
        let history = match self.store.group_messages(group_id, CALL_CONTEXT).await {
            Ok(history) => history,
            Err(error) => {
                log::warn!("unable to load group messages; group={group_id}: {error:#}");
                return false;
            }
        };
        if history.last().is_none_or(|last| last.id != seq) {
            return false;
        }
        let state = call_state(joined, &history);
        match decision
            .probability_with(&state, CALL_QUESTION, &CALL_CRITERIA)
            .await
        {
            Ok(probability) => {
                log::info!("group call scored {probability:.2}; group={group_id} seq={seq}");
                probability >= CALL_THRESHOLD
            }
            Err(error) => {
                log::warn!("Jev could not judge a group message; group={group_id}: {error:#}");
                false
            }
        }
    }

    /// Learns what message `seq` says about each member into their memory, after Jev screens it.
    pub(super) async fn learn_from_group(&self, joined: &JoinedGroup, seq: i64) {
        let Some(decision) = self.decision() else {
            return;
        };
        let group_id = joined.group.id;
        let history = match self
            .store
            .group_messages_until(group_id, seq, LEARN_CONTEXT)
            .await
        {
            Ok(history) => history,
            Err(error) => {
                log::warn!("unable to load group messages; group={group_id}: {error:#}");
                return;
            }
        };
        let Some(latest) = history.last().filter(|last| last.id == seq) else {
            return;
        };
        let state = call_state(joined, &history);
        let probability = match decision
            .probability_with(&state, MEMORY_QUESTION, &MEMORY_CRITERIA)
            .await
        {
            Ok(probability) => probability,
            Err(error) => {
                log::warn!("Jev could not score a group message; group={group_id}: {error:#}");
                return;
            }
        };
        log::info!("group memory scored {probability:.2}; group={group_id} seq={seq}");
        if probability < MEMORY_THRESHOLD {
            return;
        }
        for row in &joined.members {
            let Some(user) = self.user(&row.user_id) else {
                continue;
            };
            let reading = Reading {
                kind: ReadingKind::Group,
                id: format!("group:{group_id}:{seq}"),
                query: latest.content.clone(),
                text: learn_text(joined, &history, row),
            };
            let learned = user
                .learn_reading(self.generator(&row.user_id), &reading)
                .await;
            if !learned.is_empty() {
                log::info!(
                    "group memory learned {}; group={group_id} seq={seq} user={user}",
                    learned.len()
                );
            }
        }
    }

    /// Runs Beep for `actor` on the messages up to `mention` and returns the screened reply.
    pub(super) async fn answer_group(
        self: &Arc<Self>,
        actor: &Arc<User>,
        joined: &JoinedGroup,
        mention: i64,
    ) -> Result<String> {
        let group_id = joined.group.id;
        let history = self
            .store
            .group_messages_until(group_id, mention, HISTORY + 1)
            .await?;
        let Some((latest, earlier)) = history.split_last() else {
            return Ok(FAILED.to_owned());
        };

        // Each member as the run knows them. Their memories are private, so
        // they only count when Jev is there to check the reply.
        let members: Arc<[Member]> = joined
            .members
            .iter()
            .filter_map(|row| {
                Some(Member {
                    name: row.name.clone(),
                    user: self.user(&row.user_id)?,
                })
            })
            .collect();
        let decision = self.decision();
        let secrets: Secrets = Arc::new(Mutex::new(Vec::new()));
        let today = actor.today().await;
        let mut context = format!("Today is {today}. The group is \"{}\".", joined.group.name);
        let _ = write!(
            context,
            " Members: {}.",
            joined
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        match self.store.owned_skills(&SkillOwner::Group(group_id)).await {
            Ok(owned) => {
                if let Some(index) = skills::index(GROUP_INDEX_HEADER, &owned) {
                    let _ = write!(context, "\n\n{index}");
                }
            }
            Err(error) => log::warn!("unable to list group skills: {error:#}; group={group_id}"),
        }
        if decision.is_some() {
            let mut private = String::new();
            for member in members.iter() {
                let rows = member.user.memory_rows().await.unwrap_or_default();
                let rows: Vec<_> = rows
                    .into_iter()
                    .filter(|row| shareable(row.source))
                    .collect();
                // `profile_facts` keeps to memories about the member.
                let facts = profile_facts(&rows, today);
                if facts.is_empty() {
                    continue;
                }
                let _ = write!(private, "\n\nAbout {} (private):", member.name);
                let mut secrets = secrets.lock().unwrap_or_else(PoisonError::into_inner);
                for fact in facts.into_iter().take(15) {
                    let _ = write!(private, "\n- {fact}");
                    secrets.push(Secret {
                        owner: member.name.clone(),
                        content: fact,
                    });
                }
            }
            context.push_str(&private);
        }

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TimeTool));
        registry.register(Arc::new(SkillTool {
            store: self.store.clone(),
            owner: SkillOwner::Group(group_id),
            writes: false,
        }));
        if let Some(search) = &self.tools.search {
            registry.register(search.clone());
        }
        let chat: Vec<Said> = history
            .iter()
            .map(|row| Said {
                author: match row.author_id {
                    Some(_) => row.author_name.clone(),
                    None => "Beep".to_owned(),
                },
                text: row.content.clone(),
            })
            .collect();
        match &self.tools.decision {
            Some(tool) => {
                // The run holds members' memories, and the gate only reads the
                // reply, so the web and app tools screen what they send out.
                registry.register(tool.clone());
                let me = joined
                    .members
                    .iter()
                    .find(|row| row.user_id == actor.id())
                    .map(|row| row.name.clone())
                    .unwrap_or_default();
                let actor_member = Member {
                    name: me,
                    user: actor.clone(),
                };
                // Members who keep their apps to themselves still lend them
                // when they are the one asking.
                let lenders: Vec<Member> = joined
                    .members
                    .iter()
                    .filter(|row| row.shares_apps || row.user_id == actor.id())
                    .filter_map(|row| members.iter().find(|m| m.user.id() == row.user_id))
                    .cloned()
                    .collect();
                let run = Arc::new(GroupRun {
                    app: self.clone(),
                    group_id,
                    group_name: joined.group.name.clone(),
                    actor: actor_member.clone(),
                    secrets: secrets.clone(),
                    chat: chat.clone(),
                    decision: tool.clone(),
                });
                let key = self.tools.firecrawl_key.as_ref().map(|k| k.expose_secret());
                group_reminders::register(&mut registry, run.clone());
                group_tools::register(&mut registry, run, lenders, key).await;
                group_memory::register(
                    &mut registry,
                    members.clone(),
                    actor_member,
                    secrets.clone(),
                );
            }
            None => {
                if let Some(key) = &self.tools.firecrawl_key {
                    register_web_tools(&mut registry, key.expose_secret());
                }
                let (composio, mcp) = tokio::join!(actor.app_tools(), actor.mcp_tools());
                let rules = actor.app_rules().await.unwrap_or_else(|error| {
                    log::warn!("permissions: unable to load, asking for everything: {error:#}");
                    Rules::default()
                });
                let tools = markdown::wrap(composio.tools.into_iter().chain(mcp).collect());
                for tool in permissions::guard(tools, rules, Approved(None), &composio.hinted, None)
                {
                    registry.register(tool);
                }
            }
        }

        crate::agent::tool_cost::log("group", &registry);
        let model = self.model_for(actor).await;
        let mut llm = self
            .llm
            .with_model(model.clone())
            .with_user(actor.id())
            .with_reasoning(self.thinking_for(actor).await);
        if let Some(routing) = self.models.routing(&model) {
            llm.provider_preferences = Some(routing.clone());
        }
        let mut agent = Agent::builder(self.provider.clone())
            .llm(llm)
            .tools(registry)
            .allow_parallel_tool_calls(true)
            .build()
            .await;

        let mut conversation = Conversation::new();
        conversation.set_system(include_str!("../../../prompts/group.md"));
        for row in earlier {
            conversation.add(match row.author_id {
                Some(_) => ConversationEntry::user(said(&row.author_name, &row.content)),
                None => ConversationEntry::assistant(&row.content),
            });
        }
        let input = AgentInput::User {
            content: said(&latest.author_name, &latest.content),
            context: Some(context),
            parts: None,
        };

        let cancel = CancellationToken::new();
        let mut reply = String::new();
        let mut needs_approval = false;
        {
            let stream = agent.start(&mut conversation, cancel.clone(), input, None);
            tokio::pin!(stream);
            let deadline = sleep(MAX_RUN);
            tokio::pin!(deadline);
            loop {
                let event = tokio::select! {
                    _ = &mut deadline => { cancel.cancel(); return Ok(FAILED.to_owned()); }
                    event = timeout(IDLE_TIMEOUT, stream.next()) => match event {
                        Ok(event) => event,
                        Err(_) => { cancel.cancel(); return Ok(FAILED.to_owned()); }
                    },
                };
                match event {
                    Some(AgentEvent::TextDelta(delta)) => reply.push_str(&delta),
                    Some(AgentEvent::Interrupted { .. }) => {
                        needs_approval = true;
                    }
                    Some(AgentEvent::Done(_)) => break,
                    Some(AgentEvent::Error(_)) | None => return Ok(FAILED.to_owned()),
                    _ => {}
                }
            }
        }
        let reply = reply.trim();
        if needs_approval && reply.is_empty() {
            return Ok(NEEDS_APPROVAL.to_owned());
        }
        if reply.is_empty() {
            return Ok(FAILED.to_owned());
        }

        let secrets = secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(decision) = decision else {
            // Without Jev no member memory entered the run.
            return Ok(reply.to_owned());
        };
        let gate = Gate {
            decision,
            secrets: &secrets,
            chat: &chat,
        };
        Ok(gate
            .screen(reply.to_owned(), self.generator(actor.id()))
            .await)
    }
}
