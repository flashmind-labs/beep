//! The link between the server and a node: a machine running `beep-node`
//! that Beep runs work on.
//!
//! A node keeps one connection open on `NODE_ALPN`, signed in with its key
//! like any device. The first bi stream it opens is the control stream: it
//! sends a `NodeHello`, the server answers `Down::Welcome`, and from then on
//! the node sends `Up` messages and the server `Down` ones. Work runs in
//! sessions, each a bi stream the server opens that starts with the name of
//! the protocol it speaks, one of the hello's `offers`.
//!
//! Unlike the app wire, messages are msgpack maps with named fields. Nodes
//! update on their own schedule, so either side may add a field (with
//! `#[serde(default)]`) or a variant, and the other skips what it does not
//! know.

use crate::SecretText;
use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;
use strum::{Display, EnumString, IntoStaticStr};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

pub mod agent;
pub mod desktop;
pub mod files;
pub mod harness;
pub mod process;
pub mod shell;
pub mod sync;
pub mod terminal;
pub mod tools;
pub mod transfer;
pub mod update;
pub mod usage;
pub mod webhook;

pub const NODE_ALPN: &[u8] = b"beep-node/1";

/// The code the server closes a node's connection with when it refuses the
/// hello's setup token; the reason says why, in words for the user.
pub const CLOSE_SETUP_REFUSED: u32 = 400;

/// The largest message either side reads.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// The first message on the control stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeHello {
    /// The daemon's version, such as "0.1.0".
    pub version: String,
    /// What the user calls the machine, such as "dev server".
    pub name: String,
    /// The session protocols this node serves, such as `shell/1` or
    /// `claude/1`. A session stream opens with one of these names; a node
    /// only offers a tool it has.
    #[serde(default)]
    pub offers: Vec<String>,
    /// The folder Beep keeps the user's files in and runs its code in, when
    /// this node is the user's own environment (such as deploy/onprem's
    /// node container). It is also the first folder `files/1` shares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// A setup token to claim before anything else (`beep-node setup`):
    /// the server links this node's key to the account that made the token,
    /// says so with `Down::Linked`, then welcomes it as any linked node. A
    /// server from before tokens closes the connection with "not linked".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<NodeSetup>,
    /// The node takes `Down::Credentials`. A node from before it never
    /// gets the user's credentials.
    #[serde(default)]
    pub credentials: bool,
    /// The node makes the branch or worktree `agent::Request::Start`'s
    /// `checkout` asks for. A node from before it ignores that field, so
    /// the server refuses a checkout plan for it.
    #[serde(default)]
    pub checkouts: bool,
    /// The node takes `Down::AgentSettings`. A node from before it starts
    /// its harnesses as it finds them.
    #[serde(default)]
    pub agent_settings: bool,
    /// The node makes `agent::CheckoutPlan::NamedWorktree` and
    /// `BranchWorktree`. A node from before them cannot read a start that
    /// asks for one, so the server refuses those plans for it.
    #[serde(default)]
    pub branch_checkouts: bool,
}

/// What `NodeHello::setup` carries.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSetup {
    /// The token as `setup::SetupToken` writes it.
    pub token: String,
    /// The name the user gave on the machine, which wins over the token's.
    /// Without either, the machine is called by the hello's `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Never shows the token.
impl fmt::Debug for NodeSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeSetup")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// What a node sends on the control stream after its hello.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
// Sent a few times an hour, never kept in bulk: the size does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Up {
    /// What the machine has. Sent after the hello and again when it changes.
    Env(EnvReport),
    /// How much disk and memory the machine has free now. Sent after the
    /// first `Env` and about once a minute after. A server from before it
    /// skips it.
    Stats(StatsReport),
}

