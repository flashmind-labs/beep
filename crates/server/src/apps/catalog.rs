//! The curated app list and the tools each app gives the agent.

use crate::{apps::feeds::slack::SLACK, apps::permissions::Acts, protocol::AppKind};

/// One entry of the curated list.
pub(crate) struct CatalogApp {
    pub(crate) slug: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: AppKind,
    /// The Composio tools the agent gets; only common ones, since every schema goes out on each request.
    pub(crate) tools: &'static [PickedTool],
}

/// A Composio tool the agent gets, and whether it acts.
pub(crate) struct PickedTool {
    pub(crate) slug: &'static str,
    pub(crate) acts: Acts,
    /// Whether `acts` holds even when Composio's tags say otherwise.
    pub(crate) over_hints: bool,
}

/// A tool that only reads.
pub(crate) const fn read(slug: &'static str) -> PickedTool {
    PickedTool {
        slug,
        acts: Acts::Never,
        over_hints: false,
    }
}

/// A read whose Composio tags wrongly say it creates something.
pub(super) const fn read_over_hints(slug: &'static str) -> PickedTool {
    PickedTool {
        slug,
        acts: Acts::Never,
        over_hints: true,
    }
}

/// A tool that sends, changes or deletes, so it falls under the app's permission level.
pub(super) const fn act(slug: &'static str) -> PickedTool {
    PickedTool {
        slug,
        acts: Acts::Always,
        over_hints: false,
    }
}

pub(super) const fn composio(
    slug: &'static str,
    name: &'static str,
    description: &'static str,
    tools: &'static [PickedTool],
) -> CatalogApp {
    CatalogApp {
        slug,
        name,
        description,
        kind: AppKind::Composio {},
        tools,
    }
}

