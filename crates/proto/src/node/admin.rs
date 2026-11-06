//! Looking after a node: what it reports (`status/1`, its traffic), which
//! build it runs and its clock (`version/1`), its log (`logs/1`), where it
//! looks for checkouts (`search/1`), a shell on it (`shell/1`), and updating
//! it and its tools (`update/1`, `tool-update/1`). Each is its own small
//! protocol.

pub mod status {
    //! `status/1`: what a node reports to a peer in its mesh, in place of the
    //! control stream it keeps with a server. The stream starts with `Open`;
    //! the node answers `Opened`, then sends reports until the peer closes it.
    //! `Offers` comes first, `Env` once probed and again when it changes, `Stats`
    //! about once a minute, `Traffic` the same.

    use crate::node::{EnvReport, StatsReport, traffic::TrafficReport};
    use serde::{Deserialize, Serialize};

    pub const PROTOCOL: &str = "status/1";

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    // Sent a few times an hour, never kept in bulk: the size does not matter.
    #[allow(clippy::large_enum_variant)]
    pub enum Report {
        /// The protocols the node serves.
        Offers(Vec<String>),
        Env(EnvReport),
        Stats(StatsReport),
        Traffic(TrafficReport),
    }
}

pub mod traffic {
    //! `Up::Traffic`: the bytes a node moved over iroh, to judge compression by.

    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    /// Cumulative counters since the node's process started. Reported again in
    /// full each time, so a lost message loses nothing; `started_at_ms` lets the
    /// server tell a restart, which zeroes them, from a report it has seen part of.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(default)]
    pub struct TrafficReport {
        /// When the process started, in milliseconds since the Unix epoch.
        pub started_at_ms: u64,
        /// Bytes by key: `alpn.<alpn>.tx` and `.rx` on the wire per protocol, with
        /// QUIC's overhead and retransmits; `endpoint.direct.tx` and
        /// `endpoint.relay.rx` and the like over all of the node's iroh sockets.
        pub counters: BTreeMap<String, u64>,
    }
}

pub mod version {
    //! `version/1`: the node says which beep-node it runs and what time its
    //! clock shows, in one message, and finishes the stream. A node from before
    //! it declines the protocol, which reads as a version not known. `status/1`
    //! is not changed to carry it.

    use serde::{Deserialize, Serialize};

    pub const PROTOCOL: &str = "version/1";

    /// A sample whose round trip took longer than this says nothing about the
    /// clock.
    const MAX_ROUND_TRIP_MS: u64 = 2_000;
    /// A clock this far off, either way, is worth a warning.
    const WARN_OFFSET_MS: i64 = 5_000;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Version {
        /// Such as "0.2.3".
        pub version: String,
        /// The node's Unix time in milliseconds when it answered; none from a
        /// node that does not say.
        #[serde(default)]
        pub now_ms: Option<u64>,
    }

    /// How far the remote clock is ahead of ours, in milliseconds (negative:
    /// behind). `before_ms` and `after_ms` are our Unix time around the round
    /// trip and `remote_ms` is what the other side answered; the answer is taken
    /// to be from the middle of the trip. None when the trip took too long or
    /// our clock went backwards during it.
    pub fn clock_offset_ms(before_ms: u64, after_ms: u64, remote_ms: u64) -> Option<i64> {
        let trip = after_ms.checked_sub(before_ms)?;
        if trip > MAX_ROUND_TRIP_MS {
            return None;
        }
        let middle = i64::try_from(before_ms.checked_add(trip / 2)?).ok()?;
        i64::try_from(remote_ms).ok()?.checked_sub(middle)
    }

    /// Whether an offset is large enough to warn about.
    pub fn clock_is_off(offset_ms: i64) -> bool {
        offset_ms.unsigned_abs() >= WARN_OFFSET_MS.unsigned_abs()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_slow_trip_is_dropped() {
            assert_eq!(clock_offset_ms(0, MAX_ROUND_TRIP_MS + 1, 0), None);
            assert_eq!(clock_offset_ms(0, MAX_ROUND_TRIP_MS, 1_000), Some(0));
        }
    }
}

pub mod logs {
    //! `logs/1`: the node's recent log. The server sends one `Request` per stream
    //! and the node answers with one `Reply`.

    use serde::{Deserialize, Serialize};

    pub const PROTOCOL: &str = "logs/1";
    /// The most lines a node gives.
    pub const MAX_LINES: u32 = 1000;
    /// The most bytes a node gives, whole lines from the end.
    pub const MAX_BYTES: usize = 256 * 1024;

    /// What the server sends.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Request {
        /// The last this many lines, up to `MAX_LINES`.
        pub lines: u32,
    }

    /// What the node answers.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Reply {
        /// The lines, oldest first; empty when nothing was logged lately.
        Log { text: String },
        /// It has no log to read, in words for the user.
        Failed { reason: String },
    }
}

pub mod search {
    //! `search/1`: where the node looks for git checkouts (`search` in its
    //! `node.toml`). The server sends one `Request` per stream and the node
    //! answers with one `Reply`.

    use serde::{Deserialize, Serialize};