/// What the server sends on the control stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Down {
    Welcome {
        /// The server takes `tools/1` streams from the node. A server from
        /// before them leaves it out, and a node from before them skips it.
        #[serde(default)]
        tools: bool,
    },
    /// The node's setup token was claimed: its key now signs into
    /// `account` (the account's name) as the machine `name`. Sent before the
    /// welcome, only to a node whose hello carried `setup`.
    Linked { account: String, name: String },
    /// The user's credentials meant for their machines, sent after the
    /// welcome and again whenever they change, each time in full. Only to
    /// a node whose hello says it takes them. The node keeps them in
    /// memory.
    Credentials(MachineCredentials),
    /// The user's settings for the coding agent harnesses on this machine,
    /// sent after the welcome and again whenever they change, each time in
    /// full. Only to a node whose hello says it takes them.
    AgentSettings(harness::AgentSettings),
}

/// What a node uses on the user's behalf.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineCredentials {
    /// Env vars every process the node starts gets, such as `GH_TOKEN`.
    pub env: Vec<EnvSecret>,
    /// The Forgejo and Gitea servers teacli signs in to.
    pub forgejo: Vec<ForgejoLogin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvSecret {
    pub name: String,
    pub value: SecretText,
}

/// One Forgejo or Gitea server and the token for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgejoLogin {
    /// Its base URL, such as `https://git.example.com`, without a trailing
    /// slash.
    pub url: String,
    pub token: SecretText,
}

/// The first message on a session stream: the protocol it speaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub protocol: String,
}

/// The node's answer to `Open`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Opened {
    Ready {},
    Refused { reason: String },
}

/// The facts a node reports about its machine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvReport {
    /// `linux` or `macos`.
    pub os: String,
    /// The OS release, such as "Ubuntu 24.04" or "macOS 26.3".
    pub os_version: String,
    pub arch: String,
    pub hostname: String,
    pub cpus: u32,
    pub memory_mb: u64,
    /// The folder commands start in when none is given.
    pub home: String,
    /// The known tools found on `PATH`.
    pub tools: Vec<ToolInfo>,
    /// Where the CLIs that sign in are signed in.
    pub logins: Vec<Login>,
    /// Android virtual devices, when the SDK is installed.
    pub avds: Vec<String>,
    /// Creative and developer apps installed on the machine.
    #[serde(default)]
    pub apps: Vec<AppInfo>,
    /// The graphics cards the machine has.
    #[serde(default)]
    pub gpus: Vec<GpuInfo>,
    /// The folders the machine's Syncthing keeps in step, as its REST API
    /// lists them; empty when Syncthing is not running.
    #[serde(default)]
    pub syncthing: Vec<SyncthingFolder>,
}

/// A folder Syncthing keeps in step with the user's other devices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncthingFolder {
    /// Its folder ID, the same on every device that shares it.
    pub id: String,
    /// Its label on this machine; may be empty.
    pub label: String,
    /// Its absolute path, `~` expanded and symlinks resolved.
    pub path: String,
}

impl SyncthingFolder {
    /// The label, or the ID when it has none.
    pub fn name(&self) -> &str {
        if self.label.is_empty() {
            &self.id
        } else {
            &self.label
        }
    }

    /// Whether `folder` is this folder or lies inside it.
    pub fn holds(&self, folder: &str) -> bool {
        let separator = |c: char| c == '/' || c == '\\';
        if self.path.is_empty() || folder.is_empty() {
            return false;
        }
        let root = self.path.trim_end_matches(separator);
        let folder = folder.trim_end_matches(separator);
        match folder.strip_prefix(root) {
            Some("") => true,
            Some(rest) => rest.starts_with(separator),
            None => false,
        }
    }
}

impl EnvReport {
    /// The Syncthing folder that holds `folder`, the innermost when they
    /// nest.
    pub fn syncthing_folder(&self, folder: &str) -> Option<&SyncthingFolder> {
        self.syncthing
            .iter()
            .filter(|synced| synced.holds(folder))
            .max_by_key(|synced| synced.path.len())
    }

