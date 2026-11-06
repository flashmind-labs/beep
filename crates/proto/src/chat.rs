//! Chats with the assistant: runs and their events, history, approvals and notices.

mod cards;
mod group_chats;
mod media;
mod research;
pub mod search;

pub use cards::*;
pub use group_chats::*;
pub use media::*;
pub use research::*;

use crate::AutoPick;
use std::time::SystemTime;
use uuid::Uuid;

ffi_structs! {
    /// Something from one of the user's apps worth knowing, as `ListUpdates`
    /// lists it.
    pub struct AppUpdate {
        pub id: i64,
        /// One line, such as "Your TestFlight build is ready."
        pub summary: String,
        /// The app's slug, as `AppIntegration::slug` gives it, such as `gmail`.
        /// Empty when the events it came from are gone.
        pub app: String,
        /// How the user knows the app, such as "Gmail"; empty with `app`.
        pub app_name: String,
        /// When the triage agent decided to tell.
        pub at: SystemTime,
    }

    /// What the assistant told the user on its own, as `ListAlerts` lists it.
    pub struct Alert {
        /// Its session. Sending a message there makes it a chat.
        pub session_id: Uuid,
        /// A few words, such as "Arrived in Girona".
        pub title: String,
        /// Where it came from, such as "Places".
        pub source: String,
        /// What Beep said, in plain text.
        pub text: String,
        pub read: bool,
        pub at: SystemTime,
    }

    /// An approval waiting in one chat, as `ListPendingApprovals` lists it.
    pub struct ChatApproval {
        pub session_id: Uuid,
        /// The chat's title; empty when the chat is gone.
        pub session_title: String,
        pub pending: PendingApproval,
    }

    /// A problem the improvement watcher found in users' chats, as
    /// `ListFindings` lists it. The same problem in other chats counts here
    /// once more rather than coming again.
    pub struct Finding {
        pub id: String,
        /// What went wrong: `correction`, `failed_tool`, `wrong_tool`,
        /// `could_have_found`, `loop`, `slow`, `gave_up` or `other`. Show one
        /// this app does not know as other.
        pub kind: String,
        pub summary: String,
        /// How to make it happen again.
        pub steps: String,
        /// Where it likely comes from: `prompt`, `tool_schema`, `missing_tool`,
        /// `model` or `other`.
        pub cause: String,
        /// Why the watcher thinks so.
        pub cause_detail: String,
        /// What it proposes to change.
        pub fix: String,
        pub status: FindingStatus,
        /// How many times it was seen.
        pub seen: u32,
        pub first_seen: SystemTime,
        pub last_seen: SystemTime,
        /// The caller's chat it was last seen in, `None` when it was only seen
        /// in other users' chats.
        pub session_id: Option<Uuid>,
        /// The ids of the first and last messages of that chat that show it
        /// (`ChatMessage::id`); both 0 without a chat.
        pub first_message: i64,
        pub last_message: i64,
    }
}

ffi_enum! {
    /// Where the user left a finding.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FindingStatus {
        0 Open {}
        1 Resolved {}
        2 Dismissed {}
    }
}

ffi_structs! {
    /// What a tool step showed while it ran, as `GetToolLabels` lists it.
    pub struct ToolLabel {
        pub call_id: String,
        /// The `RunEvent::ToolStarted` `description` of the call.
        pub description: String,
    }

    /// A sub-agent or background job of a chat, as `ListWork` lists it.
    pub struct WorkItem {
        /// As the agent writes it, such as "w12". `CancelWork` takes it.
        pub id: String,
        pub kind: WorkKind,
        /// What it was asked to do, cut short.
        pub task: String,
        pub started_at: SystemTime,
        /// What it did last, such as the tool it called. Empty before its
        /// first step.
        pub step: String,
        /// How it ended; `None` while it runs.
        pub outcome: Option<WorkOutcome>,
    }

    /// How a sub-agent or job ended.
    pub struct WorkOutcome {
        pub ok: bool,
        /// Its report or result, or why it failed.
        pub text: String,
    }
}

ffi_enum! {
    /// What runs in the background: a sub-agent the agent started, or one
    /// slow tool call it moved there.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum WorkKind {
        0 Agent {}
        1 Job {}
    }
}