/// Shown in this order. GitHub, Linear, Todoist and SharePoint are left out (Composio's GitHub was mistaken for gh).
pub(crate) const CATALOG: &[CatalogApp] = &[
    composio(
        "gmail",
        "Gmail",
        "Read, search and send email",
        &[
            read("GMAIL_FETCH_EMAILS"),
            read("GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID"),
            read("GMAIL_GET_ATTACHMENT"),
            act("GMAIL_SEND_EMAIL"),
            act("GMAIL_REPLY_TO_THREAD"),
            act("GMAIL_MOVE_TO_TRASH"),
            act("GMAIL_BATCH_MODIFY_MESSAGES"),
            read("GMAIL_SEARCH_PEOPLE"),
        ],
    ),
    composio(
        "googlecalendar",
        "Google Calendar",
        "See and schedule events",
        &[
            read("GOOGLECALENDAR_EVENTS_LIST_ALL_CALENDARS"),
            act("GOOGLECALENDAR_CREATE_EVENT"),
            act("GOOGLECALENDAR_PATCH_EVENT"),
            act("GOOGLECALENDAR_DELETE_EVENT"),
            read("GOOGLECALENDAR_FREE_BUSY_QUERY"),
        ],
    ),
    composio(
        "notion",
        "Notion",
        "Search and update pages",
        &[
            read("NOTION_SEARCH_NOTION_PAGE"),
            read("NOTION_GET_PAGE_MARKDOWN"),
            act("NOTION_CREATE_NOTION_PAGE"),
            act("NOTION_ADD_MULTIPLE_PAGE_CONTENT"),
            act("NOTION_UPDATE_PAGE"),
            read("NOTION_QUERY_DATABASE"),
            act("NOTION_INSERT_ROW_FROM_NL"),
        ],
    ),
    composio(
        "googledrive",
        "Google Drive",
        "Find and read your files",
        &[
            read("GOOGLEDRIVE_LIST_FILES"),
            read("GOOGLEDRIVE_GET_FILE_METADATA"),
            read("GOOGLEDRIVE_EXPORT_GOOGLE_WORKSPACE_FILE"),
            read("GOOGLEDRIVE_DOWNLOAD_FILE"),
        ],
    ),
    composio(
        "outlook",
        "Outlook",
        "Email, calendar and contacts",
        &[
            read("OUTLOOK_SEARCH_MESSAGES"),
            read("OUTLOOK_GET_MESSAGE"),
            read("OUTLOOK_DOWNLOAD_OUTLOOK_ATTACHMENT"),
            read("OUTLOOK_QUERY_EMAILS"),
            act("OUTLOOK_SEND_EMAIL"),
            act("OUTLOOK_REPLY_EMAIL"),
            act("OUTLOOK_MOVE_MESSAGE"),
            read("OUTLOOK_GET_CALENDAR_VIEW"),
            act("OUTLOOK_CALENDAR_CREATE_EVENT"),
            act("OUTLOOK_ACCEPT_EVENT"),
            act("OUTLOOK_DECLINE_EVENT"),
            act("OUTLOOK_UPDATE_CALENDAR_EVENT"),
            act("OUTLOOK_DELETE_CALENDAR_EVENT"),
        ],
    ),
    composio(
        "microsoft_teams",
        "Microsoft Teams",
        "Read and send messages",
        &[
            read("MICROSOFT_TEAMS_SEARCH_MESSAGES"),
            read("MICROSOFT_TEAMS_CHATS_GET_ALL_CHATS"),
            read("MICROSOFT_TEAMS_GET_CHAT_MESSAGE"),
            read("MICROSOFT_TEAMS_TEAMS_LIST_CHANNEL_MESSAGES"),
            act("MICROSOFT_TEAMS_TEAMS_POST_CHAT_MESSAGE"),
        ],
    ),
    composio(
        "one_drive",
        "OneDrive",
        "Find and read your files",
        &[
            read("ONE_DRIVE_SEARCH_DRIVE_ITEMS"),
            read("ONE_DRIVE_GET_RECENT_ITEMS"),
            read("ONE_DRIVE_GET_SHARED_ITEMS"),
            read("ONE_DRIVE_GET_ITEM"),
            read("ONE_DRIVE_LIST_FOLDER_CHILDREN"),
            read("ONE_DRIVE_GET_DRIVE_ITEM_BY_SHARING_URL"),
            read("ONE_DRIVE_DOWNLOAD_FILE"),
            act("ONE_DRIVE_MOVE_ITEM"),
            act("ONE_DRIVE_ONEDRIVE_CREATE_FOLDER"),
            act("ONE_DRIVE_CREATE_LINK"),
        ],
    ),
    composio(
        "excel",
        "Excel",
        "Read and edit workbooks",
        &[
            read("EXCEL_SEARCH_FILES"),
            read("EXCEL_GET_WORKSHEET_USED_RANGE"),
            read("EXCEL_LIST_TABLE_COLUMNS"),
            act("EXCEL_INSERT_RANGE"),
        ],
    ),
    composio(
        SLACK,
        "Slack",
        "Read and send messages",
        &[
            read("SLACK_SEARCH_MESSAGES"),
            read("SLACK_FETCH_CONVERSATION_HISTORY"),
            read("SLACK_FETCH_MESSAGE_THREAD_FROM_A_CONVERSATION"),
            read("SLACK_LIST_UNREAD_CHANNEL_MESSAGES"),
            read("SLACK_FIND_CHANNELS"),
            read("SLACK_FIND_USERS"),
            // Only opens the DM channel a message to a person needs, so the
            // approval comes once, at the send. Composio tags it as creating.
            read_over_hints("SLACK_OPEN_DM"),
            act("SLACK_SEND_MESSAGE"),
            act("SLACK_ADD_REACTION_TO_AN_ITEM"),
        ],
    ),
    CatalogApp {
        slug: "fastmail",
        name: "Fastmail",
        description: "Email, contacts and calendar",
        kind: AppKind::Fastmail {},
        tools: &[],
    },
];

pub(crate) fn catalog_app(slug: &str) -> Option<&'static CatalogApp> {
    CATALOG.iter().find(|app| app.slug == slug)
}
