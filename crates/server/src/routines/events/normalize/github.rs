//! Normalize GitHub notifications into application events.

use super::{SUMMARY_CHARS, UnsupportedEvent};
use crate::{
    apps::feeds::mail::timestamp,
    memory::memories::clip,
    routines::events::model::{AppEvent, DedupeKey, EventApp, EventKind, Source},
};
use beep_projects::model::RepoName;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Value, json};
use url::Url;

/// GitHub's notification schema and the complete original payload kept with the event.
#[derive(Debug)]
pub struct GitHubNotification {
    fields: GitHubNotificationFields,
    raw: Value,
}

impl<'de> Deserialize<'de> for GitHubNotification {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(deserializer)?;
        let fields = GitHubNotificationFields::deserialize(&raw).map_err(de::Error::custom)?;
        Ok(Self { fields, raw })
    }
}

#[derive(Debug, Deserialize)]
struct GitHubNotificationFields {
    id: GitHubThreadId,
    reason: Option<GitHubReason>,
    subject: Option<GitHubSubject>,
    repository: Option<GitHubRepository>,
    updated_at: Option<GitHubRevision>,
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
struct GitHubThreadId(String);

impl TryFrom<String> for GitHubThreadId {
    type Error = UnsupportedEvent;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        if id.is_empty() {
            return Err(UnsupportedEvent);
        }
        Ok(Self(id))
    }
}

#[derive(Debug, Deserialize, strum::AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
enum GitHubReason {
    ReviewRequested,
    Mention,
    TeamMention,
    Assign,
    CiActivity,
    Author,
    Comment,
    Subscribed,
    #[serde(untagged)]
    #[strum(transparent)]
    Unrecognized(String),
}

#[derive(Debug, Default, Deserialize)]
struct GitHubSubject {
    title: Option<String>,
    #[serde(rename = "type")]
    kind: Option<GitHubSubjectKind>,
    url: Option<Url>,
}

#[derive(Debug, Deserialize, strum::AsRefStr)]
enum GitHubSubjectKind {
    PullRequest,
    Issue,
    CheckSuite,
    #[serde(untagged)]
    #[strum(transparent)]
    Unrecognized(String),
}

#[derive(Debug, Deserialize)]
struct GitHubRepository {
    #[serde(default, deserialize_with = "repository_name")]
    full_name: Option<RepoName>,
}

fn repository_name<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RepoName>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .filter(|name| !name.is_empty())
        .map(|name| name.parse().map_err(de::Error::custom))
        .transpose()
}

/// A notification revision, retaining the timestamp's original spelling for deduplication.
#[derive(Debug, Deserialize)]
#[serde(from = "String")]
struct GitHubRevision {
    token: String,
    occurred: Option<DateTime<Utc>>,
}

impl From<String> for GitHubRevision {
    fn from(token: String) -> Self {
        Self {
            occurred: timestamp(&token),
            token,
        }
    }
}

/// A GitHub notification thread, excluding successful CI runs.
impl TryFrom<&GitHubNotification> for AppEvent {
    type Error = UnsupportedEvent;

    fn try_from(notification: &GitHubNotification) -> Result<Self, Self::Error> {
        let fields = &notification.fields;
        let thread = &fields.id.0;
        let empty_subject = GitHubSubject::default();
        let subject = fields.subject.as_ref().unwrap_or(&empty_subject);
        let title = subject
            .title
            .as_deref()
            .filter(|title| !title.is_empty())
            .unwrap_or("(untitled)");
        let kind_of_thread = subject.kind.as_ref().map(AsRef::as_ref).unwrap_or_default();
        let repo = fields
            .repository
            .as_ref()
            .and_then(|repository| repository.full_name.as_ref())
            .map(ToString::to_string)
            .unwrap_or_else(|| Into::<String>::into("a repository"));
        let updated = fields
            .updated_at
            .as_ref()
            .map(|revision| revision.token.as_str())
            .unwrap_or_default();
        let kind = match fields.reason.as_ref() {
            Some(GitHubReason::ReviewRequested) => EventKind::ReviewRequested,
            Some(GitHubReason::Mention | GitHubReason::TeamMention) => EventKind::Mention,
            Some(GitHubReason::Assign) => EventKind::Assigned,
            Some(GitHubReason::CiActivity) => EventKind::CiFailed,
            Some(GitHubReason::Author | GitHubReason::Comment) => EventKind::ThreadActivity,
            Some(GitHubReason::Subscribed)
                if matches!(subject.kind, Some(GitHubSubjectKind::PullRequest)) =>
            {
                EventKind::PullRequest
            }
            _ => EventKind::GithubOther,
        };
        if kind == EventKind::CiFailed && title.to_ascii_lowercase().contains("succeeded") {
            return Err(UnsupportedEvent);
        }
        let number = subject
            .url
            .as_ref()
            .map(Url::as_str)
            .and_then(|url| url.rsplit('/').next())
            .and_then(|last| last.parse::<u64>().ok());
        let what = match (subject.kind.as_ref(), number) {
            (Some(GitHubSubjectKind::PullRequest), Some(n)) => format!("PR #{n}"),
            (Some(GitHubSubjectKind::Issue), Some(n)) => format!("issue #{n}"),
            (Some(GitHubSubjectKind::CheckSuite), _) => "CI".to_owned(),
            (Some(other), _) if !other.as_ref().is_empty() => other.as_ref().to_owned(),
            _ => "thread".to_owned(),
        };
        let happened = match kind {
            EventKind::ReviewRequested => "review requested on",
            EventKind::Mention => "mentioned in",
            EventKind::Assigned => "assigned",
            EventKind::CiFailed => "failed",
            EventKind::ThreadActivity => "new activity on",
            EventKind::PullRequest => "activity on",
            _ => "notification on",
        };
        let link = match (subject.kind.as_ref(), number) {
            (Some(GitHubSubjectKind::PullRequest), Some(n)) => {
                Some(format!("https://github.com/{repo}/pull/{n}"))
            }
            (Some(GitHubSubjectKind::Issue), Some(n)) => {
                Some(format!("https://github.com/{repo}/issues/{n}"))
            }
            _ => None,
        };
        let mut ids = json!({"repo": repo, "thread_id": thread, "type": kind_of_thread});
        if let Some(n) = number {
            ids["number"] = json!(n);
        }
        if let Some(link) = link {
            ids["url"] = json!(link);
        }
        Ok(AppEvent {
            app: EventApp::Github,
            kind,
            // A thread comes back each time it gets new activity.
            key: DedupeKey::new(&["github", thread, updated]),
            summary: clip(
                &format!("{repo}: {happened} {what} \"{title}\""),
                SUMMARY_CHARS,
            ),
            detail: None,
            ids,
            raw: notification.raw.clone(),
            source: Source::Poll,
            occurred: fields
                .updated_at
                .as_ref()
                .and_then(|revision| revision.occurred)
                .unwrap_or_else(Utc::now),
        })
    }
}