ffi_structs! {
    /// A web page a reply used: one Beep read, or a search result.
    pub struct WebSource {
        /// An http(s) URL.
        pub url: String,
        /// The page's title, empty when unknown.
        pub title: String,
    }

    /// The web pages one turn used, pages read first, at most `MAX_SOURCES`.
    pub struct TurnSources {
        /// The `HistoryEntry::User` `timestamp` of the turn.
        pub message: i64,
        pub sources: Vec<WebSource>,
    }
}

/// The most sources a reply shows.
pub const MAX_SOURCES: usize = 6;

ffi_structs! {
    /// The agent one turn ran as.
    pub struct TurnAgent {
        /// The `HistoryEntry::User` `timestamp` of the turn.
        pub message: i64,
        /// The agent's name, like "Shopping". `None` for General and for a turn
        /// that ran without agents.
        pub agent: Option<String>,
    }
}

ffi_enum! {
    /// One entry of a run's event stream. `RunFinished` carries the session's
    /// `updated_at` once the run saved its turn, `None` when it saved none:
    /// the phone that streamed the run already holds that version.
    #[derive(Debug, Clone, PartialEq)]
    pub enum RunEvent {
        0 TextDelta { delta: String }
        1 ToolStarted { name: String, call_id: String, description: String }
        2 ToolResult { name: String, call_id: String, output: String, success: bool }
        3 Usage { usage: Usage }
        4 Error { message: String }
        5 DeviceQuery { query_id: String, query: DeviceQuery }
        6 RunFinished { status: String, updated_at: Option<SystemTime> }
        7 RunStarted { run_id: String, session_id: String }
        8 ReasoningDelta { delta: String }
        9 MemoryScore { probability: f64 }
        10 AutoPicked { pick: AutoPick }
        11 Transcript { text: String } since VOICE
        12 Sources { sources: Vec<WebSource> } since SOURCES
    }
}

ffi_enum! {
    /// Work the server asks the phone to do during a run. `Approve` asks the user to
    /// allow a tool call, answered as `{"decision": "allow"}`, where the
    /// decision is `allow`, `always` or `deny`. `Browser` and `MacFiles` are
    /// unused: the agent no longer drives the in-app browser or reads Mac
    /// files, so the server never sends them. They stay so released apps
    /// still decode the enum.
    #[derive(Debug, Clone, PartialEq)]
    pub enum DeviceQuery {
        0 Contacts { name: String }
        1 Browser { command: BrowserCommand }
        3 Approve { approval: Approval }
        4 MacFiles { command: MacFileCommand } since MAC_FILES
    }
}

ffi_enum! {
    /// What Beep asks the Mac app to do with the user's files. Paths are
    /// absolute or start at `~`, and stay under the home folder. `List`
    /// answers a folder's entries as a JSON list of `name`, `folder`, `size`
    /// and `modified`; `Find` the paths under `folder` whose name contains
    /// `name`, as a JSON list; `Read` a file as JSON with `name`, `mime`,
    /// `size` and base64 `data`, up to 5 MB. Unused: kept for
    /// wire compatibility with released apps, never sent.
    #[derive(Debug, Clone, PartialEq)]
    pub enum MacFileCommand {
        0 List { path: String }
        1 Find { name: String, folder: String }
        2 Read { path: String }
    }
}

ffi_structs! {
    /// A tool call that waits for the user's OK, as the phone shows it.
    pub struct Approval {
        /// The app's name, such as "Fastmail".
        pub app: String,
        /// What the tool does, such as "send email".
        pub action: String,
        /// The call's arguments, labelled for reading.
        pub fields: Vec<ApprovalField>,
    }

    pub struct ApprovalField {
        pub label: String,
        pub value: String,
    }

    /// An approval a run is waiting on, carried by a `Notice` so the phone can
    /// answer it from the notification.
    pub struct PendingApproval {
        pub run_id: String,
        pub query_id: String,
        pub approval: Approval,
    }
}

ffi_enum! {
    /// The user's answer to an `Approval`. `Always` also stops asking for
    /// that action.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ApprovalDecision {
        0 Deny {}
        1 Allow {}
        2 Always {}
    }
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deny {} => "deny",
            Self::Allow {} => "allow",
            Self::Always {} => "always",
        }
    }
}

