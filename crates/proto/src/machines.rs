//! The user's machines: their state, tools, updates and terminals.

use crate::{AttachTerminal, Duplex, node};
use std::time::SystemTime;
use uuid::Uuid;

ffi_structs! {
    /// A tool on a machine with a newer release (`ListToolUpdates`).
    #[derive(Eq)]
    pub struct ToolUpdate {
        /// Its `Machine::id`.
        pub machine_id: String,
        /// Such as `gh`.
        pub tool: String,
        /// The version the machine runs.
        pub version: String,
        /// The newest release.
        pub latest: String,
    }

    /// `AddMachineOverSsh`'s answer, which is always a failure now.
    #[derive(Eq)]
    pub struct SshAdd {
        /// Always `None`.
        pub machine_id: Option<String>,
        /// Always empty.
        pub system: String,
        /// Why it did nothing.
        pub error: String,
        /// Always empty.
        pub output: Vec<String>,
    }

    /// A machine `UpdateMachine` can update (`ListUpdatableMachines`).
    #[derive(Eq)]
    pub struct UpdatableMachine {
        /// Its `Machine::id`.
        pub id: String,
        /// The agent sessions running on it, which an update waits for.
        pub agent_sessions: u32,
    }

    /// What `UpdateMachine` did.
    #[derive(Eq)]
    pub struct MachineUpdate {
        /// The beep-node version it ran.
        pub from: String,
        /// The version installed, or `from` when that is already the newest.
        pub to: String,
        /// It restarts on `to`: it goes offline and connects again shortly.
        pub restarting: bool,
        /// Why it does not restart, in words for the user; empty otherwise.
        pub note: String,
    }

    /// The machine Auto would start a coding agent on, as
    /// `PreviewAgentMachine` gives it.
    pub struct MachinePick {
        pub machine_id: String,
        pub machine_name: String,
        /// Why, such as "It has Xcode and runs the fewest sessions."
        pub why: String,
    }

    /// A command kept running on one of the user's machines, for a project or
    /// on its own.
    pub struct Terminal {
        pub id: Uuid,
        /// `org/repo`, or empty for a terminal of no project.
        pub project: String,
        pub machine_id: String,
        pub machine_name: String,
        pub name: String,
        pub command: String,
        /// The folder it runs in.
        pub cwd: String,
        pub state: TerminalState,
        pub started_at: SystemTime,
    }
}

ffi_enum! {
    /// How a terminal stands. `Exited` ended on its own (`code` is `None`
    /// when a signal ended it); `Offline` is not known while its machine
    /// is offline; `Gone` is no longer on its machine, such as after a
    /// restart.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum TerminalState {
        0 Running {}
        1 Exited { code: Option<i32> }
        2 Stopped {}
        3 Offline {}
        4 Gone {}
    }
}

ffi_structs! {
    /// A terminal and the last of its output, as `GetTerminal` gives it.
    pub struct TerminalOutput {
        pub terminal: Terminal,
        /// Oldest first.
        pub lines: Vec<String>,
        /// Lines were dropped before these.
        pub earlier: bool,
    }
}

impl Duplex for AttachTerminal {
    type Up = TerminalInput;
}

wire_enum! {
    /// What `AttachTerminal` streams. `Output` is the terminal's output
    /// from byte `at`, raw, escape sequences and all; an `at` past the
    /// asked `from` means the bytes in between were no longer kept.
    /// `Ended` says how it stood when the stream ended: it stopped, or its
    /// machine went offline. `ViewOnly` is a terminal that takes no input,
    /// to read with `GetTerminal` instead.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum TerminalEvent {
        0 Output { at: u64, data: serde_bytes::ByteBuf }
        1 Ended { state: TerminalState }
        2 ViewOnly {}
    }
}

wire_enum! {
    /// What the client sends after `AttachTerminal`: keys typed, at most
    /// `MAX_TERMINAL_INPUT` bytes at a time, or the terminal's new size.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum TerminalInput {
        0 Keys { data: serde_bytes::ByteBuf }
        1 Resize { cols: u32, rows: u32 }
    }
}

/// The most one `TerminalInput::Keys` may carry.
pub const MAX_TERMINAL_INPUT: usize = node::terminal::MAX_INPUT;

#[cfg(test)]
impl crate::compat::Sample for serde_bytes::ByteBuf {
    fn samples(_: u32) -> Vec<Self> {
        vec![serde_bytes::ByteBuf::from(
            b"$ ls\r\n\x1b[32mok\x1b[0m".to_vec(),
        )]
    }
}

