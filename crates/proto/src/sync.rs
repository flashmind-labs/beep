//! Project sync between machines.

use serde::{Deserialize, Serialize};

/// One project's sync in detail, as `GetProjectSyncDetail` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSyncDetail {
    pub project: String,
    pub pinned: bool,
    /// Paused on every machine with `PauseProjectSync`.
    pub paused: bool,
    pub machines: Vec<MachineSyncDetail>,
}

/// One machine's sync of a project in detail.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineSyncDetail {
    /// How it stands, as `ListProjectSync` gives it. While paused its
    /// status is `InSync`, or `Offline` when it does not answer.
    pub sync: MachineSync,
    /// The machine reports itself paused.
    pub paused: bool,
    /// What it is bringing in line now, while it syncs.
    pub progress: Option<SyncProgress>,
    /// Some paths it still has to bring, or that failed and are tried
    /// again, at most 50.
    pub waiting: Vec<String>,
    /// Its beep-node can settle conflicts with `ResolveSyncConflict`.
    pub can_resolve: bool,
}

/// How far a machine is through what it brings in line now.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncProgress {
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Bytes a second over the last few seconds; 0 before it can tell.
    pub rate: u64,
    /// Seconds left at that rate; `None` before it can tell.
    pub eta_secs: Option<u32>,
}

wire_enum! {
    /// Which side of a conflict `ResolveSyncConflict` keeps: the file the
    /// copy stands beside (`Original`), or the conflict copy (`Copy`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ConflictKeep {
        0 Original {}
        1 Copy {}
    }
}

/// A machine whose folder of a project Syncthing keeps in step, as
/// `ListProjectSyncthing` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncthingPlace {
    /// The `Machine::id` it is.
    pub machine_id: String,
    /// The project's folder there.
    pub folder: String,
    /// The Syncthing folder's label there, or its ID without one.
    pub label: String,
}

/// One project's sync across the caller's machines.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSync {
    pub project: String,
    pub pinned: bool,
    pub machines: Vec<MachineSync>,
}

/// How one machine stands with a project it syncs.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineSync {
    /// The `Machine::id` it is.
    pub machine_id: String,
    pub machine_name: String,
    pub folder: String,
    pub status: SyncStatus,
    /// Files kept in step, and their bytes.
    pub files: u64,
    pub bytes: u64,
    /// Files another machine changed that this one does not have yet.
    pub pending: u64,
    /// The copies two machines' edits of one file left, by path in the
    /// folder, such as `notes (conflict 1a2b3c4d).txt`.
    pub conflicts: Vec<String>,
    /// Set when the machine rescans the folder this often instead of
    /// watching it, because its file watch limit is reached.
    pub rescan_secs: Option<u32>,
    /// What raises that limit, such as a `sysctl` line.
    pub hint: Option<String>,
    /// Why the machine's last scan or fetch failed.
    pub error: Option<String>,
}

wire_enum! {
    /// Where a machine's sync of a project stands: `Offline` (offline or
    /// did not answer), `Scanning` (looking for changes on disk),
    /// `Syncing` (fetching or writing what another machine changed),
    /// `InSync`, or `Failed` (`MachineSync::error` says why).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum SyncStatus {
        0 Offline {}
        1 Scanning {}
        2 Syncing {}
        3 InSync {}
        4 Failed {}
    }
}

/// What a project's sync leaves out on one machine.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncSkipped {
    pub paths: Vec<SkippedFile>,
    /// Files in a git repository that git carries between machines rather
    /// than sync.
    pub through_git: u64,
    /// More paths are left out than `paths` lists.
    pub truncated: bool,
}

/// A file or folder sync leaves out, and the rule that does.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkippedFile {
    /// Relative to the project folder, parts joined with `/`.
    pub path: String,
    pub folder: bool,
    pub rule: SkipRule,
}

wire_enum! {
    /// Why sync leaves a path out, in the order the rules apply: `Git`
    /// (`.git`, and in a git repository the files git carries), the
    /// project's `.gitignore`, a `Default` for the kind of project its
    /// `marker` file shows (such as `target/` for `Cargo.toml`), the
    /// project's `.beepignore`, or a symlink or special file (`Link`).
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum SkipRule {
        0 Git {}
        1 GitIgnore {}
        2 Default { marker: String }
        3 BeepIgnore {}
        4 Link {}
    }
}
