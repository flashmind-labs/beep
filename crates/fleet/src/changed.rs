use beep_proto::{HostedRuns, LabeledProjectLists};

/// What a background read changed, for the page that shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum Changed {
    Project {
        /// `org/repo`.
        project: String,
        read: ProjectRead,
    },
    /// A node that pushes no sync status said something new of it.
    Sync,
    /// The plans and spending: `Session::spend` gives more than before.
    Usage,
}

/// What a background read of a project found.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectRead {
    Lists(LabeledProjectLists),
    Runs(HostedRuns),
}