    pub const PROTOCOL: &str = "search/1";

    /// What the server sends.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Request {
        Get {},
        /// Looks in `folder` (empty for home) from now on, and walks it again
        /// in the background.
        Set {
            folder: String,
        },
        /// Walks the folder again, quickly and not deep, so checkouts made
        /// since count; answers once done. Older nodes close the stream.
        Rescan {},
    }

    /// What the node answers.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Reply {
        /// The folder as written, `~/` for home.
        Folder { folder: String },
        /// In words for the user.
        Failed { reason: String },
    }
}

pub mod shell {
    //! `shell/1`: a shell on the node. Each stream is one shell that lives as
    //! long as the stream, so the folder and variables a command sets carry over
    //! to the next. Commands run one at a time.

    use serde::{Deserialize, Serialize};

    pub const PROTOCOL: &str = "shell/1";

    /// What the server sends.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Request {
        /// Runs `command` in the shell, with stdin empty and stderr merged into
        /// stdout. After `timeout_secs` the node stops it, and the shell with it.
        Run { command: String, timeout_secs: u32 },
    }

    /// What the node sends for a `Run`: any number of `Output`, then `Done`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Event {
        Output {
            text: String,
        },
        Done {
            /// `None` when the command did not finish: it timed out or ended the
            /// shell.
            exit_code: Option<i32>,
            timed_out: bool,
            /// The shell's folder afterwards, while it still runs.
            cwd: Option<String>,
            /// The command ran in a new shell, started in the home folder,
            /// because there was none yet or the last one ended.
            fresh: bool,
            /// The node dropped output past its limit.
            truncated: bool,
        },
    }
}

pub mod update {
    //! `update/1`: the node installs the newest beep-node release over its own
    //! binary and restarts its service, as `beep-node update` does on the
    //! machine. Only the beep-node CLI offers it; the Mac app's node updates
    //! with the app.
    //!
    //! The server sends one `Request::Update`; the node answers with one
    //! `Event`, finishes the stream, and only then restarts, so the server
    //! reads the answer before the connection drops. It holds new prompts and
    //! restarts once its agent sessions are idle, or after a wait.

    use serde::{Deserialize, Serialize};

    pub const PROTOCOL: &str = "update/1";

    /// What the server sends.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Request {
        /// Installs the newest release if this node is behind it.
        Update {
            /// Restarts without waiting for the agent sessions to be idle. A
            /// node from before it restarts at once.
            #[serde(default)]
            now: bool,
        },
    }

    /// What the node answers.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Event {
        /// `version` is already the newest release, or newer.
        Newest { version: String },
        /// `to` replaced `from`.
        Installed {
            from: String,
            to: String,
            /// The service restarts on `to` once the stream is finished: the
            /// node drops off and connects again.
            restarting: bool,
            /// Why it does not restart, or that it waits for the agent
            /// sessions first, in words for the user; empty when it restarts at
            /// once.
            #[serde(default)]
            note: String,
        },
        /// Nothing was installed, or the install stopped; in words for the
        /// user.
        Failed { reason: String },
    }
}

pub mod tool_update {
    //! `tool-update/1`: the node installs a newer release of a tool it
    //! provisions (`gh`, `teacli`, `flsh`) into its own `bin` folder, checked
    //! against the release's published checksums. The server looks up the
    //! newest release and names it; the node answers with one `Reply`.

    use serde::{Deserialize, Serialize};
    use strum::{Display, EnumString, IntoStaticStr};

    pub const PROTOCOL: &str = "tool-update/1";
    /// Offered beside `PROTOCOL` by a node that also installs `Tool::Flsh`; a
    /// node from before it cannot read a request that names flsh.
    pub const FLSH: &str = "tool-update/flsh";

    /// A tool the node can update.
    #[derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Hash,
        Serialize,
        Deserialize,
        Display,
        EnumString,
        IntoStaticStr,
    )]
    #[serde(rename_all = "snake_case")]
    #[strum(serialize_all = "snake_case")]
    pub enum Tool {
        Gh,
        Teacli,
        Flsh,
    }

    /// Where a tool's releases are published.
    pub enum Releases {
        /// This GitHub repository's releases.
        GitHub(&'static str),
        /// `<base>/latest.txt` names the newest version, and
        /// `<base>/<version>/<file>` with `<file>.sha256` holds each file.
        Channel(&'static str),
    }

    impl Tool {
        pub const ALL: [Tool; 3] = [Tool::Gh, Tool::Teacli, Tool::Flsh];

        pub fn releases(self) -> Releases {
            match self {
                Self::Gh => Releases::GitHub("cli/cli"),
                Self::Teacli => Releases::GitHub("flashmind-labs/teacli"),
                Self::Flsh => Releases::Channel("https://releases.flashmind.com/flashmind/cli"),
            }
        }
    }

    /// What the server sends.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Request {
        /// Installs release `version` (such as `2.103.3`) of `tool`.
        Install { tool: Tool, version: String },
    }

    /// What the node answers.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[non_exhaustive]
    pub enum Reply {
        Installed {},
        /// In words for the user.
        Failed {
            reason: String,
        },
    }
}
