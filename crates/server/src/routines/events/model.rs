//! The one shape every account event takes, whichever way it arrived.

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fmt;
use strum::{EnumString, IntoStaticStr};

/// The app an event happened in; its slug is the catalog's, so its permission level applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
pub enum EventApp {
    #[strum(serialize = "gmail")]
    Gmail,
    #[strum(serialize = "outlook")]
    Outlook,
    #[strum(serialize = "fastmail")]
    Fastmail,
    #[strum(serialize = "googlecalendar")]
    GoogleCalendar,
    #[strum(serialize = "github")]
    Github,
}

impl EventApp {
    pub fn slug(self) -> &'static str {
        self.into()
    }

    /// How the user knows the app.
    pub fn name(self) -> &'static str {
        match self {
            EventApp::Gmail => "Gmail",
            EventApp::Outlook => "Outlook",
            EventApp::Fastmail => "Fastmail",
            EventApp::GoogleCalendar => "Google Calendar",
            EventApp::Github => "GitHub",
        }
    }
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EventKind {
    NewMail,
    CalendarCreated,
    CalendarChanged,
    CalendarCancelled,
    CalendarSoon,
    ReviewRequested,
    Mention,
    Assigned,
    /// Activity on a pull request in a watched repository.
    PullRequest,
    CiFailed,
    /// Comments or reviews on something the user opened or commented on.
    ThreadActivity,
    GithubOther,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// The topic the user turns this kind on or off with.
    pub fn topic(self) -> Topic {
        match self {
            EventKind::NewMail => Topic::Mail,
            EventKind::CalendarCreated
            | EventKind::CalendarChanged
            | EventKind::CalendarCancelled => Topic::CalendarChanges,
            EventKind::CalendarSoon => Topic::CalendarUpcoming,
            EventKind::ReviewRequested => Topic::GithubReviews,
            EventKind::Mention => Topic::GithubMentions,
            EventKind::Assigned => Topic::GithubAssigned,
            EventKind::PullRequest => Topic::GithubPullRequests,
            EventKind::CiFailed => Topic::GithubCi,
            EventKind::ThreadActivity => Topic::GithubActivity,
            EventKind::GithubOther => Topic::GithubOther,
        }
    }

    /// Whether this kind alone suggests urgency; used when triage fails.
    pub fn looks_urgent(self) -> bool {
        matches!(
            self,
            EventKind::ReviewRequested
                | EventKind::Mention
                | EventKind::Assigned
                | EventKind::CiFailed
                | EventKind::CalendarSoon
                | EventKind::CalendarCancelled
        )
    }
}

/// A group of events the user turns on or off, by asking Beep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
pub enum Topic {
    #[strum(serialize = "mail")]
    Mail,
    #[strum(serialize = "calendar_changes")]
    CalendarChanges,
    #[strum(serialize = "calendar_upcoming")]
    CalendarUpcoming,
    #[strum(serialize = "github_reviews")]
    GithubReviews,
    #[strum(serialize = "github_mentions")]
    GithubMentions,
    #[strum(serialize = "github_assigned")]
    GithubAssigned,
    #[strum(serialize = "github_pull_requests")]
    GithubPullRequests,
    #[strum(serialize = "github_ci")]
    GithubCi,
    #[strum(serialize = "github_activity")]
    GithubActivity,
    #[strum(serialize = "github_other")]
    GithubOther,
}

impl Topic {
    pub const ALL: [Topic; 10] = [
        Topic::Mail,
        Topic::CalendarChanges,
        Topic::CalendarUpcoming,
        Topic::GithubReviews,
        Topic::GithubMentions,
        Topic::GithubAssigned,
        Topic::GithubPullRequests,
        Topic::GithubCi,
        Topic::GithubActivity,
        Topic::GithubOther,
    ];

    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Whether a user who never chose gets it.
    pub fn default_on(self) -> bool {
        self != Topic::GithubOther
    }

    pub fn describe(self) -> &'static str {
        match self {
            Topic::Mail => "New mail in Gmail, Outlook or Fastmail, leaving out promotions",
            Topic::CalendarChanges => "Calendar events created, changed or cancelled",
            Topic::CalendarUpcoming => "Calendar events starting within half an hour",
            Topic::GithubReviews => "GitHub review requests",
            Topic::GithubMentions => "GitHub mentions of the user or their team",
            Topic::GithubAssigned => "GitHub issues and pull requests assigned to the user",
            Topic::GithubPullRequests => {
                "Pull requests opened or updated in repositories the user watches"
            }
            Topic::GithubCi => "Failed GitHub Actions runs on the user's work",
            Topic::GithubActivity => {
                "Comments and reviews on GitHub threads the user opened or joined"
            }
            Topic::GithubOther => "Every other GitHub notification",
        }
    }
}

/// How an event reached the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Source {
    /// A Composio trigger, delivered over its realtime channel.
    Trigger,
    /// One of the server's own polls.
    Poll,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// What makes two events the same happening, such as `gmail:<message id>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupeKey(String);

impl DedupeKey {
    pub fn new(parts: &[&str]) -> Self {
        Self(parts.join(":"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DedupeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One thing that happened in a user's account.
#[derive(Debug, Clone)]
pub struct AppEvent {
    pub app: EventApp,
    pub kind: EventKind,
    pub key: DedupeKey,
    /// One line for the agent, such as `From Ana: Lease renewal`.
    pub summary: String,
    /// A little more, such as the start of the email, when there is any.
    pub detail: Option<String>,
    /// What the agent passes to the app's tools to act on it.
    pub ids: Value,
    /// The payload as it came. It stays on the server.
    pub raw: Value,
    pub source: Source,
    pub occurred: DateTime<Utc>,
}
