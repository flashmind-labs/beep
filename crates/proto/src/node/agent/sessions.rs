//! The `agents/1` protocol: one request, one reply, about all the sessions,
//! and the names of the protocols each agent speaks.

use super::{
    Activity, Entry, ExternalSession, LiveStatus, ModelList, Question, SessionFolder, SessionInfo,
    Steer, StopReason,
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};

/// What the server sends on an `agents/1` stream: one request, one reply.
/// A node older than a request closes the stream or answers `Error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionsRequest {
    List {},
    /// The last `last` entries of a session's transcript. `session` may
    /// also be an external session's id.
    Transcript {
        session: String,
        last: u32,
        /// Keep the agent's thoughts, as `EntryKind::Thought` entries. A
        /// server from before them leaves it out and gets none, so it
        /// never shows a thought as an unknown entry.
        #[serde(default)]
        thoughts: bool,
        /// Read the `last` entries before the entry with this index
        /// (counting from the first the node still has, thoughts
        /// included), as the reply's `earlier` counts them, instead of the
        /// last ones. A node from before it ignores this and answers with
        /// the last entries, which an app tells by `earlier` plus the
        /// entries not ending at `before`.
        #[serde(default)]
        before: Option<u64>,
    },
    /// Sessions of `agent` started outside Beep, newest first: at most
    /// `limit`, or only `session` when given.
    External {
        agent: String,
        #[serde(default)]
        limit: u32,
        #[serde(default)]
        session: Option<String>,
    },
    /// Follows these external sessions: `Watching` first, then `Activity`
    /// as they finish turns or wait for the user, until the server closes
    /// the stream.
    Watch {
        sessions: Vec<String>,
    },
    /// The git checkouts coding agents worked in, from the sessions Claude
    /// Code, Codex, pi, flsh and OpenCode keep: at most `limit`, newest
    /// activity first. Answered with `Folders`.
    Folders {
        #[serde(default)]
        limit: u32,
    },
    /// The sessions those agents ran in any of `folders` (or a subfolder or
    /// worktree of one), newest first, at most `limit`. Answered with
    /// `External`, `title` falling back to the first prompt.
    InFolders {
        folders: Vec<String>,
        #[serde(default)]
        limit: u32,
    },
    /// The picture of the `[image: image-<key>]` line in `session`'s
    /// transcript, `key` being its `image_key`. Answered with `Image`, or
    /// `Error` when the node no longer has it.
    Image {
        session: String,
        key: String,
    },
    /// The models the agent `agent` can run on here, for `Start`'s `model`.
    /// Answered with `Models`, or `Error` for an agent the node does not have.
    Models {
        agent: String,
    },
    /// What the node finds of each coding agent harness it knows, answered with
    /// `Harnesses`. `refresh` looks the newest versions up again instead of
    /// using the ones found lately.
    Harnesses {
        #[serde(default)]
        refresh: bool,
    },
    /// Installs the harness `agent`, or updates it to the newest version,
    /// with the command its `HarnessInfo::install` names, then answers
    /// `Harnesses` as they stand after, or `Error` saying why it failed.
    UpdateHarness {
        agent: String,
    },
    /// Sends each picture a running session's transcript newly shows, as
    /// `SessionImage` replies, until the server closes the stream; the server
    /// keeps them so they show while the node is offline. Pictures that came
    /// while no stream was open wait, a bounded few.
    FollowImages {},
    /// Sends what each finished turn spent, as `TurnUsage` replies, and how
    /// much of each agent's plan is left, as `PlanUsage` replies (all of them
    /// first, then each time one changes), until the server closes the stream.
    /// Turns that ended while no stream was open wait, a bounded few. On this
    /// stream the server may later send `RefreshPlanUsage`; anything else is
    /// its goodbye.
    FollowUsage {
        /// Also sends what sessions spent as the agents' own logs on the
        /// node tell it, as `SessionSpend` replies: the last weeks first,
        /// then each session again as it changes. A node from before it
        /// sends none.
        #[serde(default)]
        history: bool,
    },
    /// Sent on a `FollowUsage` stream: reads the plans' usage again now,
    /// rather than at the next check.
    RefreshPlanUsage {},
    /// Up to `AGENT_FILE_CHUNK` bytes from `offset` of the media file `path`,
    /// absolute or starting `~/` for the node's home. Answered with `File`,
    /// `NoFile` when there is no such file, or `Error` saying why it cannot be
    /// read.
    File {
        path: String,
        offset: u64,
    },
    /// The folder `path` inside the working folder `root`, as `ListWorkFolder`
    /// asks it. Answered with `Folder`, or `Error` saying why, for a path
    /// outside `root` among others.
    Folder {
        root: String,
        path: String,
        ignored: bool,
        /// Above 0, the folders under it too, as `ListWorkTree` asks them,
        /// answered with `Tree`. A node from before it answers `Folder`.
        #[serde(default)]
        depth: u32,
    },
    /// Up to `len` bytes of the file `path` inside `root` from `offset`, as
    /// `ReadWorkFile` asks them. Answered with `File`, `NoFile` or `Error`.
    ReadFile {
        root: String,
        path: String,
        offset: u64,
        len: u32,
    },
    /// Part of a file written into `root`, as `WriteWorkFile` sends it.
    /// Answered with `Written`, or `Error` saying why.
    WriteFile {
        root: String,
        path: String,
        offset: u64,
        size: u64,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// The custom agents the harnesses define for the project folder `folder`
    /// and in the node's home, as `ListCustomAgents` asks them. Answered with
    /// `CustomAgents`, or `Error` saying why.
    CustomAgents {
        folder: String,
    },
    /// Sends what the user would want to hear about the node's sessions, as
    /// `Alert` replies, until the server closes the stream: a question a
    /// session asks, a turn that ended with no prompt queued behind it, and a
    /// session that failed. Alerts that came while no stream was open wait, a
    /// bounded few.
    FollowAlerts {},
    /// What changed in the checkout `ask.root`, as `GetWorkDiff` asks it.
    /// Answered with `Diff`, or `Error` saying why.
    Diff {
        ask: DiffAsk,
    },
    /// How Claude Code session `session`, started outside Beep, is doing now,
    /// as its hooks report it. Answered with `Live`, or `Error`.
    Live {
        session: String,
    },
    /// Queues `steer` on the node for the next hook of `session` to deliver: a
    /// message after the current tool call or at the turn's end, a stop at the
    /// next tool call. Answered with `Live`, or `Error` saying why it cannot,
    /// such as a session idle at its prompt.
    Steer {
        session: String,
        steer: Steer,
    },
    /// The worktrees of the git repository holding the session folder
    /// `folder`, as `git worktree list` gives them. Answered with
    /// `Worktrees` (none when `folder` is not in a repository), or `Error`
    /// saying why. A node from before it answers `Error` or closes the
    /// stream.
    Worktrees {
        folder: String,
    },
    /// Deletes the ended session `session` of the harness `agent` from this
    /// machine: its transcript file and the node's record of it. `session`
    /// is the agent's own id or the node's. Answered with `Deleted`, or
    /// `Error` saying why not, such as a session that still runs. A node
    /// from before it answers `Error` or closes the stream.
    DeleteSession {
        agent: String,
        session: String,
    },
    /// The slash commands and skills the agent of the running session
    /// `session` offers, as it last reported them (the list arrives after
    /// the session starts and can change, so it is asked for when needed).
    /// Answered with `Commands`, empty for a session that is not running,
    /// or `Error`. A node from before it answers `Error` or closes the
    /// stream.
    Commands {
        session: String,
    },
    /// How the move of session `session` to this node stands, as the target
    /// says. Answered with `Moving`, or `Error`. A node from before it
    /// answers `Error` or closes the stream: it cannot take a session.
    MoveProgress {
        session: String,
    },
    /// Whether this node can take a session that works in a worktree of
    /// `project`, on `branch`: it makes the worktree itself, and says if
    /// its Syncthing is set not to sync that folder. Answered with
    /// `MoveChecked`; a node from before it closes the stream, and takes
    /// checkout sessions only.
    MoveCheck {
        project: String,
        branch: String,
    },
    /// The copies of other machines' sessions this node keeps, and the
    /// versions that lost to another machine's. Answered with `Copies`; a
    /// node from before background copies closes the stream.
    Copies {},
    /// Runs the copy of session `session` of `agent` here, in the project's
    /// checkout on this node; with `lost`, the version set aside at that
    /// time instead. Answered with `CopyDone`, or `Error` saying why not.
    ContinueCopy {
        agent: String,
        session: String,
        lost: Option<u64>,
    },
    /// Throws away the copy of a session, or its version set aside at
    /// `lost`. Answered with `CopyDone`.
    DiscardCopy {
        agent: String,
        session: String,
        lost: Option<u64>,
    },
    /// `Image` made small on the node, so a remote log's picture does not
    /// cross the network whole. Answered as `Image` is; a node from before
    /// it closes the stream.
    Thumb {
        session: String,
        key: String,
    },
    /// Sends the CI runs the user started that ended, as `Run` replies, until
    /// the caller closes the stream: on the node that watches each project. A
    /// node from before it closes the stream.
    FollowRuns {},
}