ffi_enum! {
    /// An action for the in-app browser on the phone. Unused: kept for wire
    /// compatibility with released apps, never sent.
    #[derive(Debug, Clone, PartialEq)]
    pub enum BrowserCommand {
        0 Navigate { url: String }
        1 Click { selector: String }
        2 Type { selector: String, text: String }
        3 Scroll { direction: ScrollDirection }
        4 GetText { selector: String }
        5 GetPageInfo {}
        6 Back {}
        7 Forward {}
        8 Reload {}
        9 ReadPage {}
        10 ClickElement { index: u32 }
        11 TypeElement { index: u32, text: String, submit: bool }
        12 WaitFor { selector: String }
    }
}

ffi_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ScrollDirection {
        0 Up {}
        1 Down {}
    }
}

ffi_enum! {
    /// The phone's answer to a `DeviceQuery`.
    #[derive(Debug, Clone, PartialEq)]
    pub enum DeviceAnswer {
        0 Ok { output: String }
        1 Err { message: String }
    }
}

ffi_enum! {
    /// A session message. An `Assistant` entry's `tools` are the tool results
    /// of the turn it ends; its `content` is empty when the turn had no text.
    /// `memory_score` is Jev's score for the turn, on its last assistant entry,
    /// `reasoning` is the model's reasoning over the whole turn, and `model`
    /// the model that answered it, empty when not recorded.
    /// A `User` entry's `auto` is what Jev picked for it, when it did.
    /// `Scheduled` stands in for the prompt a routine woke the agent with;
    /// `label` is the routine's name, empty when it has none.
    /// `Approval` ends a session that stopped to ask before a tool call, so
    /// the phone can show the card again; `query_id` is the call to answer.
    #[derive(Debug, Clone, PartialEq)]
    pub enum HistoryEntry {
        0 User { content: String, timestamp: i64, auto: Option<AutoPick> }
        1 Assistant { content: String, timestamp: i64, tools: Vec<HistoryTool>, memory_score: Option<f64>, reasoning: String, model: String }
        2 Scheduled { label: String, timestamp: i64 }
        3 Approval { query_id: String, approval: Approval }
    }
}

ffi_structs! {
    /// A tool result kept in session history, so the client can rebuild its card.
    pub struct HistoryTool {
        pub name: String,
        pub call_id: String,
        /// The tool output, cut to at most 16 KiB.
        pub output: String,
        pub success: bool,
    }

    /// A conversation starter: `title` goes on a chip, `prompt` is sent when tapped.
    pub struct PromptSuggestion {
        pub title: String,
        pub prompt: String,
    }

    /// Starters for the empty chat. `personal` is false for the fixed fallback
    /// sent when the model failed, which is not worth keeping.
    pub struct PromptSuggestions {
        pub suggestions: Vec<PromptSuggestion>,
        pub personal: bool,
    }

    /// Tokens spent on one model call.
    #[derive(Default)]
    pub struct Usage {
        pub prompt_tokens: u32,
        pub completion_tokens: u32,
        pub total_tokens: u32,
        pub cache_read_tokens: u32,
        pub cache_creation_tokens: u32,
    }

    /// One entry of a run's event stream, numbered from 1.
    pub struct Event {
        pub seq: i64,
        pub event: RunEvent,
    }

    pub struct Run {
        pub id: String,
        pub session_id: String,
        pub status: String,
    }

    pub struct Session {
        pub id: String,
        pub title: String,
        pub pinned: bool,
        /// When its turns last changed, in milliseconds since the Unix epoch. A
        /// phone holding a copy from then has nothing to fetch.
        pub updated_at: i64,
    }

    /// A shared copy of a session.
    pub struct SharedLink {
        pub token: String,
        pub url: String,
        pub title: String,
        /// When the copy was made, in milliseconds since the Unix epoch.
        pub shared_at: i64,
    }

    /// What a shared link shows: the session's messages as they were shared.
    pub struct SharedSession {
        pub title: String,
        /// In milliseconds since the Unix epoch.
        pub shared_at: i64,
        pub entries: Vec<HistoryEntry>,
    }

    /// A session whose messages hold the searched words.
    pub struct SessionMatch {
        pub session: Session,
        /// The words around the latest match, cut short with `...`.
        pub snippet: String,
        /// Whether the user wrote the matching message, rather than Beep.
        pub from_user: bool,
    }
}

ffi_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ChatKind {
        0 Session {}
        1 Group {}
    }
}

