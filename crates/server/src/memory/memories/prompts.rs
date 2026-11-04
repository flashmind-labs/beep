//! The prompts that learn memories from a chat or a reading and curate them.

use super::{Exchange, Hit, MAX_CONTEXT_CHARS, MAX_TURN_CHARS, clip, days, write_actions};
use crate::{
    protocol::MemorySource,
    store::{MemoryRow, Scope},
};
use chrono::{NaiveDate, Utc};
use std::fmt::Write;

pub(crate) const LEARN_SYSTEM: &str = "You keep the long-term memory of Beep, a personal assistant app. \
Read the latest exchange and decide what it teaches about the user that is worth remembering. \
Earlier turns are only there to explain the latest one; do not learn from them again.
First triage it: casual chatter, a one-off task, or something personal. Small talk, thanks and questions about \
the world teach nothing; then reply {\"ops\": []}. A task often does teach something in passing: a request for \
gluten-free restaurants says the user wants gluten-free food, pulling their Amex statements says they have an \
American Express card, a reminder to write docs for a project says they work on that project. Keep that fact, \
not the task.
Worth keeping: the user's name, the people and pets in their life and how they relate (a person the user asks \
about by name is someone in their life), where they live and work, \
their job and ongoing projects, stable likes, dislikes and dietary needs, routines, important dates, goals, \
plans such as trips, visits and appointments, and how they want Beep to answer. Only what the user said or clearly \
confirmed, or what Beep found about the user in their own data (calendar, mail, notes) at their request, such as \
upcoming trips, events and appointments. Never guesses, general knowledge, Beep's own suggestions, the task itself \
(what to search, which reminder to set), or secrets such as passwords, codes, tokens, card or account numbers.
Known memories are listed with their ids. If a fact is already known, do nothing. If the exchange corrects or \
extends one, update it by id with the full new text. If the user says something is no longer true or asks \
Beep to forget it, forget it by id. Otherwise add it.
\"Beep did\" lists what Beep did in the user's apps during the latest exchange, such as sending an email or \
creating an event. When that finishes a task, plan or follow-up a known memory still describes as ahead, forget \
that memory by id, or update it to the lasting fact if one remains. An action alone is not a new fact to add.
A fact that only holds for a while (a trip, a visit, a stay, an appointment, a temporary job) gets valid_from \
and/or valid_until as YYYY-MM-DD, worked out from today's date. Lasting facts leave both out.
Write each memory as one short self-contained sentence about one independently useful fact, in third person \
that names its subject. Store distinct facts separately, even when they have the same subject, \
like \"Has a dog named Rex.\" or \"Is in Lisbon at the Harbour Hotel.\", in the language of the user's \
latest message. Never translate a known memory.
Reply with only JSON: {\"ops\": [{\"op\": \"add\" | \"update\" | \"forget\", \"id\": string, \"content\": string, \
\"valid_from\": string, \"valid_until\": string, \"quote\": string}]}. \
id only for update and forget; content only for add and update. quote, for add and update, is the \
few words of the text that show the fact, copied exactly, at most 20 words. At most 5 ops.";

/// The memories shown to a learner, with ids it can update or forget.
pub(super) fn write_known(text: &mut String, known: &[Hit]) {
    let _ = writeln!(text, "\nKnown memories:");
    if known.is_empty() {
        let _ = writeln!(text, "(none related)");
    }
    for h in known {
        let _ = writeln!(
            text,
            "- {} [{}, {}, {}]{}: {}",
            h.id,
            h.category,
            h.source.as_str(),
            h.date,
            days(&h.valid),
            h.content
        );
    }
}

pub(crate) fn learn_prompt(known: &[Hit], exchange: &Exchange) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}", exchange.today.format("%A %Y-%m-%d"));
    write_known(&mut text, known);
    if !exchange.earlier.is_empty() {
        let _ = writeln!(text, "\nEarlier in the chat:");
        for turn in &exchange.earlier {
            let _ = writeln!(text, "User: {}", clip(&turn.message, MAX_CONTEXT_CHARS));
            let _ = writeln!(text, "Beep: {}", clip(&turn.reply, MAX_CONTEXT_CHARS));
        }
    }
    let latest = &exchange.latest;
    let _ = writeln!(
        text,
        "\nLatest exchange:\nUser: {}",
        clip(&latest.message, MAX_TURN_CHARS)
    );
    let _ = write!(text, "Beep: {}", clip(&latest.reply, MAX_TURN_CHARS));
    let _ = write_actions(&mut text, &latest.actions);
    text.push('\n');
    text
}

/// What the learner reads besides chats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingKind {
    Email,
    Slack,
    /// A stretch of a Beep group chat, read for one member.
    Group,
}

