//! Branch reviews.

use std::time::SystemTime;
use uuid::Uuid;

ffi_structs! {
    /// Where a reviewed branch is now, as `CheckBranchReview` reads it.
    pub struct ReviewHead {
        /// The local branch's head, else `origin`'s; `None` when the branch is
        /// gone.
        pub head: Option<String>,
        /// It moved since the review, so its findings may be out of date.
        pub stale: bool,
    }

    /// A review of a branch against its base, as it stands.
    pub struct BranchReview {
        pub id: Uuid,
        /// `org/repo`.
        pub project: String,
        pub branch: String,
        /// The branch it merges into.
        pub base: String,
        /// The commit reviewed.
        pub head: String,
        pub head_at: SystemTime,
        pub started_at: SystemTime,
        /// The machine whose checkout the reviewers read.
        pub machine_name: String,
        /// Lines added and removed.
        pub changed_lines: u32,
        pub files: u32,
        pub merge: ReviewMerge,
        /// The branch's open pull request, when it has one.
        pub pr: Option<ReviewPr>,
        pub shards: Vec<ReviewShard>,
        /// Ranked: open ones by severity first, dismissed then dropped ones
        /// last.
        pub findings: Vec<ReviewFinding>,
    }

    /// A reviewed branch's pull request.
    pub struct ReviewPr {
        pub number: u64,
        pub url: String,
        pub checks: ReviewChecks,
    }
}

ffi_enum! {
    /// How the checks on a pull request stand. `None` when it reports
    /// none or the host does not say.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum ReviewChecks {
        0 None {}
        1 Pending {}
        2 Passing {}
        3 Failing {}
    }
}

ffi_enum! {
    /// Whether the branch merges into its base. `Unknown` when git on the
    /// machine is too old to say.
    #[derive(Debug, Clone, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum ReviewMerge {
        0 Clean {}
        1 Conflicts { files: Vec<String> }
        2 Unknown {}
    }
}

ffi_structs! {
    /// One reviewer's part of the diff.
    pub struct ReviewShard {
        /// The folder its files are under, such as `apps/web`, or `.` for the
        /// files at the top.
        pub area: String,
        pub files: u32,
        pub lines: u32,
        /// The coding agent reading it, such as `claude`.
        pub agent: String,
        pub state: ReviewShardState,
        /// Findings its reviewer reported.
        pub found: u32,
    }
}

ffi_enum! {
    /// Where one shard stands. `Reading` says what the reviewer does now;
    /// `Verifying` waits for the verifier.
    #[derive(Debug, Clone, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum ReviewShardState {
        0 Reading { doing: String }
        1 Verifying {}
        2 Done {}
        3 Failed { why: String }
    }
}

ffi_structs! {
    /// One thing a reviewer found.
    pub struct ReviewFinding {
        /// Unique within its review.
        pub id: u32,
        /// Its shard's area.
        pub area: String,
        pub severity: ReviewSeverity,
        pub title: String,
        pub file: String,
        pub line: Option<u32>,
        /// One line on why it matters.
        pub why: String,
        pub verdict: ReviewVerdict,
        pub dismissed: bool,
    }
}

ffi_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum ReviewSeverity {
        0 High {}
        1 Medium {}
        2 Low {}
    }
}

ffi_enum! {
    /// What the verifier made of a finding. `Unchecked` when it could not
    /// say, so the finding stays.
    #[derive(Debug, Clone, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum ReviewVerdict {
        0 Waiting {}
        1 Verified {}
        2 Dropped { why: String }
        3 Unchecked {}
    }
}
