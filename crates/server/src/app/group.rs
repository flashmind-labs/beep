//! Group chats: members post, and a message that mentions @beep, or that
//! Jev judges Beep can help with, starts a run that answers in the group.
//! The run reads every member's memories, and its reply passes the leak gate
//! before anyone sees it.

use super::{
    App, UserNotice, group_reminders,
    group_tools::{self, GroupRun},
    learning::MEMORY_THRESHOLD,
    run::register_web_tools,
};
use crate::{
    apps::markdown,
    decision::Criteria,
    images::Picture,
    leak::{Gate, Said, Secret},
    memories::{Reading, ReadingKind, profile_facts},
    permissions::{self, Approved, Rules},
    protocol::{Group, GroupAuthor, GroupMember, GroupMessage, Notice},
    skills::{self, GROUP_INDEX_HEADER},
    store::{JoinedGroup, MemberRow, MessageRow, NewMessage, PicturesOf, SkillOwner},
    tools::{
        group_memory::{self, Member, Secrets, shareable},
        skills::SkillTool,
    },
    user::User,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
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
    time::Duration,
};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// Messages the model sees before the one that mentions it.
const HISTORY: i64 = 40;
/// The longest a group reply may take.
const MAX_RUN: Duration = Duration::from_secs(5 * 60);
/// How long the run may go without an event.
const IDLE_TIMEOUT: Duration = Duration::from_secs(150);
/// What Beep posts when the run fails.
const FAILED: &str = "Sorry, I couldn't answer that just now. Try again in a bit.";
/// What Beep posts when an app call needs the sender's approval.
const NEEDS_APPROVAL: &str =
    "That needs your OK first. Ask me in our own chat and you can approve it there.";

/// How long Beep waits after a message without @beep, so a sender who
/// writes in bursts finishes first.
const SETTLE: Duration = Duration::from_secs(3);
/// Messages Jev reads to judge whether Beep should answer.
const CALL_CONTEXT: i64 = 15;
/// How sure Jev must be before Beep answers without being mentioned.
const CALL_THRESHOLD: f64 = 0.5;
const CALL_QUESTION: &str =
    "Nobody wrote @beep. Should Beep, the group's assistant, answer the latest message on its own?";
const CALL_CRITERIA: Criteria = Criteria {
    yes: "The latest message asks Beep, or asks the group something Beep can answer, find or do now: where to eat or stay, what to do on a day, opening hours, how to get somewhere, prices, weather, a fact, a plan, an itinerary or a list; or a recent question like that is still open because nobody knows the answer; or the group is planning something Beep could help with; or it asks Beep to remember, note or write down something, or asks for the group or everyone to be reminded of something (\"remind everyone\", \"recuérdanoslo\", \"avísanos a las 10\") (\"apúntalo\", \"remember this\"); or it replies to what Beep said, including a correction, a complaint that Beep was wrong, an objection or a new constraint on Beep's suggestion (an allergy, a diet, a date, a budget) that calls for a better answer.",
    no: "People talking to each other: greetings, jokes, news, feelings, a question only another member can answer (their plans, opinions or whereabouts), a reply to another member that does not need Beep, members deciding among themselves or discussing Beep's suggestion with each other, a rhetorical question or a complaint about the world, thanks, or something already answered.",
};
/// Messages the learner reads around a new one.
const LEARN_CONTEXT: i64 = 10;
const MEMORY_QUESTION: &str = "Does the latest message reveal something lasting about one of the members that Beep should remember about them?";
const MEMORY_CRITERIA: Criteria = Criteria {
    yes: "A lasting fact about a member, said by them or by someone else about them: diet, allergies and intolerances, \
health, preferences and dislikes, people in their life, where they live, work, hobbies, routines, or plans they take part in, \
like a trip; or a request for Beep to remember or note something said earlier in the chat (\"apúntalo\", \"remember this\")",
    no: "Greetings, jokes, logistics of the moment, questions, Beep's own replies, opinions about the world, or anything \
that says nothing lasting about a member",
};

