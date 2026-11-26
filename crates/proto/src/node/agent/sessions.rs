//! The `agents/1` protocol: one request, one reply, about all the sessions,
//! and the names of the protocols each agent speaks.

use super::{
    Activity, Entry, ExternalSession, LiveStatus, ModelList, Question, ReviewOrder, ReviewRun,
    SessionFolder, SessionInfo, Steer, StopReason,
};
use crate::SessionId;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};

/// What the caller sends on an `agents/1` stream: one request, one reply.
/// A node older than a request closes the stream or answers `Error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, strum::IntoStaticStr)]
#[non_exhaustive]
pub enum SessionsRequest {
    List {},
    /// The last `last` entries of a session's transcript. `session` may
    /// also be an external session's id.
    Transcript {
        session: SessionId,
        last: u32,
        /// Keep the agent's thoughts, as `EntryKind::Thought` entries. A
        /// caller from before them leaves it out and gets none, so it
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
        session: Option<SessionId>,
    },
    /// Follows these external sessions: `Watching` first, then `Activity`
    /// as they finish turns or wait for the user, until the caller closes
    /// the stream.
    Watch {
        sessions: Vec<SessionId>,
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
        session: SessionId,
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
    /// Refused: no current caller follows pictures. Kept so the request
    /// numbers stay.
    FollowImages {},
    /// Sends what each finished turn spent, as `TurnUsage` replies, and how
    /// much of each agent's plan is left, as `PlanUsage` replies (all of them
    /// first, then each time one changes), until the caller closes the stream.
    /// Turns that ended while no stream was open wait, a bounded few. On this
    /// stream the caller may later send `RefreshPlanUsage`; anything else is
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
    /// The folder `path` inside the working folder `root`. Answered with `Folder`, or `Error` saying why, for a path
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
    /// and in the node's home. Answered with
    /// `CustomAgents`, or `Error` saying why.
    CustomAgents {
        folder: String,
    },
    /// Sends what the user would want to hear about the node's sessions, as
    /// `Alert` replies, until the caller closes the stream: a question a
    /// session asks, a turn that ended with no prompt queued behind it, and a
    /// session that failed. Alerts that came while no stream was open wait, a
    /// bounded few.
    FollowAlerts {},
    /// What changed in the checkout `ask.root`.
    /// Answered with `Diff`, or `Error` saying why.
    Diff {
        ask: DiffAsk,
    },
    /// How Claude Code session `session`, started outside Beep, is doing now,
    /// as its hooks report it. Answered with `Live`, or `Error`.
    Live {
        session: SessionId,
    },
    /// Queues `steer` on the node for the next hook of `session` to deliver: a
    /// message after the current tool call or at the turn's end, a stop at the
    /// next tool call. Answered with `Live`, or `Error` saying why it cannot,
    /// such as a session idle at its prompt.
    Steer {
        session: SessionId,
        steer: Steer,
    },
    /// The worktrees of the git repository holding the session folder
    /// `folder`, as `git worktree list` gives them. Answered with
    /// `Worktrees` (none when `folder` is not in a repository), or `Error`
    /// saying why. A node from before it answers `Error` or closes the
    /// stream. With `session`, each worktree says whether that session
    /// created it.
    Worktrees {
        folder: String,
        #[serde(default)]
        session: Option<SessionId>,
    },
    /// Deletes the ended session `session` of the harness `agent` from this
    /// machine: its transcript file and the node's record of it. `session`
    /// is the agent's own id or the node's. Answered with `Deleted`, or
    /// `Error` saying why not, such as a session that still runs. A node
    /// from before it answers `Error` or closes the stream.
    DeleteSession {
        agent: String,
        session: SessionId,
    },
    /// The slash commands and skills the agent of the running session
    /// `session` offers, as it last reported them (the list arrives after
    /// the session starts and can change, so it is asked for when needed).
    /// Answered with `Commands`, empty for a session that is not running,
    /// or `Error`. A node from before it answers `Error` or closes the
    /// stream.
    Commands {
        session: SessionId,
    },
    /// How the move of session `session` to this node stands, as the target
    /// says. Answered with `Moving`, or `Error`. A node from before it
    /// answers `Error` or closes the stream: it cannot take a session.
    MoveProgress {
        session: SessionId,
    },
    /// Asked by apps from before worktree moves were removed, before they
    /// move a session in a worktree. Answered with `MoveChecked` and
    /// `ignored: false`: the node resumes such a session in the project
    /// folder.
    MoveCheck {
        project: String,
        branch: String,
    },
    /// The copies of other machines' sessions this node keeps, and the
    /// versions that lost to another machine's. Answered with `Copies`; a
    /// node from before background copies closes the stream. Nodes keep no
    /// copies now and answer with none; `ContinueCopy` is refused and
    /// `DiscardCopy` does nothing.
    Copies {},
    /// Runs the copy of session `session` of `agent` here, in the project's
    /// checkout on this node; with `lost`, the version set aside at that
    /// time instead. Answered with `CopyDone`, or `Error` saying why not.
    ContinueCopy {
        agent: String,
        session: SessionId,
        lost: Option<u64>,
    },
    /// Throws away the copy of a session, or its version set aside at
    /// `lost`. Answered with `CopyDone`.
    DiscardCopy {
        agent: String,
        session: SessionId,
        lost: Option<u64>,
    },
    /// `Image` made small on the node, so a remote log's picture does not
    /// cross the network whole. Answered as `Image` is; a node from before
    /// it closes the stream.
    Thumb {
        session: SessionId,
        key: String,
    },
    /// Sends the CI runs the user started that ended, as `Run` replies, until
    /// the caller closes the stream: on the node that watches each project. A
    /// node from before it closes the stream.
    FollowRuns {},
    /// Whether each of `paths` (absolute or starting `~/`, at most
    /// `MAX_PRESENT_PATHS`) is there, as the Artifacts tab asks. Answered
    /// with `Present`; a node from before it answers `Error`.
    Present {
        paths: Vec<String>,
    },
    /// Starts a temporary reviewer of `order.cwd`'s changes, a child of
    /// `order.parent` when given. Answered with `Review` once it runs, or
    /// `Error` saying why not. The reviewer never shows in `List`, only in `ListAll` under its parent, and goes
    /// once it submitted or failed; its review stays. A node from before it
    /// answers `Error`.
    StartReview {
        order: ReviewOrder,
    },
    /// How the review `id` stands. Answered with `Review`, or `Error` when
    /// the node has no such review.
    Review {
        id: SessionId,
    },
    /// The reviews of the session `session`'s work, oldest first. Answered
    /// with `Reviews`.
    Reviews {
        session: SessionId,
    },
    /// Live commands owned by this session, answered with `Processes`.
    Processes {
        session: SessionId,
    },
    /// Stops only a process still owned by this session, answered with `ProcessStopped`.
    StopProcess {
        session: SessionId,
        pid: u32,
    },
    /// Installs the ACP adapters the agents here lack now, rather than at
    /// the next hourly try. Answered with `AdaptersInstalling` at once; how
    /// it goes comes in the node's `EnvReport::adapters`.
    InstallAdapters {},
    /// Known native sessions first, then whenever a local CLI opens,
    /// updates or closes one, as `External` snapshots.
    FollowPresence {},
    /// Mutates entries inside a working folder. Answered only with `FilesEdited` or `Error`.
    EditFiles {
        root: String,
        action: FileAction,
    },
    /// Writes a temporary attachment outside the project, authorized by `root`.
    /// A draft with a UUID and the agent name `draft` can use an empty root.
    /// `path` is a file name on the first part, then the absolute path returned
    /// by `Written`. Older nodes refuse this request.
    UploadFile {
        root: String,
        agent: String,
        session: SessionId,
        path: String,
        offset: u64,
        size: u64,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// `List` with the running reviewers that have a parent, marked by
    /// `SessionInfo::reviewer`. A node from before it answers `Error`.
    ListAll {},
    /// Moves a closed session to a working folder, answered with `Deleted` or `Error`.
    RelocateSession {
        agent: String,
        session: SessionId,
        folder: String,
    },
    /// Asks the process of `agent`'s session `session`, started outside Beep,
    /// to end, so Beep can continue it: the node finds that process itself.
    /// Answered with `ProcessStopped` once it ended, or `Error` saying why
    /// not. A node from before it answers `Error` or closes the stream.
    CloseExternal {
        agent: String,
        session: SessionId,
    },
    /// The sessions `sessions` started outside Beep, each looked up by its
    /// agent and id, answered with `External` holding those found, in no
    /// order. A node from before it closes the stream: ask `External` for
    /// each then.
    ExternalNamed {
        sessions: Vec<ExternalAsk>,
    },
    /// Lets the asking device fetch the media file `path`, absolute or
    /// starting `~/`, from the node's blob store for a few minutes. Answered
    /// with `Shared`, `NoFile` when there is no such file, or `Error`. A node
    /// from before it closes the stream: read the file with `File` then.
    Share {
        path: String,
    },
    /// One page of session metadata in these folders. Older nodes refuse
    /// this request; callers can fall back to `InFolders`.
    InFoldersPage {
        folders: Vec<String>,
        limit: u32,
        before: Option<HistoryCursor>,
    },
    /// The BLAKE3 hash of each of `paths` (at most `MAX_HASH_PATHS`) inside
    /// `root`, read as `ReadFile` reads them, so a viewer can tell a file it
    /// shows changed. Answered with `Hashes`; a node from before it answers
    /// `Error`.
    HashFiles {
        root: String,
        paths: Vec<String>,
    },
    /// The sessions whose prompts or agent replies hold every word of
    /// `query`, from the node's search index. Answered with `SearchHits`; a
    /// node from before it closes the stream.
    Search {
        query: String,
        /// Only these sessions, by the agent's own id; any when empty.
        sessions: Vec<SessionId>,
    },
    /// Follows the node's search index from row `after` of index `epoch`:
    /// `IndexRows` batches up to the newest row, then each change as it
    /// comes. A different epoch starts from the first row.
    FollowIndex {
        epoch: String,
        after: u64,
    },
    /// Discards one temporary upload from a composer draft.
    DiscardDraftAttachment {
        draft: SessionId,
        path: String,
    },
    /// Looks up the definition of the symbol at 0-based `line` and UTF-16
    /// `column` of `path` inside `root` using an installed language server.
    /// `text`, when given, is the editor's unsaved buffer. Answered with
    /// `LspLocations` or `Error`. Older nodes refuse this request.
    LspDefinition {
        root: String,
        path: RelPath,
        line: u32,
        column: u32,
        #[serde(default)]
        text: Option<String>,
    },
    /// Stops a Docker container this session started, answered with
    /// `ProcessStopped`. Older nodes refuse this request.
    StopContainer {
        session: SessionId,
        id: String,
    },
}

/// One session `SessionsRequest::Search` found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    /// The agent's own id for the session.
    pub session: SessionId,
    /// Unix milliseconds of its last activity.
    pub activity: u64,
    /// For each query word, the part around it of the first prompt or
    /// reply holding it.
    pub texts: Vec<String>,
    /// The details the app needs to open a hit absent from its session list.
    #[serde(default)]
    pub metadata: Option<SearchSession>,
}

/// A conversation as it appears in the search index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSession {
    pub agent: String,
    pub cwd: String,
    pub title: Option<String>,
    /// Unix seconds.
    pub started_at: u64,
    /// Beep's id when it owns the conversation.
    pub managed: Option<SessionId>,
}