impl SessionsRequest {
    /// A one-shot read that changes nothing on the node, so asking twice
    /// is safe: one lost with its connection may be sent again on the
    /// next. Streams that follow the node and anything that writes,
    /// installs or steers are not.
    pub fn is_read(&self) -> bool {
        match self {
            Self::List {}
            | Self::Transcript { .. }
            | Self::External { .. }
            | Self::Folders { .. }
            | Self::InFolders { .. }
            | Self::Image { .. }
            | Self::Thumb { .. }
            | Self::Models { .. }
            | Self::Harnesses { .. }
            | Self::File { .. }
            | Self::Folder { .. }
            | Self::ReadFile { .. }
            | Self::CustomAgents { .. }
            | Self::Diff { .. }
            | Self::Worktrees { .. }
            | Self::Commands { .. }
            | Self::MoveProgress { .. }
            | Self::MoveCheck { .. }
            | Self::Copies {}
            | Self::Live { .. } => true,
            Self::Watch { .. }
            | Self::Steer { .. }
            | Self::UpdateHarness { .. }
            | Self::FollowImages {}
            | Self::FollowUsage { .. }
            | Self::RefreshPlanUsage {}
            | Self::WriteFile { .. }
            | Self::DeleteSession { .. }
            | Self::ContinueCopy { .. }
            | Self::DiscardCopy { .. }
            | Self::FollowAlerts {}
            | Self::FollowRuns {} => false,
        }
    }
}