/// Whether `text` calls Beep.
pub(crate) fn mentions_beep(text: &str) -> bool {
    // ASCII lowercasing keeps byte offsets, so `at` indexes `text` too.
    text.to_ascii_lowercase()
        .match_indices("@beep")
        .any(|(at, _)| {
            text[at + "@beep".len()..]
                .chars()
                .next()
                .is_none_or(|next| !next.is_alphanumeric())
        })
}

fn millis(time: DateTime<Utc>) -> i64 {
    time.timestamp_millis()
}

/// `row` as `user_id` sees it.
pub(crate) fn group_message(row: &MessageRow, user_id: &str) -> GroupMessage {
    GroupMessage {
        seq: row.id,
        author: match &row.author_id {
            Some(author) => GroupAuthor::Member {
                name: row.author_name.clone(),
                you: author == user_id,
            },
            None => GroupAuthor::Beep {},
        },
        content: row.content.clone(),
        timestamp: millis(row.sent_at),
    }
}

fn group_member(row: &MemberRow, user_id: &str) -> GroupMember {
    GroupMember {
        name: row.name.clone(),
        you: row.user_id == user_id,
        creator: row.creator,
        joined_at: millis(row.joined_at),
    }
}

/// Marks a group busy for as long as it lives, so a run that fails or
/// panics still lets Beep be asked again.
struct Typing {
    app: Arc<App>,
    group_id: Uuid,
}

impl Drop for Typing {
    fn drop(&mut self) {
        self.app.stop_typing(self.group_id);
    }
}

/// What Jev reads to judge the last of `history`: the group and its recent messages.
fn call_state(joined: &JoinedGroup, history: &[MessageRow]) -> String {
    let names: Vec<&str> = joined.members.iter().map(|m| m.name.as_str()).collect();
    let mut state = format!(
        "Group chat \"{}\" with {} and Beep, an assistant that searches the web and knows \
         the members.\n\nMessages, oldest first:",
        joined.group.name,
        names.join(", ")
    );
    for (i, row) in history.iter().enumerate() {
        let latest = if i + 1 == history.len() {
            " (latest)"
        } else {
            ""
        };
        let _ = write!(state, "\n{}{latest}: {}", row.author_name, row.content);
    }
    state
}

/// The chat as the learner reads it for `member`: their own messages marked,
/// the last one the new message.
fn learn_text(joined: &JoinedGroup, history: &[MessageRow], member: &MemberRow) -> String {
    let others: Vec<&str> = joined
        .members
        .iter()
        .filter(|m| m.user_id != member.user_id)
        .map(|m| m.name.as_str())
        .collect();
    let mut text = format!(
        "Group chat \"{}\". The user is {}; the other members are {}.\n\nMessages, oldest first:",
        joined.group.name,
        member.name,
        others.join(", ")
    );
    for (i, row) in history.iter().enumerate() {
        let author = match &row.author_id {
            Some(id) if *id == member.user_id => format!("{} (the user)", row.author_name),
            Some(_) => row.author_name.clone(),
            None => "Beep".to_owned(),
        };
        let new = if i + 1 == history.len() { " (new)" } else { "" };
        let _ = write!(text, "\n{author}{new}: {}", row.content);
    }
    text
}

pub(super) fn said(name: &str, text: &str) -> String {
    format!("{name}: {text}")
}

impl App {
    /// `joined` as `user_id` sees it.
    pub(crate) fn group_view(&self, joined: &JoinedGroup, user_id: &str) -> Group {
        Group {
            id: joined.group.id.to_string(),
            name: joined.group.name.clone(),
            members: joined
                .members
                .iter()
                .map(|member| group_member(member, user_id))
                .collect(),
            last: joined.last.as_ref().map(|row| group_message(row, user_id)),
            updated_at: millis(joined.group.updated_at),
            beep_typing: self.beep_typing(joined.group.id),
            pinned: joined
                .members
                .iter()
                .any(|member| member.user_id == user_id && member.pinned),
            apps_shared: joined
                .members
                .iter()
                .any(|member| member.user_id == user_id && member.shares_apps),
        }
    }