/// The character where `text` first holds the lowercase `word`, counted
/// in `text`'s own characters, which lowercasing can lengthen.
pub fn folded_at(text: &str, word: &str) -> Option<usize> {
    let folded = text.to_lowercase();
    let folded_at = folded.find(word)?;
    let mut bytes = 0;

    Some(
        text.chars()
            .take_while(|ch| {
                let before = bytes;
                bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
                before < folded_at
            })
            .count(),
    )
}

/// One change of a node's search index. A row with text adds a prompt or
/// reply; one without resets the session: a copy drops what it holds of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRow {
    pub seq: u64,
    /// The agent's own id for the session.
    pub session: SessionId,
    pub text: Option<String>,
    /// Unix milliseconds of the session's last activity when it was read.
    pub activity: u64,
    /// An empty text with metadata updates the session without adding a
    /// searchable message. Older copies treat it as an empty message.
    #[serde(default)]
    pub metadata: Option<SearchSession>,
}

/// The most paths one `HashFiles` asks about.
pub const MAX_HASH_PATHS: usize = 64;

/// A position in history: newest activity first, then agent and id ascending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCursor {
    pub last_activity: u64,
    pub agent: String,
    pub id: SessionId,
}

/// Session metadata only. Transcripts are requested when a session opens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPage {
    pub sessions: Vec<ExternalSession>,
    pub next: Option<HistoryCursor>,
}

