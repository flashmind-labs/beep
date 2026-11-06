//! What the assistant keeps for the user: what Beep remembers about them,
//! the skills it learns, and crons with the places they can start on.

ffi_structs! {
    pub struct Memory {
        pub id: String,
        pub content: String,
        pub category: String,
        pub source: MemorySource,
        /// `YYYY-MM-DD` of the last change.
        pub updated: String,
        /// `YYYY-MM-DD` bounds of a fact that only holds for a while, like a trip.
        pub valid_from: Option<String>,
        pub valid_until: Option<String>,
        /// The words in the chat, email or Slack message the memory came from.
        #[serde(default)]
        pub quote: Option<String>,
    }

    /// One read of a mailbox or calendar, and the memories it added or
    /// confirmed. A memory a later read confirms moves to that read.
    pub struct MemoryRead {
        pub id: String,
        /// The app that was read, such as "Gmail" or "Outlook calendar".
        pub source: String,
        /// RFC 3339 of when the read finished.
        pub finished_at: String,
        /// The `Memory` ids, in the order `ListMemories` gives them.
        pub memories: Vec<String>,
    }
}

ffi_enum! {
    /// Who a memory came from: the user saying so, Beep picking it up from a
    /// chat, or Beep reading it in the user's email or Slack.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MemorySource {
        0 User {}
        1 Learned {}
        2 Email {}
        3 Slack {}
    }
}

impl MemorySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User {} => "user",
            Self::Learned {} => "learned",
            Self::Email {} => "email",
            Self::Slack {} => "slack",
        }
    }

    /// Reads `as_str`; anything else, including the old `auto`, is learned.
    pub fn parse(text: &str) -> Self {
        match text {
            "user" => Self::User {},
            "email" => Self::Email {},
            "slack" => Self::Slack {},
            _ => Self::Learned {},
        }
    }
}

ffi_structs! {
    /// A way of doing something that Beep learned from how the user, or a
    /// group, asks for it. Beep writes and updates them on its own.
    pub struct Skill {
        pub id: String,
        pub name: String,
        /// One line on when it applies.
        pub description: String,
        /// The instructions, in Markdown.
        pub body: String,
        /// The group it belongs to; both `None` for the user's own.
        pub group_id: Option<String>,
        pub group_name: Option<String>,
        /// A paused skill stays, but Beep does not use it.
        pub enabled: bool,
        /// How many times Beep used it.
        pub uses: u32,
        /// RFC 3339.
        pub last_used: Option<String>,
        /// RFC 3339 of the last change.
        pub updated_at: String,
        /// Whether the user changed it last, rather than Beep.
        pub edited: bool,
        /// Whether an earlier version is kept for `UndoSkill`.
        pub can_undo: bool,
    }

    /// A memory that is about something other than the user.
    pub struct ScopedMemory {
        /// The `Memory::id`.
        pub id: String,
        pub scope: MemoryScope,
    }
}

ffi_enum! {
    /// What a memory is about when it is not the user: a machine by its key,
    /// with its name while the machine is known; a project, such as a
    /// repository, by the name Beep knows it by; or one of the user's
    /// accounts by app, such as `github`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum MemoryScope {
        0 Machine { id: String, name: Option<String> }
        1 Project { name: String }
        2 Account { app: String }
    }
}

ffi_enum! {
    /// Arriving at a saved place or leaving it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PlaceCrossing {
        0 Arrive {}
        1 Leave {}
    }
}

ffi_structs! {
    /// A place the user saved by name, such as Home or Gym, that routines can
    /// start on arriving at or leaving.
    pub struct Place {
        pub id: String,
        pub name: String,
        pub latitude: f64,
        pub longitude: f64,
        /// Within this many metres counts as being there.
        pub radius: u32,
        /// A street address or area name, when known.
        pub address: Option<String>,
        pub created_at: String,
        /// Whether the phone's last location was there.
        pub here: bool,
        /// When the phone was last there.
        pub last_here_at: Option<String>,
        /// How many routines start on arriving at or leaving it.
        pub routines: u32,
    }

    pub struct Cron {
        pub id: String,
        pub label: String,
        pub prompt: String,
        /// RFC 3339 for a one-off, a place trigger as `CreateCron` describes,
        /// else a cron expression.
        pub schedule: String,
        pub enabled: bool,
        pub created_at: String,
        pub run_once: bool,
        pub last_run_at: Option<String>,
        pub next_run_at: Option<String>,
    }
}
