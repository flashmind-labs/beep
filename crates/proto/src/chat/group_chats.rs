//! Group chats.

ffi_structs! {
    /// A reminder Beep posts into a group when it is due.
    pub struct GroupReminder {
        pub id: String,
        pub label: String,
        /// What Beep posts.
        pub message: String,
        /// RFC 3339 for a one-off, else a cron expression in `set_by`'s timezone.
        pub schedule: String,
        pub next_run_at: Option<String>,
        /// The member who asked for it, by their name in the group.
        pub set_by: String,
    }

    /// A group chat as its list row shows it.
    pub struct Group {
        pub id: String,
        pub name: String,
        /// Oldest first; Beep is not listed.
        pub members: Vec<GroupMember>,
        pub last: Option<GroupMessage>,
        /// When the last message came in, in milliseconds since the Unix epoch.
        pub updated_at: i64,
        /// Beep is writing a reply.
        pub beep_typing: bool,
        /// Pinned to the top of the caller's list.
        #[serde(default)]
        pub pinned: bool,
        /// Beep may use the caller's apps here when other members ask.
        #[serde(default = "shared_by_default")]
        pub apps_shared: bool,
    }
}

fn shared_by_default() -> bool {
    true
}

ffi_structs! {
    pub struct GroupMember {
        pub name: String,
        /// The caller.
        pub you: bool,
        /// Made the group.
        pub creator: bool,
        /// In milliseconds since the Unix epoch.
        pub joined_at: i64,
    }

    /// A group with its latest messages, oldest first.
    pub struct GroupChat {
        pub group: Group,
        pub messages: Vec<GroupMessage>,
    }

    /// One message in a group. Beep's replies hold text only: its thinking and
    /// tool calls never reach the group.
    pub struct GroupMessage {
        /// Grows with each message in the group.
        pub seq: i64,
        pub author: GroupAuthor,
        pub content: String,
        /// In milliseconds since the Unix epoch.
        pub timestamp: i64,
    }
}

ffi_enum! {
    /// Who wrote a group message. `you` marks the caller's own messages.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GroupAuthor {
        0 Member { name: String, you: bool }
        1 Beep {}
    }
}

ffi_structs! {
    /// A group's invite link.
    pub struct GroupLink {
        pub token: String,
        pub url: String,
    }

    /// What an invite shows before joining.
    pub struct GroupPreview {
        pub id: String,
        pub name: String,
        /// Member names, oldest first.
        pub members: Vec<String>,
        /// The caller is in it already.
        pub joined: bool,
    }
}