/// One session asked for by `SessionsRequest::ExternalNamed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalAsk {
    /// Such as `claude` or `codex`.
    pub agent: String,
    /// The agent's own id for it.
    pub session: SessionId,
}

/// The most sessions one `ExternalNamed` asks for.
pub const MAX_EXTERNAL_NAMED: usize = 100;

/// File browser operations. Destinations must not already exist, except
/// the file `Write` replaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FileAction {
    CreateFile {
        path: RelPath,
    },
    CreateFolder {
        path: RelPath,
    },
    Rename {
        path: RelPath,
        to: RelPath,
    },
    Copy {
        paths: Vec<RelPath>,
        folder: RelPath,
    },
    Move {
        paths: Vec<RelPath>,
        folder: RelPath,
    },
    Delete {
        paths: Vec<RelPath>,
    },
    /// Replaces the text of the file `path`, keeping its permissions. Older
    /// nodes refuse it.
    Write {
        path: RelPath,
        text: String,
    },
}

/// A path inside a working folder, `/`-separated on every system, with no
/// `.`, `..` or empty parts; empty for the folder itself. On the wire it is
/// the string, so peers that send a plain string read it the same.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

/// Why a string is not a [`RelPath`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0} is not a relative path inside the working folder")]
pub struct NotRelPath(String);