    /// What the machine can be asked to do, sorted and without repeats.
    pub fn capabilities(&self) -> Vec<Capability> {
        let has_tool = |name: &str| self.tools.iter().any(|tool| tool.name == name);
        let has_app = |name: AppName| self.apps.iter().any(|app| app.name == name);
        let mut found = Vec::new();
        if !self.gpus.is_empty() {
            found.push(Capability::Gpu);
        }
        if self.gpus.iter().any(|gpu| gpu.vendor == GpuVendor::Nvidia) {
            found.push(Capability::Cuda);
        }
        if has_app(AppName::Blender) {
            found.push(Capability::Blender);
        }
        if has_app(AppName::UnrealEngine) {
            found.push(Capability::Unreal);
        }
        if has_tool("xcodebuild") || has_app(AppName::Xcode) {
            found.push(Capability::Xcode);
        }
        if has_tool("adb") || has_app(AppName::AndroidStudio) {
            found.push(Capability::Android);
        }
        if has_tool("docker") || has_app(AppName::DockerDesktop) {
            found.push(Capability::Docker);
        }
        found.sort();
        found.dedup();
        found
    }
}

/// A machine's live resources.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsReport {
    /// The workspace's volume first, when the node keeps one, then the
    /// home folder's when it is on another volume.
    pub disks: Vec<DiskUsage>,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    /// The load average over the last minute.
    pub load: Option<f64>,
}

/// How full the volume holding one folder is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskUsage {
    /// The folder it was measured at.
    pub path: String,
    /// What a user may still write, in bytes.
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// An installed app worth knowing about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: AppName,
    /// Its version, when it could be read.
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Display,
    IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
pub enum AppName {
    Blender,
    #[strum(serialize = "Unreal Engine")]
    UnrealEngine,
    Xcode,
    #[strum(serialize = "Android Studio")]
    AndroidStudio,
    #[strum(serialize = "Docker Desktop")]
    DockerDesktop,
    /// An app a newer node reports that this build does not know.
    #[serde(other)]
    Unknown,
}

/// A graphics card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub model: String,
    /// Video memory in MB, when known.
    #[serde(default)]
    pub vram_mb: Option<u64>,
    #[serde(default)]
    pub driver: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    #[strum(serialize = "NVIDIA")]
    Nvidia,
    #[strum(serialize = "AMD")]
    Amd,
    Intel,
    Apple,
    #[serde(other)]
    Other,
}

/// What a machine can be asked to do.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    IntoStaticStr,
    EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "lowercase")]
pub enum Capability {
    Gpu,
    Cuda,
    Blender,
    Unreal,
    Xcode,
    Android,
    Docker,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    /// The first line of its version output, when it gives one.
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Login {
    /// The tool, such as `gh`.
    pub tool: String,
    pub signed_in: bool,
    /// Who it is signed in as, when it says.
    #[serde(default)]
    pub account: Option<String>,
}

/// Sends messages on one stream of the node link.
pub struct LinkTx(FramedWrite<SendStream, LengthDelimitedCodec>);

/// Reads messages from one stream of the node link.
pub struct LinkRx(FramedRead<RecvStream, LengthDelimitedCodec>);

pub fn link(send: SendStream, recv: RecvStream) -> (LinkTx, LinkRx) {
    let codec = || {
        LengthDelimitedCodec::builder()
            .max_frame_length(MAX_FRAME)
            .new_codec()
    };
    (
        LinkTx(FramedWrite::new(send, codec())),
        LinkRx(FramedRead::new(recv, codec())),
    )
}

impl LinkTx {
    pub async fn send<T: Serialize>(&mut self, message: &T) -> Result<()> {
        let body = rmp_serde::to_vec_named(message)?;
        self.0.send(body.into()).await?;
        Ok(())
    }

    /// Finishes the stream and waits until the peer read all of it or
    /// stopped reading.
    pub async fn finish(self) -> Result<()> {
        let mut send = self.0.into_inner();
        send.finish()?;
        send.stopped().await?;
        Ok(())
    }
}

impl LinkRx {
    /// Reads no message longer than `max` bytes from now on, such as a
    /// hello from a key nobody linked yet; `MAX_FRAME` is the default.
    pub fn set_max_frame(&mut self, max: usize) {
        self.0.decoder_mut().set_max_frame_length(max);
    }