/// What `SessionsRequest::Diff` reads, as `GetWorkDiff` names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffAsk {
    /// The folder in the checkout, absolute or starting `~/`.
    pub root: String,
    /// The branch to compare against; empty for the checkout's default.
    pub base: String,
    /// Empty for the working tree, else a branch.
    pub head: String,
    /// Files to give in full, and only them; empty for every file.
    pub only: Vec<String>,
    /// Lockfiles come with their lines.
    pub lockfiles: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionsReply {
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    Transcript {
        entries: Vec<Entry>,
        /// Entries before these ones.
        earlier: u32,
    },
    Error {
        message: String,
    },
    External {
        sessions: Vec<ExternalSession>,
    },
    /// The answer to `Watch`: the sessions it could not find.
    Watching {
        unknown: Vec<String>,
    },
    Activity {
        activity: Activity,
    },
    Folders {
        folders: Vec<SessionFolder>,
    },
    /// The answer to `Image`.
    Image {
        mime: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// The answer to `Models`.
    Models {
        models: ModelList,
    },
    Harnesses {
        report: crate::node::harness::HarnessReport,
    },
    /// One picture on a `FollowImages` stream: of `session`, shown in its
    /// transcript by the line of its `image_key`.
    SessionImage {
        session: String,
        mime: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// One finished turn on a `FollowUsage` stream.
    TurnUsage {
        usage: crate::node::usage::TurnUsage,
    },
    /// One agent's plan usage on a `FollowUsage` stream.
    PlanUsage {
        usage: crate::node::usage::PlanUsage,
    },
    /// What one session spent, from the agent's own log, on a
    /// `FollowUsage` stream that asked for `history`.
    SessionSpend {
        usage: crate::node::usage::SessionSpend,
    },
    /// The answer to `File`: the whole file's `size` and its MIME type,
    /// with the bytes from the asked offset, empty at or past the end.
    File {
        size: u64,
        mime: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// The answer to `File` when there is no file at its path.
    NoFile {},
    /// The answer to `Folder`.
    Folder {
        folder: crate::WorkFolder,
    },
    /// The answer to `Folder` with a `depth`.
    Tree {
        tree: crate::WorkTree,
    },
    /// The answer to `WriteFile`: the path inside the root it writes to.
    Written {
        path: String,
    },
    /// The answer to `CustomAgents`.
    CustomAgents {
        agents: Vec<crate::CustomAgent>,
    },
    /// One alert on a `FollowAlerts` stream.
    Alert {
        alert: Alert,
    },
    /// The answer to `Diff`.
    Diff {
        diff: crate::WorkDiff,
    },
    /// The answer to `Live` and `Steer`.
    Live {
        status: Box<LiveStatus>,
    },
    /// The answer to `Worktrees`.
    Worktrees {
        worktrees: Vec<Worktree>,
    },
    /// The answer to `DeleteSession`.
    Deleted {},
    /// The answer to `Commands`.
    Commands {
        commands: Vec<AgentCommand>,
    },
    /// The answer to `MoveProgress`: `None` when no move of that session
    /// to this node is known.
    Moving {
        progress: Option<MoveProgress>,
    },
    /// The answer to `MoveCheck`.
    MoveChecked {
        /// The worktree's folder is one Syncthing keeps in step here, and
        /// its `.stignore` leaves it out: the session cannot move.
        ignored: bool,
    },
    /// The answer to `Copies`.
    Copies {
        copies: Vec<CopyView>,
    },
    /// The answer to `ContinueCopy` and `DiscardCopy`.
    CopyDone {},
    /// One run on a `FollowRuns` stream.
    Run {
        news: RunNews,
    },
}

/// A CI run the user started (a push or a pull request) that ended, told by
/// the node that watches its project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunNews {
    /// The project, as `org/repo`.
    pub project: String,
    /// The host the run is on, such as `github.com`, as the Actions tile names it.
    pub host: String,
    pub run: crate::WorkflowRun,
}

/// A slash command or skill an agent offers, as `SessionsRequest::Commands`
/// lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCommand {
    /// As the agent names it, without a leading `/`: `compact`,
    /// `skill:plan` or `$imagegen`.
    pub name: String,
    pub description: String,
    /// What the command takes after its name, such as `[number]`; empty
    /// when it takes nothing or the agent does not say.
    #[serde(default)]
    pub hint: String,
}

/// One worktree of a repository, as `SessionsRequest::Worktrees` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    /// Its folder, as git names it.
    pub path: String,
    /// The branch checked out, without `refs/heads/`; `None` when the head
    /// is detached.
    pub branch: Option<String>,
    /// The commit at its head, abbreviated.
    pub head: String,
    /// The one holding the folder asked about.
    #[serde(default)]
    pub own: bool,
}