impl RelPath {
    /// The working folder itself.
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Its names, outermost first.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|part| !part.is_empty())
    }
}

impl TryFrom<&str> for RelPath {
    type Error = NotRelPath;

    /// Drops empty and `.` parts, as in `a//b/./c/`; refuses absolute paths,
    /// `..`, and the `\\` and `:` of Windows paths.
    fn try_from(path: &str) -> Result<Self, Self::Error> {
        let outside = path.starts_with('/') || path.contains(['\\', ':']);
        let parts = path.split('/').filter(|part| !matches!(*part, "" | "."));
        let parts: Vec<&str> = parts.collect();

        if outside || parts.contains(&"..") {
            return Err(NotRelPath(path.to_owned()));
        }

        Ok(Self(parts.join("/")))
    }
}

impl TryFrom<String> for RelPath {
    type Error = NotRelPath;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        Self::try_from(path.as_str())
    }
}

impl From<RelPath> for String {
    fn from(path: RelPath) -> Self {
        path.0
    }
}

impl AsRef<str> for RelPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RelPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The path on this machine, under the working folder.
impl From<&RelPath> for std::path::PathBuf {
    fn from(path: &RelPath) -> Self {
        path.parts().collect()
    }
}

/// The most paths one `Present` asks about.
pub const MAX_PRESENT_PATHS: usize = 200;

impl SessionsRequest {}

/// What `SessionsRequest::Diff` reads.
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
    #[serde(default)]
    pub context: u32,
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
        unknown: Vec<SessionId>,
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
        session: SessionId,
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
    /// The answer to `Folder` with a `depth`. On the `files` topic of a folder it also says
    /// which paths changed since the update before, relative to the folder, at most 256, with
    /// `.git` for a change in its Git metadata outside the folder. Empty when they are not
    /// known, and from a node from before it: then anything may have changed.
    Tree {
        tree: crate::WorkTree,
        #[serde(default)]
        changed: Vec<String>,
    },
    /// The answer to `WriteFile` (relative to its root), or `UploadFile`
    /// (an absolute path on the host).
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
        /// Always false now.
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
    /// The answer to `Present`: one per path asked, in order.
    Present {
        present: Vec<bool>,
    },
    /// The answer to `StartReview` and `Review`.
    Review {
        run: ReviewRun,
    },
    /// The answer to `Reviews`.
    Reviews {
        runs: Vec<ReviewRun>,
    },
    Processes {
        processes: Vec<RunningProcess>,
        /// The Docker containers the session started; empty from nodes that predate them.
        #[serde(default)]
        containers: Vec<RunningContainer>,
    },
    /// The answer to `StopProcess` and `CloseExternal`.
    ProcessStopped {},
    /// The answer to `InstallAdapters`.
    AdaptersInstalling {},
    /// Sent only in response to `EditFiles`.
    FilesEdited {},
    /// The answer to `Share`: the file's BLAKE3 hash, a raw blob the asking
    /// device may fetch over iroh-blobs, and its size.
    Shared {
        #[serde(with = "serde_bytes")]
        hash: [u8; 32],
        size: u64,
    },
    /// The answer to `InFoldersPage`.
    SessionPage {
        page: SessionPage,
    },
    /// The answer to `HashFiles`: one per path asked, in order, `None` for
    /// one that is not there or cannot be read.
    Hashes {
        hashes: Vec<Option<serde_bytes::ByteArray<32>>>,
    },
    /// The answer to `Search`, most recent activity first.
    SearchHits {
        hits: Vec<SearchHit>,
    },
    /// One batch on a `FollowIndex` stream, in `seq` order. `epoch` names the
    /// index the rows belong to; `current` is set once the batch reaches the
    /// newest row.
    IndexRows {
        epoch: String,
        rows: Vec<IndexRow>,
        current: bool,
    },
    /// The answer to `LspDefinition`: definition targets in server order, and
    /// a short note when no language server could answer.
    LspLocations {
        result: LspDefinitions,
    },
    /// Every move this node is the target of, as the `move-progress` topic
    /// pushes them. Added after `LspLocations`.
    Moves {
        moves: Vec<MoveEntry>,
    },
}