impl ReadingKind {
    pub(crate) fn source(self) -> MemorySource {
        match self {
            ReadingKind::Email => MemorySource::Email {},
            ReadingKind::Slack => MemorySource::Slack {},
            // Members said it in Beep itself, so like a chat it may be used in groups.
            ReadingKind::Group => MemorySource::Learned {},
        }
    }

    pub(crate) fn intro(self) -> &'static str {
        match self {
            ReadingKind::Email => EMAIL_INTRO,
            ReadingKind::Slack => SLACK_INTRO,
            ReadingKind::Group => GROUP_INTRO,
        }
    }
}

/// An email or a Slack conversation for the learner.
pub struct Reading {
    pub kind: ReadingKind,
    /// Tells attempts apart in the log; never content.
    pub id: String,
    /// Searched to bring up the memories it may touch.
    pub query: String,
    /// What the learner reads, headed by what it is.
    pub text: String,
}

pub(super) const EMAIL_INTRO: &str = "You keep the long-term memory of Beep, a personal assistant app. \
You get one email from the user's own mailbox: either sent by the user, or received by them. Decide what it \
teaches about the user that is worth remembering. Most emails teach nothing lasting, such as newsletters, \
marketing, notifications and routine receipts; then reply {\"ops\": []}. A quick screen let this email \
through because it might be about the user, but the screen is loose and you are the final judge. \
The older the email, the higher the bar: from an email more than a few weeks old keep only stable facts, like \
where the user lives, who they know, what they do and what they like, and nothing about what they were doing then.";

pub(super) const SLACK_INTRO: &str = "You keep the long-term memory of Beep, a personal assistant app. \
You get a stretch of one Slack conversation around messages the user wrote, messages that mention them, or \
direct messages to them, with the user's own messages marked. Decide what it teaches about the user that is worth \
remembering. Most chat teaches nothing lasting, such as status updates, quick questions, links, jokes and small \
talk; then reply {\"ops\": []}. Facts about the user's work count, like their role, team and the projects they own, \
but not the day-to-day of tasks and tickets. One exception: a request made to the user with a date or deadline \
still ahead, like someone asking them to send something by Friday, is kept as a short fact naming who asked, with \
valid_until on that day. Drop requests without a date.";

pub(super) const GROUP_INTRO: &str = "You keep the long-term memory of Beep, a personal assistant app, for one member \
of a Beep group chat, called the user here. You get the latest stretch of the chat with the user's own messages \
marked. Only the last message is new; the ones before it are context and were read already. Decide what the last \
message teaches about the user that is worth remembering, whether the user says it or another member says it about \
them, like a friend saying the user cannot eat soy. Plans the user takes part in count, like a trip the group is \
taking. When the last message asks Beep to remember, like \"remember that\" or \"recuérdanoslo\", the earlier messages \
it points to are what to learn from. Most group chat teaches nothing lasting about the user, such as jokes, greetings, Beep's suggestions, or \
facts about other members; then reply {\"ops\": []}.";

/// How the learner judges and answers, whatever it reads.
pub(crate) const LEARN_RULES: &str = "Worth keeping: the user's name, the people in their life and how they relate, where \
they live and work, homes and property they own, their job and ongoing projects, stable likes, dislikes and dietary \
needs, health conditions and insurance, beliefs, religion and politics, money: the banks, investments and debts \
they have, residency and visas, routines, important dates, goals, and plans such as trips, visits and \
appointments that are still ahead. Only what the text clearly shows about the user, not about anyone else, and \
only what is still likely true today. \
Never keep one-off errands and admin (utilities, meters, deliveries, repairs, support requests), single \
payments and statements, events and episodes that only mattered at the time, plans that are already over by \
today, guesses (nothing \"likely\" or \"probably\"), general knowledge, or secrets and numbers such as \
passwords, codes, tokens, card, account, contract or ID numbers.
Known memories are listed with their ids. If a fact is already known, do nothing. If the text shows a known \
fact changed or extends it, update it by id with the full new text. Forget a memory by id only when the text \
shows it is no longer true; never because the text does not mention it. Otherwise add it.
A fact that only holds for a while (a trip, a visit, a stay, an appointment, a temporary job) gets valid_from \
and/or valid_until as YYYY-MM-DD, worked out from the date the text was written. Lasting facts leave both out.
Write each memory as one short self-contained sentence about the user in third person that names its subject, \
like \"Has a dog named Rex.\" or \"Works at Acme as a designer.\", in the language most known memories are in, \
English when there are none. Never translate a known memory.
Reply with only JSON: {\"ops\": [{\"op\": \"add\" | \"update\" | \"forget\", \"id\": string, \"content\": string, \
\"valid_from\": string, \"valid_until\": string, \"quote\": string}]}. \
id only for update and forget; content only for add and update. quote, for add and update, is the \
few words of the text that show the fact, copied exactly, at most 20 words. At most 5 ops.";

pub(crate) fn reading_prompt(known: &[Hit], reading: &Reading) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}", Utc::now().format("%A %Y-%m-%d"));
    write_known(&mut text, known);
    let _ = writeln!(text, "\n{}", reading.text);
    text
}