    /// Beep is writing in group `id`.
    pub(crate) fn beep_typing(&self, id: Uuid) -> bool {
        self.groups_busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&id)
    }

    /// Marks group `id` busy; `false` when Beep is already answering there.
    fn start_typing(&self, id: Uuid) -> bool {
        self.groups_busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id)
    }

    fn stop_typing(&self, id: Uuid) {
        self.groups_busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }

    /// Tells every member that group `id` changed.
    fn group_changed(&self, members: &[MemberRow], id: Uuid) {
        for member in members {
            let _ = self
                .notifications
                .send(UserNotice::group_changed(&member.user_id, &id.to_string()));
        }
    }

    /// Wakes every member but `except` with `body`, titled with the group.
    pub(super) fn group_notice(
        &self,
        members: &[MemberRow],
        joined: &JoinedGroup,
        except: Option<&str>,
        body: &str,
    ) {
        for member in members {
            if except == Some(member.user_id.as_str()) {
                let _ = self.notifications.send(UserNotice::group_changed(
                    &member.user_id,
                    &joined.group.id.to_string(),
                ));
                continue;
            }
            let _ = self.notifications.send(UserNotice {
                user: member.user_id.clone(),
                notice: Notice {
                    title: joined.group.name.clone(),
                    body: body.to_owned(),
                    session_id: joined.group.id.to_string(),
                    app: None,
                    approval: None,
                    changed: false,
                    group: true,
                },
            });
        }
    }

    /// Posts `user`'s message, with its pictures, to a group and, when it
    /// mentions @beep, starts Beep's reply. `None` when the user is not in
    /// the group.
    pub async fn send_to_group(
        self: &Arc<Self>,
        user: &Arc<User>,
        group_id: Uuid,
        request_id: Uuid,
        text: &str,
        pictures: &[Picture],
    ) -> Result<Option<GroupMessage>> {
        let posted = user
            .post_to_group(NewMessage {
                group_id,
                request_id,
                content: text,
            })
            .await?;
        let Some(posted) = posted else {
            return Ok(None);
        };
        let message = group_message(&posted.message, user.id());
        if !posted.new {
            return Ok(Some(message));
        }
        // Kept before anyone hears of the message, so they find its pictures.
        if !pictures.is_empty() {
            let of = PicturesOf {
                chat: group_id,
                message: posted.message.id,
            };
            if let Err(error) = user.save_pictures(of, pictures).await {
                log::warn!(
                    "unable to keep a group message's pictures; group={group_id}: {error:#}"
                );
            }
        }
        let Some(joined) = user.group(group_id).await? else {
            return Ok(Some(message));
        };
        self.group_notice(
            &joined.members,
            &joined,
            Some(user.id()),
            &said(&posted.message.author_name, text),
        );
        if self.decision().is_some() {
            let app = self.clone();
            let joined = joined.clone();
            let seq = posted.message.id;
            tokio::spawn(async move { app.learn_from_group(&joined, seq).await });
        }
        let mentioned = mentions_beep(text);
        if !mentioned && self.decision().is_none() {
            return Ok(Some(message));
        }
        let app = self.clone();
        let actor = user.clone();
        let mention = posted.message.id;
        tokio::spawn(async move {
            let group_id = joined.group.id;
            if !mentioned && !app.called_without_mention(&joined, mention).await {
                return;
            }
            if !app.start_typing(group_id) {
                return;
            }
            app.group_changed(&joined.members, group_id);
            let typing = Typing {
                app: app.clone(),
                group_id,
            };
            let reply = match app.answer_group(&actor, &joined, mention).await {
                Ok(reply) => reply,
                Err(error) => {
                    log::warn!("group run failed; group={group_id}: {error:#}");
                    FAILED.to_owned()
                }
            };
            let posted = app.store.post_beep(group_id, &reply).await;
            drop(typing);
            // Someone may have left or joined during the run.
            let members = match app.store.group_members(group_id).await {
                Ok(members) => members,
                Err(error) => {
                    log::warn!("unable to load members; group={group_id}: {error:#}");
                    return;
                }
            };
            match posted {
                Ok(_) => {
                    app.group_notice(&members, &joined, None, &said("Beep", &reply));
                    app.count_for_group_skills(&actor, &joined);
                }
                Err(error) => {
                    log::warn!("unable to post Beep's reply; group={group_id}: {error:#}");
                    app.group_changed(&members, group_id);
                }
            }
        });
        Ok(Some(message))
    }

    /// Posts a reminder `user` set into group `group_id` and wakes every
    /// member. Skipped once `user` has left the group.
    pub(crate) async fn post_group_reminder(
        &self,
        user: &User,
        group_id: Uuid,
        message: &str,
    ) -> Result<()> {
        let Some(joined) = user.group(group_id).await? else {
            log::info!("group reminder skipped, its maker left; group={group_id}");
            return Ok(());
        };
        self.store.post_beep(group_id, message).await?;
        self.group_notice(&joined.members, &joined, None, &said("Beep", message));
        Ok(())
    }

    /// Whether Beep should answer message `seq`, which does not mention it.
    /// Waits for the sender to settle first; a newer message gets its own look.
    async fn called_without_mention(&self, joined: &JoinedGroup, seq: i64) -> bool {
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

    /// Learns what message `seq` says about each member, into that member's
    /// own memory. Jev screens it first, since most group chat teaches nothing.
    async fn learn_from_group(&self, joined: &JoinedGroup, seq: i64) {
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

    /// Runs Beep for `actor` on the group's messages up to `mention`, the
    /// one that called it, and returns the screened reply.
    async fn answer_group(
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
                for tool in permissions::guard(tools, rules, Approved(None), &composio.hinted) {
                    registry.register(tool);
                }
            }
        }

        crate::tool_cost::log("group", &registry);
        let model = self.model_for(actor).await;
        let mut llm = self
            .llm
            .with_model(model.clone())
            .with_user(actor.id())
            .with_reasoning(self.thinking_for(actor).await)
            .with_session_id(group_id);
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
        conversation.set_system(include_str!("../../prompts/group.md"));
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

#[cfg(test)]
mod eval;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::GroupRow;

    #[test]
    fn jev_reads_the_group_and_marks_the_latest_message() {
        let at = DateTime::<Utc>::UNIX_EPOCH;
        let member = |name: &str| MemberRow {
            user_id: name.to_lowercase(),
            name: name.into(),
            creator: false,
            joined_at: at,
            pinned: false,
            shares_apps: true,
        };
        let row = |id, author: &str, content: &str| MessageRow {
            id,
            author_id: Some(author.to_lowercase()),
            author_name: author.into(),
            content: content.into(),
            sent_at: at,
        };
        let joined = JoinedGroup {
            group: GroupRow {
                id: Uuid::nil(),
                name: "Tokyo trip".into(),
                updated_at: at,
            },
            members: vec![member("Dani"), member("Sam")],
            last: None,
        };
        let state = call_state(
            &joined,
            &[
                row(1, "Sam", "Landed!"),
                row(2, "Dani", "Where should we eat?"),
            ],
        );
        assert!(state.starts_with("Group chat \"Tokyo trip\" with Dani, Sam and Beep"));
        assert!(state.ends_with("\nSam: Landed!\nDani (latest): Where should we eat?"));

        let beep = MessageRow {
            author_id: None,
            ..row(2, "Beep", "Try ramen.")
        };
        let text = learn_text(
            &joined,
            &[beep, row(3, "Dani", "Not ramen, Sam can't have soy")],
            &joined.members[1],
        );
        assert!(text.starts_with(
            "Group chat \"Tokyo trip\". The user is Sam; the other members are Dani."
        ));
        assert!(text.ends_with("\nBeep: Try ramen.\nDani (new): Not ramen, Sam can't have soy"));
    }

    #[test]
    fn only_a_whole_mention_calls_beep() {
        assert!(mentions_beep("@beep where should we eat?"));
        assert!(mentions_beep("what do you think @Beep?"));
        assert!(mentions_beep("hey @BEEP"));
        assert!(!mentions_beep("beep me later"));
        assert!(!mentions_beep("@beeper is a phone"));
    }
}