ffi_structs! {
    /// A folder on one of the caller's machines, as `ListMachineFolder` reads it.
    pub struct MachineFolder {
        /// Its absolute path on the machine.
        pub path: String,
        /// The folder above it; `None` at the root.
        pub parent: Option<String>,
        /// `false` for a folder that is not there yet; an import makes it.
        pub exists: bool,
        /// The folders in it by name, sorted, hidden ones left out.
        pub folders: Vec<String>,
        /// Folders left out past the limit.
        pub more: u32,
        /// It holds nothing, hidden files included.
        pub empty: bool,
        /// It is a git checkout: its `origin` remote, empty when it has none.
        pub origin: Option<String>,
    }

    /// A setup token as `CreateNodeToken` made it.
    pub struct NodeToken {
        /// The token as text, such as `bk1-0f3ka-...`.
        pub token: String,
        pub expires_at: SystemTime,
    }

    /// The apps, graphics cards and abilities of one machine, as
    /// `ListMachineDetails` lists them.
    pub struct MachineDetails {
        /// The `Machine::id` it describes.
        pub id: String,
        pub apps: Vec<MachineApp>,
        pub gpus: Vec<MachineGpu>,
        pub capabilities: Vec<MachineCapability>,
    }

    /// An app installed on a machine.
    pub struct MachineApp {
        /// Its display name, such as "Blender" or "Unreal Engine 5.4".
        pub name: String,
        pub version: Option<String>,
    }

    /// A graphics card in a machine.
    pub struct MachineGpu {
        /// Its maker, such as "NVIDIA" or "Apple".
        pub vendor: String,
        pub model: String,
        /// Video memory in MB, when known.
        pub vram_mb: Option<u64>,
        pub driver: Option<String>,
    }

    /// A machine's live resources, as `ListMachineStats` lists them.
    pub struct MachineStats {
        /// The `Machine::id` it describes.
        pub id: String,
        /// When the server got the sample.
        pub sampled_at: SystemTime,
        /// The workspace's volume first, when the machine keeps one, then the
        /// home folder's when it is on another volume.
        pub disks: Vec<MachineDisk>,
        pub memory_used_mb: u64,
        pub memory_total_mb: u64,
        /// The load average over the last minute, when the machine gives one.
        pub load: Option<f64>,
    }

    /// What a machine moved over iroh since it first reported, as
    /// `ListMachineTraffic` lists it.
    pub struct MachineTraffic {
        /// The `Machine::id` it describes.
        pub id: String,
        /// When the server last got a report.
        pub updated_at: SystemTime,
        /// Key and value, so a new counter needs no new shape.
        pub counters: Vec<TrafficCounter>,
    }

    /// One lifetime total, such as `alpn.beep-sync/1.tx` or `endpoint.relay.rx`.
    pub struct TrafficCounter {
        pub key: String,
        pub bytes: u64,
    }

    /// How full the volume holding one folder is.
    pub struct MachineDisk {
        /// The folder it was measured at.
        pub path: String,
        /// What a user may still write, in bytes.
        pub free_bytes: u64,
        pub total_bytes: u64,
    }
}

ffi_enum! {
    /// What a machine can be asked to do.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    pub enum MachineCapability {
        0 Gpu {}
        1 Cuda {}
        2 Blender {}
        3 Unreal {}
        4 Xcode {}
        5 Android {}
        6 Docker {}
    }
}

impl MachineCapability {
    /// Its name for people, such as "Xcode" or "GPU".
    pub fn label(self) -> &'static str {
        match self {
            Self::Gpu {} => "a GPU",
            Self::Cuda {} => "CUDA",
            Self::Blender {} => "Blender",
            Self::Unreal {} => "Unreal Engine",
            Self::Xcode {} => "Xcode",
            Self::Android {} => "Android tools",
            Self::Docker {} => "Docker",
        }
    }
}

ffi_structs! {
    /// A machine running beep-node, and what it last reported.
    pub struct Machine {
        /// Its key, the `LinkedDevice::id` it signs in with.
        pub id: String,
        pub name: String,
        /// Connected to the server now.
        pub online: bool,
        /// When it last connected or reported.
        pub seen_at: SystemTime,
        /// The beep-node version, such as "0.1.0".
        pub version: String,
        /// `None` until its first report.
        pub env: Option<MachineEnv>,
    }

    /// What a machine reported about itself.
    pub struct MachineEnv {
        /// `linux` or `macos`.
        pub os: String,
        /// Such as "Ubuntu 24.04" or "macOS 26.3".
        pub os_version: String,
        pub arch: String,
        pub hostname: String,
        pub cpus: u32,
        pub memory_mb: u64,
        /// The folder commands start in.
        pub home: String,
        pub tools: Vec<MachineTool>,
        pub logins: Vec<MachineLogin>,
        /// Android virtual devices.
        pub avds: Vec<String>,
    }

    /// A known tool found on a machine's `PATH`.
    pub struct MachineTool {
        pub name: String,
        /// The first line of its version output.
        pub version: Option<String>,
    }

    /// Whether a CLI on a machine is signed in, such as `gh`.
    pub struct MachineLogin {
        pub tool: String,
        pub signed_in: bool,
        pub account: Option<String>,
    }
}