/// What curation is told the memories are about.
pub(super) fn curated_subject(scope: &Scope) -> &'static str {
    match scope {
        Scope::User => "one user",
        Scope::Machine(_) => "one of the user's machines",
        Scope::Project(_) => "one of the user's projects",
        Scope::Account(_) => "one of the user's accounts",
    }
}

pub(super) const CURATE_RULES: &str = "Source \"user\" means the user said it or asked Beep to remember it; \"learned\" \
means Beep picked it up from a chat; \"email\" means Beep read it in the user's email; \"slack\" means Beep \
read it in the user's Slack.
Clean it up:
- Keep one independently useful fact per memory, with enough context to identify its subject. Split a memory \
that contains distinct facts into separate self-contained sentences. Keep dates and qualifications; only split \
when all resulting facts have the same days as the original. Do not invent facts.
- Merge only memories that say the same fact with the same meaning and days. A shared subject or topic is not \
enough. Keep each distinct detail in its own memory.
- When two memories clearly contradict each other, keep the newer one and supersede the older with it, unless the older \
was said by the user and the newer was only learned; then keep both. \
A superseded memory is kept as the newer one's history; never forget the losing side of a contradiction. \
Differences in time or context may explain both statements; when unsure, keep both.
- Forget memories that are trivial, a one-off task, general knowledge rather than about what these memories \
are about, or a secret such as a password or code. Never forget a memory said by the user unless its days are over.
- A memory marked over has passed. When it still says something lasting, like a place the user went, \
someone they met or a job they did, update it into a lasting fact in the past tense that names the month and year, \
like \"Went to Lisbon in October 2026.\", with no days. Forget it when nothing about it lasts, like an \
appointment or a deadline.
- Fix a memory that is unclear without context so it names its subject.
Leave everything else alone; most memories need nothing. Each id may appear in at most one op.
";

pub(super) const CURATE_PROFILE: &str = "Then pick the profile: the ids of at most 15 memories that matter in nearly every chat, such as the user's \
name, how they want Beep to answer, their closest people, where they live and work, and their job, mostly from \
identity, people, preference and work. Not projects, \
passing interests or anything with days. Memories marked core are in it now; keep it stable and change it only \
for a better pick. A merged memory is named by any of its ids.
";

pub(super) const CURATE_REPLY: &str = "Write memories as one short self-contained sentence in third person. Never translate: keep a memory in the \
language it is already in, and write a merge in the language most of its memories are in, English when unsure.
Reply with only JSON: {\"ops\": [{\"op\": \"merge\", \"ids\": [string], \"content\": string, \
\"valid_from\": string, \"valid_until\": string} | {\"op\": \"update\", \"id\": string, \"content\": string, \
\"valid_from\": string, \"valid_until\": string} | {\"op\": \"split\", \"id\": string, \"contents\": [string]} | \
{\"op\": \"supersede\", \"id\": string, \"by\": string} | \
{\"op\": \"forget\", \"id\": string}]}. \
Leave out days for lasting facts. \
At most 40 ops, [] when nothing needs doing.";

/// The curation instructions for `scope`; only the user's own scope picks the profile.
pub(crate) fn curate_system(scope: &Scope) -> String {
    let mut text = format!(
        "You tidy the long-term memory of Beep, a personal assistant app. \
You get every memory about {} with its id, category, source, last change date and, for facts that only \
hold for a while, their days. {CURATE_RULES}",
        curated_subject(scope)
    );
    if *scope == Scope::User {
        text.push_str(CURATE_PROFILE);
    }
    text.push_str(CURATE_REPLY);
    if *scope == Scope::User {
        text.push_str(" Add \"core\": [string], the profile's ids, next to \"ops\".");
    }
    text
}

pub(crate) fn curate_prompt(rows: &[MemoryRow], today: NaiveDate) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Today: {}\n\nMemories:", today.format("%Y-%m-%d"));
    let mut rows: Vec<&MemoryRow> = rows.iter().collect();
    rows.sort_by(|a, b| a.category.cmp(&b.category));
    for row in rows {
        let ended = if row.valid.ended(today) { ", over" } else { "" };
        let core = if row.core { ", core" } else { "" };
        let _ = writeln!(
            text,
            "- {} [{}, {}, {}]{}{ended}{core}: {}",
            row.id,
            row.category,
            row.source.as_str(),
            row.updated(),
            days(&row.valid),
            row.content
        );
    }
    text
}