    /// The next message, `None` once the peer finished the stream.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let Some(frame) = self.0.next().await else {
            return Ok(None);
        };
        let frame = frame?;
        let message = rmp_serde::from_slice(&frame).context("could not read a node message")?;
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_with_unknown_fields_still_reads() {
        #[derive(Serialize)]
        struct Newer {
            os: &'static str,
            displays: u32,
        }
        let body = rmp_serde::to_vec_named(&Newer {
            os: "linux",
            displays: 2,
        })
        .unwrap();
        let report: EnvReport = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(report.os, "linux");
        assert!(report.tools.is_empty());
    }

    #[test]
    fn a_hello_without_a_workspace_still_reads() {
        #[derive(Serialize)]
        struct Older {
            version: &'static str,
            name: &'static str,
            offers: Vec<&'static str>,
        }
        let body = rmp_serde::to_vec_named(&Older {
            version: "0.1.0",
            name: "box",
            offers: vec!["shell/1"],
        })
        .unwrap();
        let hello: NodeHello = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(hello.workspace, None);
        assert_eq!(hello.setup, None);
        let newer = NodeHello {
            workspace: Some("/home/user".into()),
            setup: Some(NodeSetup {
                token: "bk1-test".into(),
                name: None,
            }),
            ..hello
        };
        let body = rmp_serde::to_vec_named(&newer).unwrap();
        assert_eq!(rmp_serde::from_slice::<NodeHello>(&body).unwrap(), newer);
    }

    #[test]
    fn credentials_round_trip_and_hide_in_debug() {
        let down = Down::Credentials(MachineCredentials {
            env: vec![EnvSecret {
                name: "GH_TOKEN".into(),
                value: SecretText::new("ghp_hidden9876"),
            }],
            forgejo: vec![ForgejoLogin {
                url: "https://git.example.com".into(),
                token: SecretText::new("forgejo5555"),
            }],
        });
        let body = rmp_serde::to_vec_named(&down).unwrap();
        assert_eq!(rmp_serde::from_slice::<Down>(&body).unwrap(), down);
        let debug = format!("{down:?}");
        assert!(
            !debug.contains("9876") && !debug.contains("5555"),
            "{debug}"
        );
    }

    #[test]
    fn an_older_node_skips_credentials() {
        // What a node from before `Down::Credentials` knew.
        #[derive(Debug, Deserialize)]
        enum OldDown {
            Welcome {},
        }
        let body =
            rmp_serde::to_vec_named(&Down::Credentials(MachineCredentials::default())).unwrap();
        assert!(rmp_serde::from_slice::<OldDown>(&body).is_err());
        let hello = NodeHello {
            version: "0.1.0".into(),
            name: "box".into(),
            offers: vec![],
            workspace: None,
            setup: None,
            credentials: true,
            checkouts: true,
            agent_settings: true,
            branch_checkouts: true,
        };
        let body = rmp_serde::to_vec_named(&hello).unwrap();
        assert_eq!(rmp_serde::from_slice::<NodeHello>(&body).unwrap(), hello);
    }

