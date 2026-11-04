//! Every request the apps can send: its op, its fields and its reply.
//!
//! Each request is a struct declared in `requests!`. Op numbers are never
//! reused, and a request only gains `#[serde(default)]` fields at its end.

use crate::{link::LinkOffer, wire, *};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;
use uuid::Uuid;

/// A request type. Its frame starts with `OP`; the body is the struct's fields.
pub trait Rpc: Serialize + DeserializeOwned + Send + 'static {
    const OP: Op;
}

/// A request answered by exactly one reply.
pub trait Call: Rpc {
    type Reply: Serialize + DeserializeOwned + Send + Sync + 'static;
}

/// A request answered by any number of replies, ended by finishing the stream.
pub trait Stream: Rpc {
    type Reply: Serialize + DeserializeOwned + Send + Sync + 'static;
}

/// A `Stream` request that also sends: after its request frame, the
/// client sends any number of `Up` frames and finishes its side when done.
pub trait Duplex: Stream {
    type Up: Serialize + DeserializeOwned + Send + Sync + 'static;
}

requests! {
    /// Opens every connection: the server reads it before any other stream,
    /// and sends nothing newer than `wire` on the connection. Clients from
    /// before it never send it and read wire 1. A `Hello` later in a
    /// connection changes nothing.
    0 Hello { wire: u32, platform: Option<Platform>, version: Option<String> } => Call<()>
    1 Health {} => Call<()>
    /// Starts a run and streams its events, starting with `RunStarted`. Resending
    /// the same `request_id` follows the existing run instead of starting another.
    /// `images` are `data:image/...;base64,` URIs; the server keeps them with
    /// the chat, and `GetSessionImages` lists them. `model` is a `ModelChoice::id` to run this
    /// one turn on instead of the picked model, and `thinking` how long it
    /// reasons. `platform` is the app sending it,
    /// missing from apps older than the field. `files` are other files sent
    /// with the message, which the model reads and `GetSessionFiles` lists;
    /// older apps send none. `research` starts a research in a new chat:
    /// a lead agent plans it with the user, then a team of agents researches
    /// it in the background (see `GetResearch`). `voice` is a spoken message
    /// of at most `MAX_VOICE` bytes, of one of `VOICE_TYPES`: the server
    /// transcribes it, streams the transcript as `RunEvent::Transcript` and
    /// runs it as the message, and keeps the recording with the turn like
    /// `files`. `message` may then be empty. `spoken` marks a message said
    /// in live mode, so the apps can show those turns as a transcript (see
    /// `SpokenTurns`); a `voice` message is spoken either way.
    2 Chat {
        session_id: Uuid,
        request_id: Uuid,
        message: String,
        images: Vec<String>,
        model: Option<String>,
        thinking: Option<ReasoningLevel>,
        #[serde(default)]
        platform: Option<Platform>,
        #[serde(default)]
        files: Vec<Upload>,
        #[serde(default)]
        research: bool,
        #[serde(default)]
        voice: Option<Upload>,
        #[serde(default)]
        spoken: bool,
    } => Stream<Event>
    3 GetRun { id: String } => Call<Run>
    4 CancelRun { id: String } => Call<()>
    5 GetEvents { run_id: String, after: i64 } => Stream<Event>
    /// Returns whether a run was still waiting for this answer.
    6 DeviceResult { run_id: String, query_id: String, answer: DeviceAnswer } => Call<bool>
    7 ListSessions {} => Call<Vec<Session>>
    8 GetSession { id: String } => Call<Vec<HistoryEntry>>
    9 RenameSession { id: String, title: String } => Call<()>
    10 DeleteSession { id: String } => Call<()>
    11 PinSession { id: String, pinned: bool } => Call<()>
    /// Every memory, newest first, including ones whose time has passed.
    12 ListMemories {} => Call<Vec<Memory>>
    13 DeleteMemory { id: String } => Call<()>
    14 ListCrons {} => Call<Vec<Cron>>
    /// Returns the new cron's id. A `schedule` of `arrive:<place id>` or
    /// `leave:<place id>` starts it on arriving at or leaving a saved place
    /// instead of at a time; ending it in `:daily` runs it once a day at most,
    /// and in `:once` only the next time.
    15 CreateCron { label: String, prompt: String, schedule: String } => Call<String>
    /// Returns whether the cron is now enabled.
    16 ToggleCron { id: String } => Call<bool>
    17 DeleteCron { id: String } => Call<()>
    18 UpdateCron { id: String, enabled: Option<bool>, label: Option<String>, schedule: Option<String> } => Call<()>
    /// `fcm_token` is the push token: Firebase's, or with `apns` set, the
    /// one Apple gave an iPhone for that environment.
    19 RegisterDevice {
        device_id: String,
        fcm_token: String,
        #[serde(default)]
        apns: Option<ApnsEnvironment>,
    } => Call<()>
    20 UnregisterDevice { device_id: String } => Call<()>
    /// `place` is the phone's reverse-geocoded name for the fix, such as
    /// "Shinjuku, Tokyo, Japan", when it has one.
    21 UpdateLocation { latitude: f64, longitude: f64, accuracy: f64, timezone: String, place: Option<String> } => Call<()>
    23 StreamNotifications {} => Stream<Notice>
    /// The curated apps the assistant can use, with this user's connection state.
    28 ListApps {} => Call<Vec<AppIntegration>>
    /// Starts OAuth for an app; open the returned URL in a browser.
    29 ConnectApp { slug: String } => Call<ConnectLink>
    30 DisconnectApp { slug: String } => Call<()>
    /// Personalized conversation starters, cached per user for 12 hours.
    /// `refresh` skips the cache, at most 3 times in 10 minutes.
    31 SuggestPrompts { refresh: bool } => Call<PromptSuggestions>
    /// Starts OAuth for an MCP app such as Fastmail, whose provider only
    /// redirects to localhost. The phone listens on `redirect_uri`, an
    /// `http://localhost:<port>/...` address, and opens the returned URL.
    32 StartAppAuth { slug: String, redirect_uri: String } => Call<ConnectLink>
    /// Hands the redirect's `code`, `state` and `iss` to the server, which
    /// keeps the tokens and refreshes them.
    33 FinishAppAuth { slug: String, code: String, state: String, issuer: Option<String> } => Call<()>
    /// Replaces a memory's text; it then counts as said by the user.
    34 UpdateMemory { id: String, content: String } => Call<()>
    /// Returns how many memories were deleted.
    35 ClearMemories {} => Call<u64>
    /// Returns how many of the sessions existed and were deleted.
    36 DeleteSessions { ids: Vec<String> } => Call<u64>
    /// Runs a cron now, keeping its schedule; a one-off reminder is used up.
    /// Returns the session the run went to.
    37 RunCron { id: String } => Call<String>
    /// What Beep may do in a connected app, and which of its actions act.
    38 GetAppPermissions { slug: String } => Call<AppPermissions>
    /// `allowed` names the actions, by tool, that run without asking under
    /// `Ask`.
    39 SetAppPermissions { slug: String, level: PermissionLevel, allowed: Vec<String> } => Call<()>
    /// Answers a tool call the session stopped on to ask the user, and
    /// resumes the session in a new run. Resending the same answer returns
    /// the same run. `for_session` with `Allow` also lets the same action
    /// run without asking for the rest of this chat ("Allow for this
    /// session"); for a card of background work in the chat, such as a
    /// coding agent's question, its later cards of the same kind are
    /// allowed too. Servers from before it allow once.
    40 AnswerApproval { session_id: Uuid, query_id: String, decision: ApprovalDecision, #[serde(default)] for_session: bool } => Call<Run>
    /// The models the caller can pick for their own turns.
    41 GetModels {} => Call<Models>
    /// Picks the model for the caller's turns.
    42 SetModel { id: String } => Call<Models>
    /// The ones of `ids` the server still has, so a phone can drop the
    /// chats another phone deleted.
    43 ExistingSessions { ids: Vec<String> } => Call<Vec<String>>
    /// Removes the turn whose user message has `timestamp`, as `HistoryEntry`
    /// gives it, with the reply and tool calls that followed it, from both
    /// the history and what the model sees. Fails while the chat has a run.
    44 DeleteTurn { session_id: String, timestamp: i64 } => Call<()>
    /// Picks how long the model reasons on the caller's turns.
    45 SetThinking { level: ReasoningLevel } => Call<Models>
    /// Like `DeleteTurn`, and also removes every turn after it, so the chat
    /// ends before that prompt. A rerun sends the prompt again after this.
    46 DeleteTurnsFrom { session_id: String, timestamp: i64 } => Call<()>
    /// The sessions where the user or Beep wrote `query`, ignoring case,
    /// most recently changed first.
    47 SearchSessions { query: String } => Call<Vec<SessionMatch>>
    /// Turns Auto on or off for the caller: Jev then picks the model and
    /// thinking of each prompt from the server's Auto models.
    48 SetAutoModel { enabled: bool } => Call<Models>
    /// Lets the device that made `offer` sign into the caller's account.
    /// Any key of the account may link, and only a key with no account.
    49 LinkDevice { offer: LinkOffer } => Call<LinkedDevice>
    /// The account's keys: the primary key first, then linked devices.
    50 ListDevices {} => Call<Vec<LinkedDevice>>
    /// Unlinks a device. The primary key may unlink any; a device only
    /// itself.
    51 RemoveDevice { id: String } => Call<()>
    /// Freezes a copy of the session that anyone with the link can open,
    /// replacing the copy an earlier share made and keeping its link. The
    /// copy holds the messages only: no tool results, reasoning or context.
    52 ShareSession { id: String } => Call<SharedLink>
    /// Deletes the session's shared copy, so its link stops working.
    53 UnshareSession { id: String } => Call<()>
    /// The shared copy behind a link's `token`, made by any account.
    54 GetShare { token: String } => Call<SharedSession>
    /// Copies a shared session into the caller's chats as `session_id`, so
    /// they can carry it on. Returns the new session.
    55 CopyShare { token: String, session_id: Uuid } => Call<Session>
    /// The phone's timezone, such as "Europe/Madrid", which routines and
    /// reminders run in. Sent on connect, so no location is needed for it.
    56 SetTimezone { timezone: String } => Call<()>
    /// Makes a group chat `id` with the caller as its first member, shown
    /// to the others as `member_name`.
    57 CreateGroup { id: Uuid, name: String, member_name: String } => Call<Group>
    /// The caller's groups, most recently active first.
    58 ListGroups {} => Call<Vec<Group>>
    /// A group with its latest messages, oldest first.
    59 GetGroup { id: String } => Call<GroupChat>
    /// Posts to a group. A message that mentions `@beep` asks Beep to
    /// reply, running as the sender. Sending the same `request_id` twice
    /// posts once. `images` are `data:image/...;base64,` URIs, kept with the
    /// group and listed by `GetGroupImages`; older clients send none.
    60 SendToGroup { group_id: String, request_id: Uuid, message: String, #[serde(default)] images: Vec<String> } => Call<GroupMessage>
    /// The group's invite link. `reset` makes a new one, so the old link
    /// stops working.
    61 GroupInvite { id: String, reset: bool } => Call<GroupLink>
    /// What an invite link's `token` leads to, before joining.
    62 PeekGroup { token: String } => Call<GroupPreview>
    /// Joins the group behind an invite `token` as `member_name`.
    63 JoinGroup { token: String, member_name: String } => Call<Group>
    /// Leaves a group. The last member to leave deletes it.
    64 LeaveGroup { id: String } => Call<()>
    /// Pins a group to the top of the caller's list, as `PinSession` does a chat.
    65 PinGroup { id: String, pinned: bool } => Call<()>
    /// Lets Beep use the caller's apps in a group when other members ask, or
    /// stops it. Their own asks always may.
    66 ShareGroupApps { id: String, shared: bool } => Call<()>
    /// Answers an app action Beep asked to take for the caller from a group,
    /// sent as a `Notice` with `group` set. Allowing runs it, and Beep tells
    /// the group how it went.
    67 AnswerGroupApproval { query_id: String, allow: bool } => Call<()>
    /// Up to `FILE_PART` bytes of a file Beep sent, from `offset` on, as a
    /// `send_file` tool result names it by `id`.
    68 GetFile { id: String, offset: u64 } => Call<FilePart>
    /// The reminders set in a group, soonest first. Members only.
    70 ListGroupReminders { group_id: String } => Call<Vec<GroupReminder>>
    /// Deletes one of a group's reminders. Any member may.
    71 DeleteGroupReminder { group_id: String, id: String } => Call<()>
    /// The pictures the user sent in a session, by the `timestamp` of the
    /// `HistoryEntry::User` they came with. `GetFile` reads each one.
    72 GetSessionImages { session_id: String } => Call<Vec<SentImages>>
    /// The pictures members sent in a group, by the `seq` of their
    /// `GroupMessage`. Members only.
    73 GetGroupImages { group_id: String } => Call<Vec<SentImages>>
    /// The caller's saved places, by name.
    74 ListPlaces {} => Call<Vec<Place>>
    /// Forgets a place, and the routines that start there with it.
    75 DeletePlace { id: String } => Call<()>
    /// Renames a place or changes how near counts as being there.
    /// `radius` is in metres, 25 to 1000.
    76 UpdatePlace { id: String, name: Option<String>, radius: Option<u32> } => Call<Place>
    /// The phone's own geofencing saw it arrive at or leave saved place
    /// `id`, so the server starts that place's routines without waiting for
    /// a location fix of its own. Returns whether it changed where the server
    /// had the phone; false when it already knew, so nothing started.
    77 CrossPlace { id: String, crossing: PlaceCrossing } => Call<bool>
    /// The reads of the caller's mail and calendars that added or confirmed
    /// memories, newest first. A notice with no `session_id`, `app` or
    /// `approval` is one finishing, so tapping it opens the newest here.
    78 ListMemoryReads {} => Call<Vec<MemoryRead>>
    /// The caller's updates after `after`, the last update id the client
    /// holds, oldest first. A client starts from 0 and calls again with
    /// `UpdatePage::last` while `more` is set.
    79 GetUpdates { after: i64 } => Call<UpdatePage>
    /// The caller's newest update id now and whenever it changes, so a
    /// client knows when to call `GetUpdates`.
    80 StreamUpdates {} => Stream<i64>
    /// The caller's skills and those of every group they are in, the
    /// caller's own first, each most recently used first.
    81 ListSkills {} => Call<Vec<Skill>>
    /// Changes what is given; the old version stays for `UndoSkill`. A
    /// group's skill may be changed by any member.
    82 UpdateSkill {
        id: String,
        name: Option<String>,
        description: Option<String>,
        body: Option<String>,
        enabled: Option<bool>,
    } => Call<Skill>
    83 DeleteSkill { id: String } => Call<()>
    /// Puts back the version before the skill's last change.
    84 UndoSkill { id: String } => Call<Skill>
    /// Whether agents are on, and the agents Jev picks from.
    85 GetAgents {} => Call<Agents>
    /// Turns agents on or off for every user. Ignored now: there are no
    /// agents to turn on.
    86 SetAgents { enabled: bool } => Call<Agents>
    /// The price chart of `symbol` over `range`, as `show_stock_chart`
    /// draws it, for a chart's range buttons.
    87 StockChart { symbol: String, range: ChartRange } => Call<Chart>
    /// A code another device scans or types to join the caller's account,
    /// bringing its own account's chats, memories and devices along.
    /// `name` is what the caller calls itself, such as "MacBook", and
    /// becomes its name in the device list. Any key of the account may invite.
    88 InviteDevice { name: String } => Call<DeviceInvite>
    /// Who made the invite `code`, before joining.
    89 PeekInvite { code: String } => Call<JoinPreview>
    /// Moves the caller's account into the one that made `code`: its chats,
    /// memories, routines, places and keys. Only an account's primary key
    /// may, and not while one of its chats has a run. The server then
    /// closes the moved keys' connections, so they sign in again.
    90 JoinAccount { code: String } => Call<()>
    /// The specialized agent each turn of a session ran as, by the
    /// `timestamp` of the `HistoryEntry::User` that started it. Every turn is
    /// listed, so the newest entry is the newest turn.
    91 GetSessionAgents { session_id: String } => Call<Vec<TurnAgent>>
    /// `GetRouteCard` as apps from before stop codes read it.
    92 GetRoute { call_id: String } => Call<RouteCardV1>
    /// The routes a `get_route` call found, by the call's id, for its card.
    /// `NotFound` for a call that found none or is not the caller's.
    93 GetRouteCard { call_id: String } => Call<RouteCard>
    /// The MCP servers the user added by URL.
    94 ListMcpServers {} => Call<Vec<McpServer>>
    /// Adds a remote MCP server by its https URL. One that needs no sign-in
    /// connects at once; otherwise open `auth_url`, which comes back through
    /// `mcp_callback_url()` with what `FinishMcpAuth` takes.
    95 AddMcpServer { name: String, url: String } => Call<McpConnect>
    /// Connects an added server again, signing in again when it needs to.
    96 ConnectMcpServer { id: String } => Call<McpConnect>
    /// Hands the sign-in redirect's `code`, `state` and `iss` to the server,
    /// which finds the server the flow was for by `state`.
    97 FinishMcpAuth { code: String, state: String, issuer: Option<String> } => Call<McpServer>
    98 RemoveMcpServer { id: String } => Call<()>
    /// The run going on in the caller's session, started from any of their
    /// devices, to follow with `GetEvents` from 0. `None` when it is idle.
    99 ActiveRun { session_id: Uuid } => Call<Option<Run>>
    /// The accounts of an app that connects several, such as Fastmail, in
    /// the order Beep uses them: the agent's default account comes first.
    /// Empty when none is connected.
    100 ListAppAccounts { slug: String } => Call<Vec<AppAccount>>
    /// Like `StartAppAuth`, but signs in another account instead of
    /// reconnecting the first. `FinishAppAuth` completes it. Signing in to an
    /// account Beep already has replaces its old login.
    101 AddAppAccount { slug: String, redirect_uri: String } => Call<ConnectLink>
    /// Disconnects one account, by `AppAccount::id`. `DisconnectApp` still
    /// disconnects them all.
    102 RemoveAppAccount { slug: String, id: String } => Call<()>
    /// The files the user sent in a session, other than pictures, by the
    /// `timestamp` of the `HistoryEntry::User` they came with. `GetFile`
    /// reads each one.
    103 GetSessionFiles { session_id: String } => Call<Vec<SentFiles>>
    /// Every tool an added server lists, as it last listed them, and which
    /// of them the agent gets.
    104 ListMcpTools { id: String } => Call<McpTools>
    /// Turns one of an added server's tools on or off for the agent. The
    /// server takes more than `McpTools::limit`; the app warns first.
    105 SetMcpTool { id: String, tool: String, enabled: bool } => Call<McpTools>
    /// The research a chat runs, with its log entries after `after` (0 for
    /// all), or `None` when the chat is not a research.
    106 GetResearch { session_id: Uuid, after: i64 } => Call<Option<Research>>
    /// Stops a research's agents. The chat and what they wrote stay.
    107 StopResearch { session_id: Uuid } => Call<()>
    /// A research's paper typeset as a PDF, with its sources. `NotFound`
    /// when the chat is not a research or its paper is not written yet.
    108 GetResearchPdf { session_id: Uuid } => Call<Upload>
    /// Every research of the account, newest first.
    109 ListResearches {} => Call<Vec<ResearchSummary>>
    /// A notice as a short voice note: `text` is it rewritten for listening,
    /// with no markdown or links, and `audio` that spoken. The app sends the
    /// `title` and `body` of a `Notice` it got, or a reply to read out with
    /// an empty `title`.
    110 SpeakNotice { title: String, body: String } => Call<VoiceNote>
    /// The machines running beep-node on the account, by name. A linked
    /// machine shows once it has connected.
    111 ListMachines {} => Call<Vec<Machine>>
    /// What each memory that is not about the user is about. Ids are the
    /// ones `ListMemories` gives; a memory missing here is about the user.
    112 ListMemoryScopes {} => Call<Vec<ScopedMemory>>
    /// Transcribes speech as it is said. After this frame the client sends
    /// the audio as more frames on the same stream, each a msgpack bin of
    /// mono 16-bit little-endian PCM at `sample_rate` (only 16000 is taken),
    /// and finishes its side when the speech ends. The server streams the
    /// words back as `Transcription::Delta`, then one `Done`, or only
    /// `Unavailable` when it has no live transcription. See `Duplex`.
    113 Transcribe { sample_rate: u32 } => Stream<Transcription>
    /// When each spoken prompt of the chat was sent: the `timestamp`s of
    /// its `HistoryEntry::User`s that were said aloud, oldest first.
    114 SpokenTurns { session_id: Uuid } => Call<Vec<i64>>
    /// `text` spoken as it is, with no rewrite, for live mode to read a
    /// reply out sentence by sentence. At most `MAX_SAY_CHARS` characters.
    115 Say { text: String } => Call<VoiceNote>
    /// A short line for live mode to say while a tool runs and the reply
    /// has said nothing for a while, such as "Let me check build-box".
    /// `step` is the tool step's label, `said` the user's message, whose
    /// language the line is in. `still` asks for the line that says the
    /// work goes on, after a long quiet.
    116 Filler { step: String, said: String, still: bool } => Call<VoiceNote>
    /// The sub-agents and background jobs the agent started in the chat:
    /// running ones first, with what they did last, then those that ended
    /// in the last hour, newest first. Unused: the agent no longer starts
    /// background work, so the server answers an empty list.
    117 ListWork { session_id: Uuid } => Call<Vec<WorkItem>>
    /// Stops a running sub-agent or job of the chat, and nothing comes back
    /// from it. `false` when it had already ended; `NotFound` when the chat
    /// has no such id. Unused: the server answers `NotFound`.
    118 CancelWork { session_id: Uuid, id: String } => Call<bool>
    /// The standing permissions the user gave routines that are still in
    /// force, newest first. A grant ends when it is revoked, when it
    /// expires and when its routine is deleted or ends itself.
    119 ListGrants {} => Call<Vec<Grant>>
    /// Ends a grant, by `Grant::id`, so its routine asks again. `NotFound`
    /// when the caller has no such grant.
    120 RevokeGrant { id: String } => Call<()>
    /// The questions background work in the chat is waiting on, oldest
    /// first: app actions of its sub-agents and jobs that ask first, each
    /// answered with `AnswerApproval`. `run_id` is empty. One that stops
    /// waiting, answered on any device or withdrawn, leaves the list and
    /// comes as a notice `Notice::resolved_question` reads.
    125 ListQuestions { session_id: Uuid } => Call<Vec<PendingApproval>>
    /// What each tool step of a session showed while it ran, by
    /// `HistoryTool::call_id` (`MessageBody::Tool::call_id` in updates), such
    /// as "Recalling memories". Steps saved before the server kept it are
    /// left out, as are steps that had none. `NotFound` when the caller has
    /// no such session.
    126 GetToolLabels { session_id: Uuid } => Call<Vec<ToolLabel>>
    /// Where Beep failed the caller, as the improvement watcher found it
    /// after their chats, most recently seen first: only those with `status`
    /// when given. Servers from before every user got findings answer
    /// `Unauthorized` to anyone but an admin.
    127 ListFindings { status: Option<FindingStatus> } => Call<Vec<Finding>>
    /// Resolves or dismisses a finding, by `Finding::id`, or opens it again.
    /// Nothing in Beep changes by itself. `NotFound` when the caller has no
    /// such finding.
    128 UpdateFinding { id: String, status: FindingStatus } => Call<Finding>
    /// The web pages each turn of a session used, by the `timestamp` of the
    /// `HistoryEntry::User` that started it, for the source chips under its
    /// reply. Turns that used none are left out. `NotFound` when the caller
    /// has no such session.
    129 GetSessionSources { session_id: Uuid } => Call<Vec<TurnSources>>
    /// What each machine has installed and can be asked to do, for the
    /// command center. A machine that has not reported yet has empty lists.
    130 ListMachineDetails {} => Call<Vec<MachineDetails>>
    /// The coding-agent sessions on the caller's online machines, newest
    /// activity first: those Beep started, and those started outside Beep
    /// that something runs now. A machine that does not answer in time is
    /// left out.
    131 ListAgentSessions {} => Call<Vec<MachineSession>>
    /// Answers the question a session Beep started asks, by
    /// `AgentQuestion::id`. `reply` is one of its options, by id or name;
    /// anything else declines and goes to the agent as its next prompt.
    /// `NotFound` when the machine is not one of the caller's online ones
    /// or has no such session, `Conflict` when the session no longer asks
    /// `question`.
    132 AnswerAgentQuestion { machine_id: String, agent: String, session: String, question: u32, reply: String } => Call<()>
    /// The approvals background work waits on in any of the caller's chats,
    /// oldest first. Answer one with `AnswerApproval`.
    133 ListPendingApprovals {} => Call<Vec<ChatApproval>>
    /// The project agent for `project` (`org/repo`), made with its chat on
    /// the first ask. `BadRequest` for a malformed name.
    134 GetProjectAgent { project: String } => Call<ProjectAgent>
    /// The tasks of a project, oldest first. `NotFound` when there is no
    /// such project.
    135 ListProjectTasks { project: Uuid } => Call<Vec<ProjectTask>>
    /// The workers of a project, including those that ended in the last
    /// day. `NotFound` when there is no such project.
    136 ListProjectWorkers { project: Uuid } => Call<Vec<ProjectWorker>>
    /// Moves a task to `Dropped`, or back to `Planned` to reopen it; any
    /// other state is a `BadRequest`. `NotFound` when there is no such
    /// task.
    137 UpdateProjectTask { task: Uuid, state: ProjectTaskState } => Call<ProjectTask>
    /// Stops a running worker. `NotFound` when there is no such worker.
    138 StopProjectWorker { worker: Uuid } => Call<()>
    /// Every project agent, for the command center.
    139 ListProjects {} => Call<Vec<ProjectAgent>>
    /// The caller's online machines that offer a coding agent, for
    /// starting one from the app.
    140 ListAgentHosts {} => Call<Vec<AgentHost>>
    /// Starts a coding-agent session on `prompt` in `cwd` (the machine's
    /// workspace or home folder when `None`). `machine_id` names the
    /// machine; without it the server picks one that offers `agent` (any
    /// when `None`) and has every capability in `needs` and those the
    /// prompt and folder call for, such as Xcode for an iOS app. `chat`, one
    /// of the caller's chats, then hears when a turn ends or the agent
    /// asks something. `NotFound` when the machine is not one of the
    /// caller's online ones or `chat` is not theirs; `BadRequest` saying
    /// why when no machine fits or the node refuses. `model`, an id from
    /// `ListAgentModels`, picks the agent's model; the agent's own default
    /// when `None`. `effort` sets how hard it thinks, one of
    /// `node::agent::efforts(agent)`; `BadRequest` for another. `checkout`
    /// first makes a branch or worktree in `cwd`'s git checkout, named
    /// from the prompt; `BadRequest` when the machine's beep-node is too old
    /// to. Older apps send none of these.
    141 StartAgentSession {
        machine_id: Option<String>,
        agent: Option<String>,
        cwd: Option<String>,
        prompt: String,
        needs: Vec<MachineCapability>,
        chat: Option<Uuid>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<AgentEffort>,
        #[serde(default)]
        checkout: Option<AgentCheckoutPlan>,
    } => Call<StartedAgent>
    /// One session as `ListAgentSessions` lists it, with its plan, the
    /// files it changed and the last `last` entries of its transcript (at
    /// most 200). An outside session has no plan or files. `NotFound` when
    /// the machine is not one of the caller's online ones or has no such
    /// session.
    142 GetAgentSession { machine_id: String, agent: String, session: String, last: u32 } => Call<AgentSessionView>
    /// Sends a session Beep started a message, stops its turn or ends it.
    /// A message waits for a running turn to end. `NotFound` as for
    /// `GetAgentSession`; `BadRequest` saying why when the node refuses.
    143 SteerAgentSession { machine_id: String, agent: String, session: String, steer: AgentSteer } => Call<()>
    /// Turns syncing `project` on or off on one machine. On, `folder` is
    /// its absolute folder there, inside the machine's shared folders;
    /// the machine then keeps it in step with the other machines that
    /// sync it, node to node. Off, the machine stops and its files stay.
    /// `NotFound` when the machine is not one of the caller's,
    /// `BadRequest` when `folder` is missing, the project is pinned, or the
    /// machine refuses the folder.
    150 SetProjectSync { project: String, machine_id: String, folder: Option<String>, on: bool } => Call<ProjectSync>
    /// Every project the caller syncs, with how each machine stands; an
    /// offline machine or one that does not answer in time shows
    /// `SyncStatus::Offline`.
    151 ListProjectSync {} => Call<Vec<ProjectSync>>
    /// What `project`'s sync leaves out on one machine (build output,
    /// ignored files, links), for the file viewer. `NotFound` when the
    /// machine does not sync it or is offline.
    152 GetSyncSkipped { project: String, machine_id: String } => Call<SyncSkipped>
    /// Pins a project to the machines it is on, or unpins it. A pinned
    /// project, such as a large game or video project, never syncs: every
    /// machine stops, and `SetProjectSync` refuses to turn it on. To pin
    /// one to the top of the sidebar instead, see `PinProjectToTop`.
    153 PinProject { project: String, pinned: bool } => Call<ProjectSync>
    /// What the event triage agent found worth telling the user and that
    /// they have not seen or dismissed yet, from the last two days, newest
    /// first, at most 20. Apps show them on their own, apart from any chat.
    160 ListUpdates {} => Call<Vec<AppUpdate>>
    /// Dismisses updates by `AppUpdate::id`, so they leave the list. Ids
    /// the caller does not have are skipped.
    161 DismissUpdates { ids: Vec<i64> } => Call<()>
    /// The caller's credentials: a GitHub token, a token per Forgejo or
    /// Gitea server, and API keys, one per provider. Never the secrets
    /// themselves, only their last four characters.
    162 ListCredentials {} => Call<Vec<Credential>>
    /// Saves a credential, replacing the caller's one of the same kind (for
    /// Forgejo, the same server). A GitHub or Forgejo token is checked with
    /// its host first, and an OpenRouter key with OpenRouter; `BadRequest`
    /// saying why when the host refuses it, or for an unknown provider or a
    /// bad server URL. Git tokens always go to the caller's machines; an API
    /// key only with `to_machines`, as its provider's env var.
    163 SaveCredential { kind: CredentialKind, secret: SecretText, to_machines: bool } => Call<Credential>
    /// Removes one of the caller's credentials, by `Credential::id`, from
    /// Beep and their machines. `NotFound` when the caller has no such one.
    164 RemoveCredential { id: String } => Call<()>
    /// What each of the caller's online machines last said it has free:
    /// disk, memory and load, sampled about once a minute. A machine that
    /// is offline or has not sent any yet is left out.
    165 ListMachineStats {} => Call<Vec<MachineStats>>
    /// Clones a repository onto one of the caller's machines and lists it
    /// as a project. `repo` is `org/repo` (GitHub), a GitHub URL, or a URL
    /// on a Forgejo or Gitea server in the caller's credentials.
    /// `machine_id` names the machine; without it, the caller's environment
    /// (the machine with a workspace), else the only one online. On Linux
    /// it goes to `<workspace or home>/projects/<org>/<repo>`, on macOS and
    /// Windows to `~/Projects/<org>/<repo>`; a folder that already holds
    /// that repository is used as it is. `folder` puts it somewhere else:
    /// an absolute path or one under the home folder (`~/code/repo`). A git
    /// checkout there is imported in place, an empty or missing folder gets
    /// the clone, and a folder with other files gets it in
    /// `<folder>/<repo>`. Apps from before it send no `folder`.
    /// `worktrees` are branches to give a worktree once it is in, as
    /// `AddBranchWorktree` does; one that fails is logged and the import
    /// still stands. Apps from before it send none.
    /// `parent` is the folder a clone goes under, as `<parent>/<org>/<repo>`,
    /// absolute or under the home folder; `folder` wins when both are
    /// given. Apps from before it send none.
    /// `BadRequest` saying why: the repo is malformed or on an unknown host,
    /// the folder is not one Beep takes, it is not found or the token has
    /// no access, the machine is offline, or the clone failed.
    166 ImportProject { repo: String, machine_id: Option<String>, #[serde(default)] folder: Option<String>, #[serde(default)] worktrees: Vec<BranchPlace>, #[serde(default)] parent: Option<String> } => Call<ImportedProject>
    /// The projects the caller imported, by name, then machine; those on
    /// machines no longer linked are left out. `cloned` is always false.
    167 ListImportedProjects {} => Call<Vec<ImportedProject>>
    /// One folder on one of the caller's machines, for choosing where
    /// `ImportProject` puts a project: the folders in it and whether it is
    /// a git checkout. `path` is absolute or under the home folder
    /// (`~/code`); without it, the machine's workspace, else its home.
    /// `BadRequest` saying why: the machine is not one of the caller's, it
    /// is offline or runs no commands, or the path is malformed or not a
    /// folder.
    168 ListMachineFolder { machine_id: String, path: Option<String> } => Call<MachineFolder>
    /// Git checkouts the caller's coding agents worked in that are not
    /// imported yet, for the Add a project sheet: one row per repository,
    /// newest activity first, with its folder on each machine that has it
    /// and how many Claude Code, Codex, pi, flsh and OpenCode sessions ran
    /// there.
    /// Sessions in a subfolder or a worktree count for the checkout's main
    /// folder; the home folder and folders that are not checkouts are left
    /// out. A folder already imported on its machine, or a repository
    /// imported anywhere, is left out too, and so is a machine that is
    /// offline or does not answer in time.
    169 ListFoundFolders {} => Call<Vec<FoundProject>>
    /// The coding-agent sessions that ran in `project`'s folders on the
    /// caller's online machines, newest activity first, at most 50: those
    /// started outside Beep, such as Claude Code in a terminal. `title` is
    /// the session's title, else its first prompt. One nothing runs any
    /// more has status `Ended`; `ResumeAgentSession` picks it up. `NotFound`
    /// when the caller has not imported `project`.
    170 ListProjectSessions { project: String } => Call<Vec<MachineSession>>
    /// Resumes a session started outside Beep, by the agent's own id, as
    /// `ListProjectSessions` lists it: it then runs as a session Beep
    /// started, in its folder, on `prompt` when given. `NotFound` when the
    /// machine is not one of the caller's online ones; `BadRequest` saying
    /// why when the node refuses, such as when the session still runs in a
    /// terminal.
    171 ResumeAgentSession { machine_id: String, agent: String, session: String, prompt: Option<String> } => Call<StartedAgent>
    /// Every machine of the caller's, as `project`'s page lists it: its
    /// folder there, whether Beep is looking through it for a checkout now,
    /// and when it last did since the project was imported. A machine with
    /// a checkout first, then those online, each by name. Asking starts a
    /// look on an online machine not looked through since the import.
    /// Machines are looked through when a project is imported and when
    /// they connect: a checkout whose `origin` remote is the project's is
    /// listed as its folder there, in place. `NotFound` when the caller has
    /// not imported `project`. Admins only.
    172 ListProjectMachines { project: String } => Call<Vec<ProjectMachine>>
    /// The open issues on `project_id`'s issues host that no task of the
    /// project tracks and the caller has not dismissed, newest first, at
    /// most 100. Read with `gh` or `teacli` on the project's home checkout
    /// (else a machine it was imported on) on each call. A project with no
    /// GitHub or Forgejo host gives an empty list. `NotFound` when the
    /// caller has no such project; `BadRequest` with the host's error when
    /// the machine is offline or the command fails.
    173 ListProjectIssues { project_id: Uuid } => Call<Vec<ProjectIssue>>
    /// Makes a task that tracks the open issue `number`, without filing a
    /// new one. With `start`, a worker starts on it (or it waits for a free
    /// slot); without, the project agent is asked to plan it in the project
    /// chat. `NotFound` when the caller has no such project; `Conflict`
    /// when a task already tracks the issue; `BadRequest` saying why when
    /// the issue is not open, the project has no issues host, the agent or
    /// machine is unknown, or the host cannot be read.
    174 TrackProjectIssue { project_id: Uuid, number: u64, start: Option<IssueStart> } => Call<ProjectTask>
    /// Hides the issue `number` from `ListProjectIssues` for this project;
    /// it stays open on the host. `NotFound` when the caller has no such
    /// project.
    175 DismissProjectIssue { project_id: Uuid, number: u64 } => Call<()>
    /// A one-time setup token that links a machine to the caller's account:
    /// on the machine, `beep-node setup <token>` (see `setup::setup_command`).
    /// It works for ten minutes. `name` is what the machine is called once
    /// linked; without it, the name `setup` is given, else the machine's
    /// host name. `BadRequest` when the name is empty or too long or the
    /// server takes no machines.
    180 CreateNodeToken { name: Option<String> } => Call<NodeToken>
    /// Stops a token `CreateNodeToken` gave from working. `NotFound` when
    /// the caller made no such token or it was already used; `BadRequest`
    /// when `token` is not one.
    181 RevokeNodeToken { token: String } => Call<()>
    /// The `Machine::id` that claimed a token `CreateNodeToken` gave, or
    /// `None` while it is unused, so a sheet waiting on it knows which
    /// machine linked, even one set up again. `NotFound` when the caller
    /// made no such token; `BadRequest` when `token` is not one.
    182 GetNodeToken { token: String } => Call<Option<String>>
    /// The caller's open pull requests and issues on the git hosts in their
    /// credentials, newest activity first: the PRs they opened, those
    /// asking for their review, and the issues they opened or are assigned.
    /// Read with the saved tokens and kept for a few minutes, so asking
    /// again soon gives the same list. A host that fails is listed in
    /// `GitItems::hosts` with why, and the others still come.
    183 ListGitItems {} => Call<GitItems>
    /// The caller's GitHub accounts, the default one first, then oldest
    /// first. `ListCredentials` lists only the default one, as its single
    /// GitHub row, for apps from before several accounts.
    190 ListGitHubAccounts {} => Call<Vec<GitHubAccount>>
    /// Saves a GitHub token as one of the caller's accounts, after GitHub
    /// says who it signs in as: it replaces the account with that login,
    /// or adds one. The first account is the default. The orgs the token
    /// can see are kept, to pick the account for a project. `BadRequest`
    /// saying why when GitHub refuses it.
    191 SaveGitHubAccount { secret: SecretText } => Call<GitHubAccount>
    /// Makes one of the caller's GitHub accounts, by `GitHubAccount::id`,
    /// the default: the one machines get as `GH_TOKEN`. `NotFound` when the
    /// caller has no such account. Remove one with `RemoveCredential`.
    192 SetDefaultGitHubAccount { id: String } => Call<()>
    /// Pins `project` (`org/repo`) to the top of the sidebar, or unpins it.
    /// Unlike `PinProject`, which keeps a project off sync, this only sets
    /// where the apps list it. `BadRequest` for a malformed name.
    193 PinProjectToTop { project: String, pinned: bool } => Call<()>
    /// The projects the caller pinned to the top of the sidebar, in the
    /// order they were pinned.
    194 ListProjectsOnTop {} => Call<Vec<String>>
    /// Every project of the caller's with how many of its tasks stand in
    /// each state, its live workers and its latest task, by name: what the
    /// Projects overview shows, in one call instead of one per project.
    195 ListProjectOverview {} => Call<Vec<ProjectOverview>>
    /// The newest version of the caller's instructions for every agent
    /// Beep runs, or `None` before the first save.
    196 GetInstructions {} => Call<Option<InstructionsVersion>>
    /// Saves `text` as the next version of the caller's instructions, at
    /// most 16 KB. `base` is the version the text was edited from (`None`
    /// for the first); `Conflict` when a newer one was saved since, so an
    /// edit on another device is never overwritten. `note` says what made
    /// it, such as "Restored version 3"; empty for an edit.
    197 SaveInstructions { text: String, base: Option<u32>, note: String } => Call<InstructionsVersion>
    /// The versions of the caller's instructions, newest first, at most 50.
    198 ListInstructionsVersions {} => Call<Vec<InstructionsVersion>>
    /// The agent files on each of the caller's online machines: Claude
    /// Code's `~/.claude/CLAUDE.md`, Codex's `~/.codex/AGENTS.md`, Gemini
    /// CLI's `~/.gemini/GEMINI.md` and pi's `~/.pi/agent/AGENTS.md`, for
    /// each agent whose folder is there. A machine that cannot be read
    /// says why in `error`.
    199 ReadAgentFiles {} => Call<Vec<MachineAgentFiles>>
    /// Writes the newest instructions into each agent file on one machine,
    /// between `<!-- beep:start -->` and `<!-- beep:end -->`, for each
    /// agent whose folder is there; text outside the markers stays. Gives
    /// the files as they are after. `NotFound` when the machine is not one
    /// of the caller's online ones; `BadRequest` before the first save or
    /// when the machine refuses.
    200 SyncAgentFiles { machine_id: String } => Call<MachineAgentFiles>
    /// The terminals of `project` (`org/repo`): commands kept running in
    /// the background on the caller's machines, apart from any chat or
    /// app, oldest first, with how each stands now.
    201 ListTerminals { project: String } => Call<Vec<Terminal>>
    /// Starts `command` in `cwd` on one of the caller's online machines as
    /// a terminal of `project`, named `name` (the command when empty).
    /// `NotFound` when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why when a field is malformed or the node
    /// refuses, such as a node from before background commands.
    202 StartTerminal { project: String, machine_id: String, name: String, command: String, cwd: String } => Call<Terminal>
    /// One terminal with the last of its output, at most 100 lines.
    /// `NotFound` when the caller has no such terminal.
    203 GetTerminal { id: Uuid } => Call<TerminalOutput>
    /// Stops a terminal's command when it still runs, then forgets the
    /// terminal. `NotFound` when the caller has no such terminal;
    /// `BadRequest` when its machine is online and refuses to stop it.
    204 CloseTerminal { id: Uuid } => Call<()>
    /// `project`'s (`org/repo`) branches on one machine with a checkout
    /// (the home checkout's when it is online, else another online one):
    /// local branches and those only on `origin`, newest commit first, at
    /// most 100, each with its worktree there, how far it is ahead of and
    /// behind the default branch, and its last commit. Read with git on
    /// each call, without fetching. `NotFound` when the caller has no such
    /// project; `BadRequest` saying why when no machine with a checkout is
    /// online or git fails.
    205 ListProjectBranches { project: String } => Call<ProjectBranches>
    /// Reviews `branch` of `project` against its default branch before a
    /// merge: fetches, reads the diff's stat, whether it merges cleanly
    /// and the branch's pull request with its checks, then starts one
    /// read-only coding-agent session per shard of the diff (by top-level
    /// area). `NotFound` when the caller has no such project; `BadRequest`
    /// saying why when no machine with a checkout and a coding agent is
    /// online, the branch is not found, it has no changes or an agent does
    /// not start.
    206 StartBranchReview { project: String, branch: String } => Call<BranchReview>
    /// One review as it stands. Asking moves it on: a reviewer that ended
    /// its turn gives its findings and a fresh verifier session checks
    /// them. `NotFound` when the caller has no such review.
    207 GetBranchReview { id: Uuid } => Call<BranchReview>
    /// `project`'s newest review of each branch, newest first, at most 20,
    /// as they stand without moving them on.
    208 ListBranchReviews { project: String } => Call<Vec<BranchReview>>
    /// Dismisses one finding of a review, by `ReviewFinding::id`, or brings
    /// it back. `NotFound` when the caller has no such review or finding.
    209 DismissReviewFinding { review: Uuid, finding: u32, dismissed: bool } => Call<BranchReview>
    /// Makes a project task with the review's open findings and starts a
    /// worker on it, in a worktree off the reviewed branch. `NotFound` when
    /// the caller has no such review; `BadRequest` when nothing is open or
    /// the review is still running.
    210 FixReviewFindings { review: Uuid } => Call<ProjectTask>
    /// Asks the project agent, in the project chat, to post the review's
    /// open findings on the branch's pull request; its host tools ask the
    /// user before anything posts. Gives the project chat's id. `NotFound`
    /// when the caller has no such review; `BadRequest` when the branch has
    /// no pull request or the review is still running.
    211 PostBranchReview { review: Uuid } => Call<Uuid>
    /// Asks the project agent, in the project chat, to merge the branch's
    /// pull request; the merge always asks the user first. Gives the
    /// project chat's id. `NotFound` when the caller has no such review;
    /// `BadRequest` saying why while the review runs, findings are open,
    /// checks fail, the branch conflicts or it has no pull request.
    212 MergeReviewedBranch { review: Uuid } => Call<Uuid>
    /// How the caller's coding agents sign in on their machines, for the
    /// Agent accounts section of Settings: the Claude Code token and Codex
    /// API key Beep keeps (never the secrets) and each machine that has
    /// Claude Code or Codex.
    213 GetAgentAccounts {} => Call<AgentAccounts>
    /// Saves the token `agent` (an `AgentAccount` slug: `claude` or
    /// `codex`) uses on the caller's machines, replacing the one before.
    /// The machines get it as the agent's env var when they start it.
    /// `BadRequest` for an unknown agent or a secret that is not a token.
    214 SaveAgentToken { agent: String, secret: SecretText } => Call<AgentToken>
    /// Removes `agent`'s token from Beep and the caller's machines; running
    /// sessions keep it until they end. `NotFound` when there is none.
    215 RevokeAgentToken { agent: String } => Call<()>
    /// Starts Codex's device sign-in (`codex login --device-auth`) on one
    /// of the caller's online machines and answers with the page and code
    /// it printed. `NotFound` when the machine is not one of the caller's
    /// online ones; `BadRequest` saying why when Codex cannot start it.
    216 StartCodexLogin { machine_id: String } => Call<CodexLogin>
    /// How a sign-in `StartCodexLogin` started stands.
    217 GetCodexLogin { machine_id: String, task: u32 } => Call<CodexLoginState>
    /// Stops a sign-in `StartCodexLogin` started, when it still waits.
    218 CancelCodexLogin { machine_id: String, task: u32 } => Call<()>
    /// The machine `StartAgentSession` without a `machine_id` would start
    /// these fields on now, and why, without starting anything: what the
    /// start form shows under Auto. `BadRequest` saying why when no machine
    /// fits, as the start would.
    219 PreviewAgentMachine { agent: Option<String>, cwd: Option<String>, prompt: String, needs: Vec<MachineCapability> } => Call<MachinePick>
    /// The caller's open pull requests and the open issues they opened on
    /// `project_id`'s GitHub repository, newest activity first, each saying
    /// whether someone else wrote after the caller's last comment or
    /// review. Read with `gh` on the project's home checkout (else a
    /// machine it was imported on) on each call. A project with no GitHub
    /// host gives an empty list. `NotFound` when the caller has no such
    /// project; `BadRequest` with the host's error when the machine is
    /// offline or the command fails.
    220 ListProjectThreads { project_id: Uuid } => Call<Vec<ProjectThread>>
    /// Asks the project agent, in the project chat, to read the pull
    /// request or issue `number` of `repo` and draft a reply; posting it
    /// always asks the user first. Gives the project chat's id. `NotFound`
    /// when the caller has no such project; `BadRequest` saying why when
    /// `repo` is not the project's or the thread cannot be read.
    221 ReplyToThread { project_id: Uuid, repo: String, number: u64 } => Call<Uuid>
    /// The chat about one pull request or issue of `project_id`, made on
    /// the first ask: the same chat on every call for one thread. It runs
    /// as the project agent, reading the thread again at the start of each
    /// turn. `NotFound` when the caller has no such project; `BadRequest`
    /// saying why when `repo` is not the project's or the thread cannot be
    /// read.
    222 OpenThreadChat { project_id: Uuid, repo: String, number: u64 } => Call<ThreadChat>
    /// The chats about one pull request or issue of `project_id`, newest
    /// first. `NotFound` when the caller has no such project.
    223 ListThreadChats { project_id: Uuid } => Call<Vec<ThreadChat>>
    /// Where a reviewed branch is now: fetches it on the review's machine
    /// and says whether it moved since the review (a new local commit, or
    /// a push the review did not read). Merging a stale review is refused.
    /// `NotFound` when the caller has no such review; `BadRequest` saying
    /// why when the machine is offline or git fails.
    240 CheckBranchReview { id: Uuid } => Call<ReviewHead>
    /// What importing `repo` (`org/repo` or its URL) would find, before
    /// `ImportProject`: the checkouts of it on the caller's online machines,
    /// found by any remote (`origin` or a fork's), and its branches with
    /// work in the first one: the heads of the caller's open pull requests,
    /// branches ahead of the default branch and those with a worktree, the
    /// default branch last. Read on each call. Without a checkout, only the
    /// pull requests, read with `gh` on an online machine. `BadRequest`
    /// saying why when `repo` is malformed or no machine is online.
    245 PreviewImport { repo: String } => Call<ImportPreview>
    /// Gives `branch` of `project` (`org/repo`) a worktree on the machine
    /// `machine_id`, at `.worktrees/<branch>` in its checkout there (a `/`
    /// in the name becomes `-`), fetching the branch from a remote that has
    /// it when it is not local. Gives the branch as it is after. `NotFound`
    /// when the caller has no such project; `BadRequest` saying why when
    /// the machine is offline or has no checkout of it, the branch already
    /// has a worktree or is not found, or git fails.
    246 AddBranchWorktree { project: String, branch: String, machine_id: String } => Call<ProjectBranch>
    /// Always empty now: the server keeps no images of coding agent
    /// sessions. Read each `[image: image-<key>]` line's picture with
    /// `GetAgentImage`. Kept for the apps that still ask.
    247 ListAgentImages { machine_id: String, session: String } => Call<Vec<AgentImage>>
    /// The caller's project groups, oldest first: folders the apps list
    /// projects under, in place of their org, on every machine.
    248 ListProjectGroups {} => Call<Vec<ProjectGroup>>
    /// Saves a project group: a new one without `id`, else the caller's
    /// group `id` with its new name and folders. Each folder is kept as
    /// `groups::project_group_path` gives it, so `~/Projects/work`
    /// and `/home/user/work` are one folder, and repeats are
    /// dropped. An empty `name` takes the first folder's. `BadRequest`
    /// saying why when there is no folder or more than 16, one is a home
    /// folder or the root, or a field is too long; `NotFound` when the
    /// caller has no group `id`.
    249 SaveProjectGroup { id: Option<Uuid>, name: String, paths: Vec<String> } => Call<ProjectGroup>
    /// Forgets one of the caller's project groups; its projects go back
    /// under their orgs. `NotFound` when the caller has no such group.
    250 DeleteProjectGroup { id: Uuid } => Call<()>
    /// Unlinks one of the caller's machines, by `Machine::id`, online or
    /// not: the server forgets it and stops syncing with it, drops its
    /// connection if it has one, and refuses its node until it is set up
    /// again. Any key of the account may remove a machine, where
    /// `RemoveDevice` needs the primary key. `NotFound` when no machine of
    /// the caller's has that id; `BadRequest` for the caller's own key,
    /// which `RemoveDevice` unlinks.
    251 RemoveMachine { machine_id: String } => Call<()>
    /// Removes `project` (`org/repo`) from Beep for the caller, deleting no
    /// files on any machine: its imports go, every machine stops syncing it
    /// (as `SetProjectSync` with `on` false does), its live workers and
    /// terminals are stopped, and its agent chat, thread chats, tasks,
    /// workers, branch reviews, pins and hidden issues are deleted. Issues
    /// and pull requests on its host stay as they are. Importing it again
    /// starts it afresh. `NotFound` when the caller has no such project.
    252 RemoveProject { project: String } => Call<()>
    /// The models the coding agent `agent` can run on `machine_id`, for
    /// `StartAgentSession`'s `model`, read from the node, which keeps them
    /// a few minutes. `AgentModelPick::Fixed` when its beep-node is too old
    /// to pick one. `NotFound` when the machine is not one of the caller's
    /// online ones; `BadRequest` saying why when it has no such agent or
    /// does not answer.
    253 ListAgentModels { machine_id: String, agent: String } => Call<AgentModels>
    /// The picture of a `[image: image-<key>]` line in the log of the
    /// coding agent session `session` on `machine_id`, read from the node
    /// on each call; the server keeps nothing. `NotFound` when the caller
    /// has no such online machine or the node no longer has the image;
    /// `BadRequest` saying why when the node cannot be asked.
    260 GetAgentImage { machine_id: String, session: String, key: String } => Call<AgentImageData>
    /// The caller's machines on which `project`'s folder is kept in step by
    /// Syncthing, as each machine last reported its Syncthing folders. Beep
    /// does not sync such a folder: `SetProjectSync` refuses it, and a
    /// machine already syncing it stops. Empty when there is none or the
    /// caller has not imported `project`.
    261 ListProjectSyncthing { project: String } => Call<Vec<SyncthingPlace>>
    /// Every open pull request on the project's issues host, anyone's,
    /// newest first: the 100 newest on GitHub, 25 on Forgejo. Empty when
    /// the project has no GitHub or Forgejo host; `BadRequest` saying why
    /// when no machine of the project can read it or the host refuses.
    262 ListProjectPullRequests { project_id: Uuid } => Call<Vec<ProjectPullRequest>>
    /// The branch the open pull request `number` of `project_id` is made
    /// from, read from its host now, for Continue. `NotFound` when the
    /// caller has no such project; `BadRequest` saying why when no machine
    /// of the project can read it, the host refuses, or there is no such
    /// pull request.
    299 GetPullRequestHead { project_id: Uuid, number: u64 } => Call<PullRequestHead>
    /// The runs going on in any of the caller's sessions, started from any
    /// of their devices, so a chat list can mark the chats still
    /// answering. Empty when none runs.
    285 ListActiveRuns {} => Call<Vec<Run>>
    /// One pull request or issue of `project_id` read from its host now:
    /// its opening post, its newest comments and reviews, and whether it
    /// waits on the caller's reply, for the card atop a chat about it.
    /// `NotFound` when the caller has no such project; `BadRequest` saying
    /// why when `repo` is not the project's or the thread cannot be read.
    286 GetProjectThread { project_id: Uuid, repo: String, number: u64 } => Call<ThreadView>
    /// The coding agent harnesses on the caller's machine `machine_id`, for
    /// the Coding agents page: what its node finds of each (version, the
    /// newest one, where it is, its sign-in) with the caller's settings for
    /// it. `refresh` looks the newest versions up again. `NotFound` when
    /// the machine is not one of the caller's online ones; `BadRequest`
    /// saying why when its beep-node is too old or does not answer.
    264 ListAgentHarnesses { machine_id: String, refresh: bool } => Call<AgentHarnesses>
    /// Saves the caller's settings for the harness `agent` on `machine_id`,
    /// replacing the ones before, and sends them to the machine when it is
    /// online. An env var whose `value` is `None` keeps the value saved
    /// before, and is dropped when there was none. A harness turned off is
    /// not offered on that machine, by Auto or by name. `NotFound` when the
    /// machine is not one of the caller's; `BadRequest` saying why for an
    /// unknown agent or a field that is too long or malformed.
    265 SaveHarnessSettings { machine_id: String, agent: String, settings: HarnessSettings } => Call<()>
    /// Installs the harness `agent` on `machine_id`, or updates it to the
    /// newest version, with the command its node names, and gives the
    /// harnesses as they stand after. It can take a few minutes. `NotFound`
    /// when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why when the node knows no way to install it or
    /// the command fails.
    266 UpdateAgentHarness { machine_id: String, agent: String } => Call<AgentHarnesses>
    /// How `project`'s sync stands on each machine that syncs it, in more
    /// detail than `ListProjectSync`: whether it is paused, what a machine
    /// still has to bring, how fast it moves and how long that takes. An
    /// offline machine shows `SyncStatus::Offline` and no progress.
    290 GetProjectSyncDetail { project: String } => Call<ProjectSyncDetail>
    /// Pauses `project`'s sync on every machine, or resumes it. A paused
    /// machine keeps its files and its list, and catches up when resumed;
    /// an older beep-node stops the project instead and scans it afresh
    /// when resumed. `BadRequest` when the project is pinned.
    291 PauseProjectSync { project: String, paused: bool } => Call<ProjectSyncDetail>
    /// Settles one conflict of `project` on `machine_id`: `Copy` keeps the
    /// conflict copy at `path` as the file it stands beside, `Original`
    /// keeps that file and deletes the copy. The machine changes its folder
    /// and syncs the result to the others. `NotFound` when the machine does
    /// not sync the project or is offline; `BadRequest` saying why when
    /// its beep-node is too old, sync is paused, or `path` is not a
    /// conflict copy there.
    292 ResolveSyncConflict { project: String, machine_id: String, path: String, keep: ConflictKeep } => Call<()>
    /// Starts an interactive terminal of `project` (none when empty) on one
    /// of the caller's online machines: `command` in `cwd` (the machine's
    /// workspace, else its home folder, when empty), or the user's shell
    /// there when `command` is empty, in a pseudo-terminal of `cols` by `rows` cells,
    /// named `name` (the command, or "shell", when empty). `AttachTerminal`
    /// follows it and types into it; `CloseTerminal` stops it. `NotFound`
    /// when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why when a field is malformed or the node
    /// refuses, such as a node from before interactive terminals.
    272 OpenTerminal { project: String, machine_id: String, name: String, command: String, cwd: String, cols: u32, rows: u32 } => Call<Terminal>
    /// Follows a terminal live from byte `from` of its output, at `cols` by
    /// `rows` cells: `TerminalEvent::Output` as it comes, until it ends or
    /// the client finishes its side. After the request the client sends
    /// `TerminalInput`s (see `Duplex`). A terminal `StartTerminal` started
    /// cannot be typed into: it answers `ViewOnly` and ends. `NotFound`
    /// when the caller has no such terminal.
    273 AttachTerminal { id: Uuid, from: u64, cols: u32, rows: u32 } => Stream<TerminalEvent>
    /// Every terminal of the caller on every machine, of any project or of
    /// none (`project` empty), oldest first, with how each stands now.
    /// Terminals of an offline machine are `Offline`.
    293 ListAllTerminals {} => Call<Vec<Terminal>>
    /// What the caller's coding agents cost today and this week, by
    /// project, and how much of each agent's plan is left, for the pill
    /// atop the command center. The plans are read on the caller's
    /// machines every few minutes; `refresh` asks the online ones to read
    /// them again now, and the answer comes before they did.
    294 GetUsage { refresh: bool } => Call<UsageView>
    /// Whether importing `repo` (`org/repo` or its URL) clones the
    /// caller's fork of it rather than the repository itself: their fork on
    /// the same host, found with their token there, and the head branches
    /// of their open pull requests from it, which the import makes local
    /// branches. The import adds the repository as the remote `upstream`.
    /// `fork` is `None` when they have no fork, no token for the host, or
    /// the host could not be read. Read on each call. `BadRequest` saying
    /// why when `repo` is malformed.
    295 FindImportFork { repo: String } => Call<ImportFork>
    /// Part of a media file on the caller's online machine `machine_id`,
    /// such as a video, 3D model, PDF or picture a coding agent made.
    /// `path` is absolute or starts `~/` (the node's home). Gives up to
    /// [`AGENT_FILE_CHUNK`] bytes from `offset`, with the file's whole size
    /// and MIME type; the app asks again from `offset` plus the bytes it
    /// got until it has `size`. Only files whose extension [`media_mime`]
    /// knows and of at most [`MAX_AGENT_FILE`] bytes. The server keeps
    /// nothing. `NotFound` when the machine is not one of the caller's
    /// online ones or the file is not there; `BadRequest` saying why for
    /// another type, a file too large, or a node too old to read files
    /// (updating beep-node may help).
    300 GetAgentFile { machine_id: String, path: String, offset: u64 } => Call<AgentFileChunk>
    /// One folder of a coding agent session's working folder `root` on the
    /// caller's online machine `machine_id`, for the session's Files tab.
    /// `root` is absolute or starts `~/`; `path` is the folder inside it,
    /// relative with `/` between names, `""` for `root` itself. Leaves out
    /// what git ignores unless `ignored`, and `.git` always; marks what git
    /// sees changed. `BadRequest` saying why for a path outside `root` (a
    /// symlink out of it included), a folder that is not there, or a node
    /// too old to list folders (updating beep-node may help); `NotFound`
    /// when the machine is not one of the caller's online ones.
    301 ListWorkFolder { machine_id: String, root: String, path: String, ignored: bool } => Call<WorkFolder>
    /// Up to `len` bytes, at most [`AGENT_FILE_CHUNK`], of the file `path`
    /// inside `root` from `offset`, as `ListWorkFolder` names them: any
    /// kind of file, for the Files tab to show or save. `mime` is
    /// [`media_mime`]'s, else `application/octet-stream`. `NotFound` when
    /// the file or the machine is not there; `BadRequest` as for
    /// `ListWorkFolder`.
    302 ReadWorkFile { machine_id: String, root: String, path: String, offset: u64, len: u32 } => Call<AgentFileChunk>
    /// Writes part of a file into `root`, for files dropped on a session.
    /// `path` is where it goes inside `root`; the first part (`offset` 0)
    /// makes the folders on the way and takes a free name beside the one
    /// asked (`notes 2.txt`), and the reply is the path it took, which the
    /// later parts send. Parts come in order, each at most
    /// [`AGENT_FILE_CHUNK`], until `offset` plus the part is `size`, at
    /// most [`MAX_WORK_UPLOAD`]; only then does the file show under its
    /// name. Never replaces a file. `BadRequest` saying why for a path
    /// outside `root`, a part out of order, a file too large, or a node too
    /// old to take files.
    303 WriteWorkFile { machine_id: String, root: String, path: String, offset: u64, size: u64, #[serde(with = "serde_bytes")] data: Vec<u8> } => Call<String>
    /// What the caller's coding agents cost today, this week (from Monday)
    /// and this month, in the caller's timezone, by project and by
    /// harness, and how much of each agent's plan is used. Unlike
    /// `GetUsage` it counts sessions started outside Beep too, as the
    /// machines read them from the agents' own logs. `refresh` as in
    /// `GetUsage`.
    304 GetUsageReport { refresh: bool } => Call<UsageReport>
    /// The chat about the update `id`, as `ListUpdates` gave it, made on
    /// the first ask and the same chat after. A new chat opens with Beep
    /// telling the user the update, so they can reply; Beep answers with
    /// the update's events in its context. Opening it dismisses the update.
    /// `NotFound` when the caller has no such update, or it is gone.
    305 OpenUpdateChat { id: i64 } => Call<Session>
    /// The custom agents the coding agent harnesses define for the project
    /// folder `folder` on the caller's online machine `machine_id`, then
    /// the user's own there: Claude Code's `.claude/agents`, OpenCode's
    /// `.opencode/agent` and `opencode.json`, Codex's `.codex/agents`,
    /// pi's `.pi/agents` and flsh's `.agents`, each in the folder and in
    /// the harness's folder in the node's home. `folder` is absolute or
    /// starts `~/`. A file that does not read gives a row with `error`
    /// set. `NotFound` when the machine is not one of the caller's online
    /// ones; `BadRequest` saying why when the folder is not there or the
    /// node is too old to read custom agents (updating beep-node may help).
    306 ListCustomAgents { machine_id: String, folder: String } => Call<Vec<CustomAgent>>
    /// The open pull requests and the open issues no task tracks of
    /// `project_id`, as `ListProjectPullRequests` and `ListProjectIssues`
    /// give them, from the server's copy and with when it was read. The
    /// server reads the host on the first ask, and every few minutes while
    /// someone looks at the project. A copy a few minutes old is given as
    /// it is and read again in the background; `WatchProject` says when. `refresh` reads it now, at most once per 30 seconds per
    /// project; sooner, the copy is given as it is. A host that could not
    /// be read gives its error and the lists read before. `NotFound` when
    /// the caller has no such project.
    307 GetProjectLists { project_id: Uuid, refresh: bool } => Call<ProjectLists>
    /// The newest workflow runs (GitHub Actions or Forgejo Actions) of
    /// `project_id`'s repository, newest first, with how far a running one
    /// got and the checks of each open pull request. Read from the server's
    /// copy as `GetProjectLists` keeps it; while a run is in progress and
    /// someone asks, its jobs are read again at most every 10 seconds.
    /// `refresh` as in `GetProjectLists`. A host that cannot be read, or
    /// that limits the caller's token, says so in the reply. `NotFound`
    /// when the caller has no such project.
    308 ListProjectRuns { project_id: Uuid, refresh: bool } => Call<ProjectRuns>
    /// One workflow run of `project_id` with its jobs and steps and, when
    /// it failed, the end of its failing job's log. Read as
    /// `ListProjectRuns` reads jobs. `NotFound` when the caller has no such
    /// project; `BadRequest` saying why when the host cannot be read or has
    /// no such run.
    309 GetProjectRun { project_id: Uuid, run_id: u64 } => Call<ProjectRunDetail>
    /// As `GetUsageReport`, with what a subscription covered apart: dollars
    /// count only metered API spend (API keys, OpenRouter, Bedrock and the
    /// like), and use a Claude, ChatGPT or Copilot plan covered counts as
    /// tokens. A machine that cannot tell counts Claude Code and Codex as
    /// subscription, the other harnesses as metered.
    310 GetSpendReport { refresh: bool } => Call<SpendReport>
    /// Follows `project_id` while its page is open: the first reply says
    /// whether its host's events reach Beep now, and each later one comes
    /// when the server read its lists or runs again, or when that changed.
    /// While someone follows a project whose repository the user is an
    /// admin of, one of their machines forwards its webhook. The page
    /// keeps reading the lists every few minutes as before. `NotFound`
    /// when the caller has no such project.
    340 WatchProject { project_id: Uuid } => Stream<ProjectChange>
    /// As `ListProjectRuns`, from every host `project_id`'s repository is
    /// on: its GitHub and Forgejo hosts, from its roles, its imports and
    /// its checkout's remotes. One push to a repository mirrored on both
    /// runs on both, so the runs of each are merged newest first and each
    /// names its host. A host that cannot be read gives its own error in
    /// `HostedRuns::hosts` and the others still show. `ListProjectRuns`
    /// gives the primary host's alone. `NotFound` when the caller has no
    /// such project.
    360 ListProjectRunsAll { project_id: Uuid, refresh: bool } => Call<HostedRuns>
    /// As `GetProjectRun`, for the run `run_id` on `host`, as
    /// `HostedRun::host` names it. `BadRequest` saying why when the
    /// project's repository is not on that host.
    361 GetProjectRunOn { project_id: Uuid, host: String, run_id: u64 } => Call<ProjectRunDetail>
    /// The caller's flsh settings (provider routing and compaction), the
    /// same on every machine. All unset until they save some.
    370 GetFlshSettings {} => Call<FlshSettings>
    /// Saves the caller's flsh settings, replacing the ones before, and
    /// sends them to every machine of theirs online; each writes them to
    /// flsh's config before it starts flsh. `BadRequest` saying why for a
    /// value flsh would not take (`FlshSettings::check`).
    371 SaveFlshSettings { settings: FlshSettings } => Call<()>
    /// The caller's settings for the harness `agent` on each of their
    /// machines, online or not, so the Coding agents page can say which
    /// fields differ between them. A machine with none saved gives the
    /// defaults. Env var values never come back.
    372 ListHarnessSettings { agent: String } => Call<Vec<MachineHarnessSettings>>
    /// Saves `settings` for the harness `agent` on every machine of the
    /// caller's, online or not: online ones get them now, the others when
    /// they next connect. Each machine keeps whether the harness is on, and
    /// its own binary path unless `include_program`. An env var whose
    /// `value` is `None` takes the value saved on `from_machine`; each
    /// machine's copy is sealed for that machine. Replies how many machines
    /// were saved. `NotFound` when `from_machine` is not one of the
    /// caller's; `BadRequest` as `SaveHarnessSettings`.
    373 SaveHarnessSettingsAll { from_machine: String, agent: String, settings: HarnessSettings, include_program: bool } => Call<u32>
    /// What changed in the git checkout `root` on the caller's online
    /// machine `machine_id`, for a coding agent session's Changes tab: the
    /// diff from where `head` left `base` to `head`. `base` is a branch,
    /// `""` for the checkout's default (`origin/HEAD`, else `main` or
    /// `master`); `head` is `""` for the working tree, so uncommitted
    /// changes and untracked files count, else a branch: its worktree
    /// inside `root` read the same way when it has one, else its commits.
    /// Each file gives at most [`MAX_DIFF_FILE_LINES`] lines and all of
    /// them at most [`MAX_DIFF_LINES`]; past that, and for lockfiles
    /// unless `lockfiles`, a file comes without its lines and says why.
    /// `only` names files to give in full past those caps, up to
    /// [`MAX_DIFF_SHOWN_LINES`] each, and then the reply has only them.
    /// `root` is reached as `ListCustomAgents` reaches its folder.
    /// `NotFound` when the machine is not one of the caller's online ones;
    /// `BadRequest` saying why for a folder outside the node's reach, one
    /// not in a checkout, a branch git does not know, or a node too old to
    /// read diffs (updating beep-node may help).
    380 GetWorkDiff { machine_id: String, root: String, base: String, head: String, only: Vec<String>, lockfiles: bool } => Call<WorkDiff>
    /// As `GetWorkDiff`, for `branch` of `project` (`org/repo`) against
    /// its default branch, read on a machine with a checkout as
    /// `ListProjectBranches` picks it. `NotFound` when the caller has no
    /// such project; `BadRequest` saying why when no machine with a
    /// checkout is online or the node refuses.
    381 GetBranchDiff { project: String, branch: String, only: Vec<String>, lockfiles: bool } => Call<WorkDiff>
    /// Checks the caller's saved Forgejo or Gitea token `id` (a
    /// `Credential::id`) from where its server can be reached: Beep's
    /// server when it can reach it, else the caller's online machines with
    /// teacli, which hold the token in teacli's config. A login found is
    /// kept as the credential's `account`. `NotFound` when the caller has
    /// no such Forgejo credential.
    382 CheckCredential { id: String } => Call<CredentialCheck>
    /// `path` inside the working folder `root` and the folders under it,
    /// `depth` levels down in all (1 is `path` alone, at most
    /// [`MAX_TREE_DEPTH`]), as `ListWorkFolder` lists each, in one ask.
    /// Folders git ignores are listed but not read into, and the walk stops
    /// past [`MAX_TREE_ENTRIES`] entries. A node from before it gives
    /// `path` alone. Errors as for `ListWorkFolder`.
    383 ListWorkTree { machine_id: String, root: String, path: String, ignored: bool, depth: u32 } => Call<WorkTree>
    /// Has one of the caller's online machines install the newest
    /// beep-node over itself and restart, as `beep-node update` does
    /// there. It holds new prompts and restarts once its agent sessions
    /// are idle, or after a wait; `stop_sessions` restarts at once. The
    /// sessions resume after the restart. The reply comes once it is
    /// installed, before the restart. `NotFound` when the machine is not
    /// the caller's; `BadRequest` saying why when it is offline, too old to
    /// be updated this way, or the update failed.
    390 UpdateMachine { machine_id: String, stop_sessions: bool } => Call<MachineUpdate>
    /// The caller's online machines that `UpdateMachine` can update. A
    /// machine missing here is updated on the machine.
    391 ListUpdatableMachines {} => Call<Vec<UpdatableMachine>>
    /// Pull request or issue `number` of `project_id` read from its GitHub
    /// or Forgejo host now, for the project page: its state, labels, opening
    /// post and comments, and for a pull request its reviews and checks.
    /// `NotFound` when the caller has no such project; `BadRequest` saying
    /// why when the project has no host or the host cannot be read.
    392 GetThreadDetail { project_id: Uuid, number: u64 } => Call<ThreadDetail>
    /// The caller's repositories to clone, for the Add a project sheet: one
    /// list per git host, GitHub and each Forgejo server in their
    /// credentials, newest update first, at most 100 each. Archived ones
    /// and those the caller imported are left out. Read with `gh` and
    /// `teacli` on an online machine that has them, kept for a few
    /// minutes. A host that cannot be read is listed with why; GitHub is
    /// left out when no machine has `gh`.
    393 ListHostRepos {} => Call<Vec<HostRepos>>
    /// The tools on the caller's online machines that have a newer
    /// release `UpdateMachineTool` can install (`gh`, `teacli`), checked
    /// against the newest release every few hours.
    400 ListToolUpdates {} => Call<Vec<ToolUpdate>>
    /// Has one of the caller's online machines install the newest release
    /// of `tool` into beep-node's own tools folder, which comes first on
    /// the `PATH` of what Beep runs there. Gives the version installed.
    /// `NotFound` when the machine is not the caller's; `BadRequest`
    /// saying why when it is offline, too old, or the install failed.
    401 UpdateMachineTool { machine_id: String, tool: String } => Call<String>
    /// The hosts the ssh of `machine_id`, one of the caller's online
    /// machines, knows: `Host` names in its `~/.ssh/config` without
    /// wildcards, then unhashed names in `~/.ssh/known_hosts`. `NotFound`
    /// when the machine is not the caller's; `BadRequest` saying why when it
    /// is offline or its beep-node is too old.
    402 ListSshHosts { machine_id: String } => Call<Vec<String>>
    /// Has `machine_id`, one of the caller's online machines, add `target`
    /// (`user@host` or an ssh config alias) over its own ssh: it connects
    /// without prompting, installs beep-node there and links it to the
    /// caller with a new setup token. The reply comes once it is online or
    /// a step failed. `search` is where the new machine looks for git
    /// checkouts (`beep-node setup --search`); empty for home. Errors as for
    /// `ListSshHosts`.
    403 AddMachineOverSsh { machine_id: String, target: String, #[serde(default)] search: String } => Call<SshAdd>
    /// Where `machine_id`, one of the caller's online machines, looks for
    /// git checkouts, as written (`~/` for home). Errors as for
    /// `ListSshHosts`.
    404 GetMachineSearch { machine_id: String } => Call<String>
    /// Has `machine_id` look for git checkouts in `search` (empty for home)
    /// from now on, and search there again. Gives the folder as
    /// `GetMachineSearch` does; `BadRequest` saying why when the machine has
    /// no such folder. Errors as for `ListSshHosts`.
    405 SetMachineSearch { machine_id: String, search: String } => Call<String>
    /// `GetProjectLists` with the labels of each pull request. Errors as
    /// for `GetProjectLists`.
    406 GetLabeledProjectLists { project_id: Uuid, refresh: bool } => Call<LabeledProjectLists>
    /// `ListGitItems` with the labels of each item.
    407 ListLabeledGitItems {} => Call<LabeledGitItems>
    /// What a session Beep runs has used: how full its context is, and the
    /// tokens and cost of its finished turns. Empty from a node too old to
    /// say. `NotFound` as for `GetAgentSession`, and for a session started
    /// outside Beep.
    408 GetAgentSessionUsage { machine_id: String, agent: String, session: String } => Call<AgentSessionUsage>
    /// What each of the caller's machines moved over iroh since it first
    /// reported, online or not. A machine that never reported is left out.
    409 ListMachineTraffic {} => Call<Vec<MachineTraffic>>
    /// Copies `project` onto `machine_id` from a machine that has it and keeps
    /// them in sync; a machine that holds it already keeps what it holds.
    /// `BadRequest` says why it cannot.
    410 CopyProject { project: String, machine_id: String } => Call<ProjectCopy>
    /// The last `lines` lines of the beep-node log of `machine_id`, one of the
    /// caller's online machines, oldest first, at most 1000 lines and 256 KB.
    /// `NotFound` when the machine is not the caller's; `BadRequest` saying
    /// why when it is offline, too old or has no log to read.
    411 GetMachineLog { machine_id: String, lines: u32 } => Call<String>
    /// The git hosts the caller has a token for, for the New project sheet,
    /// with who a repository there can belong to. A host that cannot answer
    /// says why in its `error`.
    412 ListProjectForges {} => Call<Vec<ProjectForge>>
    /// Makes a project called `name`. With `repository`, creates it on that
    /// git host with the caller's token and clones it onto each of
    /// `machine_ids`, which then sync it; a machine that fails is listed in
    /// `failed` and the repository stays. Without, runs `git init` on the one
    /// machine given and imports it there. `BadRequest` saying why when nothing
    /// was made: the name is taken or not valid, the token may not create
    /// there, or a machine is offline.
    413 CreateProject { name: String, repository: Option<NewRepository>, machine_ids: Vec<String> } => Call<CreatedProject>
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_from_an_app_without_platform_decodes() {
        let old = (
            Uuid::nil(),
            Uuid::nil(),
            "hi",
            Vec::<String>::new(),
            None::<String>,
            None::<ReasoningLevel>,
        );
        let buf = rmp_serde::to_vec(&old).unwrap();
        assert_eq!(rmp_serde::from_slice::<Chat>(&buf).unwrap().platform, None);
    }

    #[test]
    fn a_group_message_from_before_images_still_decodes() {
        #[derive(Serialize)]
        struct Old {
            group_id: String,
            request_id: Uuid,
            message: String,
        }
        let old = Old {
            group_id: "g1".into(),
            request_id: Uuid::new_v4(),
            message: "hi".into(),
        };
        let buf = rmp_serde::to_vec(&old).unwrap();
        let decoded: SendToGroup = rmp_serde::from_slice(&buf).unwrap();
        assert_eq!(decoded.message, "hi");
        assert!(decoded.images.is_empty());
    }

    impl Feature {
        const LATER: Feature = Feature(u32::MAX);
    }

    wire_enum! {
        #[derive(Debug, Clone, PartialEq)]
        pub enum Shape {
            0 Old { size: u32 }
            1 New {} since LATER
        }
    }

    #[test]
    fn a_gated_variant_is_left_out_of_older_samples() {
        use compat::Sample;
        assert_eq!(Shape::samples(1), [Shape::Old { size: 42 }]);
        assert_eq!(Shape::samples(u32::MAX).len(), 2);
    }

    #[test]
    fn a_request_skips_fields_a_newer_app_added() {
        // GetRun with a second field this build does not know.
        let buf = [0x92, 0xa2, b'r', b'1', 0x91, 0x07];
        assert_eq!(
            rmp_serde::from_slice::<GetRun>(&buf).unwrap(),
            GetRun { id: "r1".into() }
        );
        // A request with no fields yet, given one.
        assert!(rmp_serde::from_slice::<Health>(&[0x91, 0xc0]).is_ok());
    }
}
