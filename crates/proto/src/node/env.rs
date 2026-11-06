//! What a node reports about its machine.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

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