ffi_structs! {
    /// A chat as `GetUpdates` sends it.
    pub struct ChatInfo {
        pub id: Uuid,
        pub kind: ChatKind,
        /// A session's title or a group's name.
        pub title: String,
        /// Pinned to the top of the caller's list.
        pub pinned: bool,
        /// A group's members, oldest first; empty for a session.
        pub members: Vec<GroupMember>,
        /// Beep may use the caller's apps in this group when others ask.
        pub apps_shared: bool,
    }

    /// One step of a chat: a message, a tool call, an approval card.
    pub struct ChatMessage {
        /// Grows with each message on the server, across chats.
        pub id: i64,
        /// The message that opened its turn, its own id for that one. `None` in
        /// groups.
        pub turn_id: Option<i64>,
        /// In milliseconds since the Unix epoch.
        pub timestamp: i64,
        pub body: MessageBody,
    }

    /// What a turn ran with. It sits on the message that opened the turn and
    /// changes as the turn goes on, which sends that message again.
    pub struct TurnInfo {
        /// The model that answered, empty when not recorded.
        pub model: String,
        /// What Jev picked, when it did.
        pub auto: Option<AutoPick>,
        /// The model's reasoning over the whole turn.
        pub reasoning: String,
        pub memory_score: Option<f64>,
    }
}

ffi_enum! {
    /// `User` and `Scheduled` open a turn and carry it. `Member` and `Beep`
    /// are group messages; `you` marks the caller's own.
    #[derive(Debug, Clone, PartialEq)]
    pub enum MessageBody {
        0 User { content: String, images: Vec<ImageRef>, turn: TurnInfo }
        1 Scheduled { label: String, turn: TurnInfo }
        2 Assistant { content: String }
        3 Tool { call_id: String, name: String, output: String, success: bool }
        4 Approval { query_id: String, approval: Approval }
        5 Member { name: String, you: bool, content: String, images: Vec<ImageRef> }
        6 Beep { content: String }
    }
}

ffi_enum! {
    /// One entry of the caller's log. A message that changes comes again
    /// under a newer id; `Deleted` and `ChatDeleted` tell the client to drop
    /// its copy.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Update {
        0 Message { update_id: i64, chat_id: Uuid, message: ChatMessage }
        1 Deleted { update_id: i64, chat_id: Uuid, message_id: i64 }
        2 Chat { update_id: i64, chat: ChatInfo }
        3 ChatDeleted { update_id: i64, chat_id: Uuid }
    }
}

ffi_structs! {
    /// A page of the caller's updates.
    pub struct UpdatePage {
        pub updates: Vec<Update>,
        /// The update id to send as `after` next.
        pub last: i64,
        /// More updates follow; call again.
        pub more: bool,
        /// The server no longer has every update after `after`: drop every
        /// chat and start over from `updates`.
        pub reset: bool,
    }

    /// A message the server pushes to the user's open notification streams.
    pub struct Notice {
        pub title: String,
        pub body: String,
        /// The chat or group the notice is about. On a computer it may instead
        /// be an `AgentNoticeKey`, for a coding-agent notice.
        pub session_id: String,
        /// Slug of an app that lost its login; tapping the notice opens Apps.
        pub app: Option<String>,
        /// A tool call waiting for the user's OK, answered from the notice.
        pub approval: Option<PendingApproval>,
        /// Nothing to show: `session_id` changed on the server, or the chat list
        /// did when it is empty, so the phone refreshes its copy.
        pub changed: bool,
        /// `session_id` is a group's id. Defaults so a phone from before groups
        /// still decodes notices.
        #[serde(default)]
        pub group: bool,
    }
}

impl Notice {
    /// A background question that stopped waiting, answered on some device
    /// or withdrawn: `session_id` is its chat, and `approval` names it, to
    /// clear its card. Such a notice also has `changed` set and no title,
    /// so apps from before it only refresh the chat.
    pub fn resolved(session_id: String, question: PendingApproval) -> Self {
        Self {
            title: String::new(),
            body: String::new(),
            session_id,
            app: None,
            approval: Some(question),
            changed: true,
            group: false,
        }
    }

    /// The `query_id` of the question this notice says stopped waiting, or
    /// `None` for any other notice.
    pub fn resolved_question(&self) -> Option<&str> {
        match &self.approval {
            Some(question) if self.changed && !self.group => Some(&question.query_id),
            _ => None,
        }
    }
}
