//! The `agents/1` protocol: one request, one reply, about all the sessions.

use super::{
    Activity, Entry, ExternalSession, LiveStatus, ModelList, Question, SessionFolder, SessionInfo,
    Steer, StopReason,
};
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
            | Self::Models { .. }
            | Self::Harnesses { .. }
            | Self::File { .. }
            | Self::Folder { .. }
            | Self::ReadFile { .. }
            | Self::CustomAgents { .. }
            | Self::Diff { .. }
            | Self::Worktrees { .. }
            | Self::Live { .. } => true,
            Self::Watch { .. }
            | Self::Steer { .. }
            | Self::UpdateHarness { .. }
            | Self::FollowImages {}
            | Self::FollowUsage { .. }
            | Self::RefreshPlanUsage {}
            | Self::WriteFile { .. }
            | Self::DeleteSession { .. }
            | Self::FollowAlerts {} => false,
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

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    #[test]
    fn only_one_shot_reads_may_be_asked_again() {
        let folder = SessionsRequest::Folder {
            root: "/srv/project".into(),
            path: "src".into(),
            ignored: false,
            depth: 3,
        };
        let read = SessionsRequest::ReadFile {
            root: "/srv/project".into(),
            path: "notes.txt".into(),
            offset: 0,
            len: 10,
        };
        assert!(folder.is_read());
        assert!(read.is_read());
        assert!(SessionsRequest::List {}.is_read());
        let write = SessionsRequest::WriteFile {
            root: "/srv/project".into(),
            path: "notes.txt".into(),
            offset: 0,
            size: 2,
            data: b"hi".to_vec(),
        };
        assert!(!write.is_read());
        assert!(
            !SessionsRequest::UpdateHarness {
                agent: "claude".into()
            }
            .is_read()
        );
        assert!(!SessionsRequest::FollowAlerts {}.is_read());
        assert!(!SessionsRequest::Watch { sessions: vec![] }.is_read());
    }

    #[test]
    fn worktrees_round_trip_and_may_be_asked_again() {
        let ask = SessionsRequest::Worktrees {
            folder: "/srv/project".into(),
        };
        assert!(ask.is_read());
        let body = rmp_serde::to_vec(&ask).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            ask
        );
        let reply = SessionsReply::Worktrees {
            worktrees: vec![Worktree {
                path: "/srv/project/.worktrees/fix".into(),
                branch: None,
                head: "abc1234".into(),
                own: true,
            }],
        };
        let body = rmp_serde::to_vec(&reply).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
            reply
        );
    }

    #[test]
    fn follow_usage_reads_across_versions() {
        #[derive(Serialize, Deserialize)]
        enum Older {
            FollowUsage {},
        }
        let body = rmp_serde::to_vec_named(&Older::FollowUsage {}).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            SessionsRequest::FollowUsage { history: false }
        );
        let body =
            rmp_serde::to_vec_named(&SessionsRequest::FollowUsage { history: true }).unwrap();
        assert!(matches!(
            rmp_serde::from_slice::<Older>(&body).unwrap(),
            Older::FollowUsage {}
        ));
    }

    #[test]
    fn external_requests_and_replies_round_trip() {
        let request = SessionsRequest::External {
            agent: "claude".into(),
            limit: 20,
            session: None,
        };
        let body = rmp_serde::to_vec_named(&request).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            request
        );
        let reply = SessionsReply::Activity {
            activity: Activity {
                session: "0f3c2a9e-1111-4222-8333-944455556666".into(),
                kind: ActivityKind::NeedsInput,
                message: "Claude needs your permission to use Bash".into(),
                from_hook: true,
            },
        };
        let body = rmp_serde::to_vec_named(&reply).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
            reply
        );
    }

    #[test]
    fn model_requests_and_replies_round_trip() {
        let request = SessionsRequest::Models {
            agent: "flsh".into(),
        };
        let body = rmp_serde::to_vec_named(&request).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            request
        );
        let reply = SessionsReply::Models {
            models: ModelList {
                pick: ModelPick::Listed,
                current: Some("openai:gpt-test".into()),
                models: vec![ModelOption {
                    id: "openai:gpt-test".into(),
                    name: "GPT Test".into(),
                    description: None,
                }],
            },
        };
        let body = rmp_serde::to_vec_named(&reply).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
            reply
        );
        let start = Request::Start {
            cwd: None,
            prompt: "go".into(),
            model: Some("openai:gpt-test".into()),
            effort: Some(Effort::Xhigh),
            checkout: Some(CheckoutPlan::NewWorktree {
                base: Some("main".into()),
            }),
        };
        let body = rmp_serde::to_vec_named(&start).unwrap();
        assert_eq!(rmp_serde::from_slice::<Request>(&body).unwrap(), start);
    }

    #[test]
    fn a_transcript_ask_without_thoughts_reads_here_and_a_thought_reads_as_other_there() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum Older {
            Transcript { session: String, last: u32 },
        }
        let body = rmp_serde::to_vec_named(&Older::Transcript {
            session: "s".into(),
            last: 5,
        })
        .unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            SessionsRequest::Transcript {
                session: "s".into(),
                last: 5,
                thoughts: false,
            }
        );

        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum OlderKind {
            User,
            #[serde(other)]
            Other,
        }
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct OlderEntry {
            kind: OlderKind,
            text: String,
        }
        let thought = Entry {
            kind: EntryKind::Thought,
            text: "hmm".into(),
            ms: Some(1200),
        };
        let body = rmp_serde::to_vec_named(&thought).unwrap();
        assert_eq!(rmp_serde::from_slice::<Entry>(&body).unwrap(), thought);
        assert_eq!(
            rmp_serde::from_slice::<OlderEntry>(&body).unwrap(),
            OlderEntry {
                kind: OlderKind::Other,
                text: "hmm".into()
            }
        );
        let body = rmp_serde::to_vec_named(&OlderEntry {
            kind: OlderKind::User,
            text: "hi".into(),
        })
        .unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Entry>(&body).unwrap(),
            Entry::new(EntryKind::User, "hi".into())
        );
    }
}