/// How the move of one session to a node stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveEntry {
    pub session: SessionId,
    /// The unix milliseconds of the move's record.
    pub at: u64,
    pub progress: MoveProgress,
}

/// What `SessionsRequest::LspDefinition` returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspDefinitions {
    pub locations: Vec<LspLocation>,
    #[serde(default)]
    pub note: Option<String>,
}

/// One definition target reported by a language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspLocation {
    /// Path relative to the request's `root` when `inside_root` is true,
    /// otherwise an absolute path on the node.
    pub path: String,
    #[serde(default)]
    pub inside_root: bool,
    /// 0-based line number.
    pub line: u32,
    /// 0-based UTF-16 column.
    pub column: u32,
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
    /// The session the list was asked for created it.
    #[serde(default)]
    pub created: bool,
    /// The sessions that created it, said when the list was asked for no session: one read
    /// serves every session of the folder. Empty from an older node.
    #[serde(default)]
    pub created_by: Vec<SessionId>,
}

/// Something a node's session did that the user would want to hear about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub session: SessionId,
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
    /// The checkouts are brought to one commit and the session's worktrees
    /// come over.
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
    /// Files of the project not here yet; no longer sent (0).
    pub files_left: u64,
    /// The source cannot be reached now.
    pub source_away: bool,
    /// Why it failed when it did, or what it waits for, such as checkouts
    /// at two commits.
    pub reason: Option<String>,
    /// The worktree being brought, by its folder name, and which of how
    /// many. Added after `reason`.
    pub worktree: Option<String>,
    pub worktree_at: u32,
    pub worktrees: u32,
    /// The source's checkout has uncommitted edits, which stay there.
    pub edits_left: bool,
    /// Bytes of a chat's files copied, and what they take on the source.
    /// Added after `edits_left`.
    pub files_copied: u64,
    pub files_total: u64,
}

/// A session of another machine that this node keeps a copy of
/// (`SessionsRequest::Copies`), or a version of a session that lost to
/// another machine's after both were continued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyView {
    pub agent: String,
    /// The agent's own id for the session.
    pub id: SessionId,
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

/// A live command in a coding agent session. Times are Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningProcess {
    pub pid: u32,
    pub command: String,
    pub started_at: u64,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub left_group: bool,
    /// The process that started it; 0 from nodes that predate it.
    #[serde(default)]
    pub parent: u32,
    /// The `ports` that answer `GET /` on loopback with a page or a redirect.
    #[serde(default)]
    pub pages: Vec<u16>,
}

/// A Docker container a coding agent session started, through the
/// session's Docker socket. Times are Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningContainer {
    /// Docker's full container id.
    pub id: String,
    pub name: String,
    pub image: String,
    pub started_at: u64,
    /// The host TCP ports it publishes.
    pub ports: Vec<u16>,
    /// The `ports` that answer `GET /` on loopback with a page or a redirect.
    pub pages: Vec<u16>,
}

#[cfg(test)]
#[path = "../../tests/node_agent_search.rs"]
mod tests;