    #[test]
    fn an_older_node_reads_a_welcome_that_offers_tools() {
        // What a node from before `tools` knew.
        #[derive(Debug, Deserialize)]
        enum OldDown {
            Welcome {},
        }
        let body = rmp_serde::to_vec_named(&Down::Welcome { tools: true }).unwrap();
        assert!(matches!(
            rmp_serde::from_slice::<OldDown>(&body),
            Ok(OldDown::Welcome {})
        ));
        // And a newer node reads an older server's welcome.
        #[derive(Serialize)]
        enum Older {
            Welcome {},
        }
        let body = rmp_serde::to_vec_named(&Older::Welcome {}).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Down>(&body).unwrap(),
            Down::Welcome { tools: false }
        );
    }

    #[test]
    fn up_round_trips() {
        let up = Up::Env(EnvReport {
            os: "macos".into(),
            tools: vec![ToolInfo {
                name: "gh".into(),
                version: Some("gh version 2.80.0".into()),
            }],
            ..EnvReport::default()
        });
        let body = rmp_serde::to_vec_named(&up).unwrap();
        assert_eq!(rmp_serde::from_slice::<Up>(&body).unwrap(), up);
    }

    #[test]
    fn stats_round_trip_and_an_older_server_skips_them() {
        let up = Up::Stats(StatsReport {
            disks: vec![DiskUsage {
                path: "/home/user".into(),
                free_bytes: 212_000_000_000,
                total_bytes: 500_000_000_000,
            }],
            memory_used_mb: 9_300,
            memory_total_mb: 32_768,
            load: Some(1.25),
        });
        let body = rmp_serde::to_vec_named(&up).unwrap();
        assert_eq!(rmp_serde::from_slice::<Up>(&body).unwrap(), up);
        // What a server from before `Up::Stats` knew: it fails to read the
        // message, which its control loop skips.
        #[derive(Debug, Deserialize)]
        enum OldUp {
            #[allow(dead_code)]
            Env(EnvReport),
        }
        assert!(rmp_serde::from_slice::<OldUp>(&body).is_err());
    }

    #[test]
    fn capabilities_from_report() {
        let report = EnvReport {
            tools: vec![ToolInfo {
                name: "docker".into(),
                version: None,
            }],
            apps: vec![AppInfo {
                name: AppName::Blender,
                version: None,
            }],
            gpus: vec![GpuInfo {
                vendor: GpuVendor::Nvidia,
                model: "NVIDIA GeForce RTX 4090".into(),
                vram_mb: Some(24564),
                driver: None,
            }],
            ..EnvReport::default()
        };
        assert_eq!(
            report.capabilities(),
            [
                Capability::Gpu,
                Capability::Cuda,
                Capability::Blender,
                Capability::Docker
            ]
        );
        let name: &'static str = Capability::Cuda.into();
        assert_eq!(name, "cuda");
        assert_eq!(AppName::UnrealEngine.to_string(), "Unreal Engine");
    }

    #[test]
    fn a_syncthing_folder_holds_itself_and_what_is_inside() {
        let folder = |id: &str, path: &str| SyncthingFolder {
            id: id.into(),
            label: String::new(),
            path: path.into(),
        };
        let report = EnvReport {
            syncthing: vec![
                folder("work", "/srv/work/"),
                folder("game", "/srv/work/game"),
            ],
            ..EnvReport::default()
        };
        let found = |path| report.syncthing_folder(path).map(|f| f.id.as_str());
        assert_eq!(found("/srv/work"), Some("work"));
        assert_eq!(found("/srv/work/notes"), Some("work"));
        assert_eq!(found("/srv/work/game/src"), Some("game"));
        assert_eq!(found("/srv/workshop"), None);
        assert_eq!(found(""), None);
        let windows = folder("w", r"C:\Users\sam\work");
        assert!(windows.holds(r"C:\Users\sam\work\game"));
        assert_eq!(windows.name(), "w");
    }

    #[test]
    fn docker_desktop_gives_docker() {
        let report = EnvReport {
            apps: vec![AppInfo {
                name: AppName::DockerDesktop,
                version: None,
            }],
            ..EnvReport::default()
        };
        assert_eq!(report.capabilities(), [Capability::Docker]);
    }

    #[test]
    fn old_report_decodes() {
        #[derive(Serialize)]
        struct Older {
            os: &'static str,
            tools: Vec<ToolInfo>,
        }
        let body = rmp_serde::to_vec_named(&Older {
            os: "linux",
            tools: vec![],
        })
        .unwrap();
        let report: EnvReport = rmp_serde::from_slice(&body).unwrap();
        assert!(report.apps.is_empty());
        assert!(report.gpus.is_empty());
    }

    #[test]
    fn unknown_app_decodes_as_unknown() {
        #[derive(Serialize)]
        struct Newer {
            name: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Newer { name: "figma" }).unwrap();
        let app: AppInfo = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(app.name, AppName::Unknown);
    }
}