/// Something a node's session did that the user would want to hear about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub session: String,
    /// The agent, such as `claude`.
    pub agent: String,
    /// The title the agent gave the session, when it gives one.
    #[serde(default)]
    pub title: Option<String>,
    pub cwd: String,
    pub kind: AlertKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AlertKind {
    /// The session waits for an answer to `question`.
    Asks { question: Question },
    /// A turn ended and no prompt waits behind it. `error` says why a
    /// failed one failed.
    TurnEnded {
        stop_reason: StopReason,
        last_message: String,
        #[serde(default)]
        error: Option<String>,
    },
    /// The agent stopped on its own: it failed or exited.
    Failed { reason: String },
}

/// The version every agent protocol shares.
const VERSION: &str = "1";

/// Lists sessions and reads transcripts.
pub const SESSIONS_PROTOCOL: &str = "agents/1";

/// The protocol for the agent `name`, such as `claude/1`.
pub fn protocol(name: &str) -> String {
    format!("{name}/{VERSION}")
}

/// The agent a protocol name serves, such as `claude` for `claude/1`.
pub fn agent_of(protocol: &str) -> Option<&str> {
    let (name, version) = protocol.split_once('/')?;
    (version == VERSION && crate::node::harness::harness(name).is_some()).then_some(name)
}

/// A step of a move, in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovePhase {
    /// The source still runs the turn, or cannot be reached.
    #[default]
    Waiting,
    /// The conversation is being copied.
    Conversation,
    /// The project's files are being brought in line.
    Files,
    /// The session is starting here.
    Resuming,
    /// The session runs here.
    Done,
    /// The move failed; the session stays where it was.
    Failed,
    /// The record was deleted before the session resumed here.
    Cancelled,
}

/// What `SessionsRequest::MoveProgress` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoveProgress {
    pub phase: MovePhase,
    /// Bytes of the conversation copied, and the source's size when known.
    pub copied: u64,
    pub total: u64,
    /// Files of the project not here yet.
    pub files_left: u64,
    /// The source cannot be reached now.
    pub source_away: bool,
    /// Why it failed, when it did.
    pub reason: Option<String>,
}

/// A session of another machine that this node keeps a copy of
/// (`SessionsRequest::Copies`), or a version of a session that lost to
/// another machine's after both were continued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyView {
    pub agent: String,
    /// The agent's own id for the session.
    pub id: String,
    /// The machine that ran it (this one, for a version that lost).
    pub from: EndpointId,
    /// The project, by the name `sync/<name>` has.
    pub project: String,
    /// The folder it ran in, relative to the project's checkout.
    pub path: String,
    /// Unix seconds: its last activity, and when the copy was last brought
    /// up to date.
    pub last_activity: u64,
    pub fetched_at: u64,
    pub size: u64,
    /// Set for a version that lost: when it was set aside.
    pub lost: Option<u64>,
}
