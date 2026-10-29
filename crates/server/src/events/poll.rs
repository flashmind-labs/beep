//! What has no Composio trigger is polled on each sweep: GitHub
//! notifications (review requests, mentions, assignments, failed CI and
//! pull requests in watched repositories all show up there, account-wide),
//! Fastmail, and Gmail or Outlook while their trigger is not set up or the
//! realtime channel is down.

use super::{Topic, normalize, submit, triggers};
use crate::{
    app::App,
    apps::connected_slugs,
    feeds::mail::{self, Mailbox},
    store::Feed,
    user::{Topics, User},
};
use anyhow::{Result, bail};
use chrono::{DateTime, Duration as Days, SecondsFormat, Utc};
use serde_json::json;
use std::sync::Arc;
use strum::IntoStaticStr;

/// How far back a source's first poll reads, so connecting does not replay
/// the past.
const FIRST_LOOKBACK: Days = Days::minutes(10);
/// Each poll reads this far before where the last one stopped; the dedupe
/// key drops what it saw already.
const OVERLAP: Days = Days::minutes(2);
const GITHUB_PAGE: u32 = 50;
const GITHUB_MAX_PAGES: u32 = 5;
const GITHUB_TOPICS: [Topic; 7] = [
    Topic::GithubReviews,
    Topic::GithubMentions,
    Topic::GithubAssigned,
    Topic::GithubPullRequests,
    Topic::GithubCi,
    Topic::GithubActivity,
    Topic::GithubOther,
];

/// A polled source, also the key of its cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
enum PollSource {
    #[strum(serialize = "github")]
    Github,
    #[strum(serialize = "mail:gmail")]
    Gmail,
    #[strum(serialize = "mail:outlook")]
    Outlook,
    #[strum(serialize = "mail:fastmail")]
    Fastmail,
}

impl PollSource {
    fn as_str(self) -> &'static str {
        self.into()
    }

    fn mailbox(self) -> Option<Mailbox> {
        match self {
            PollSource::Github => None,
            PollSource::Gmail => Some(Mailbox::Gmail),
            PollSource::Outlook => Some(Mailbox::Outlook),
            PollSource::Fastmail => Some(Mailbox::Fastmail),
        }
    }
}

/// Polls each of `user`'s sources that is connected and wanted.
pub(super) async fn poll_user(app: &Arc<App>, user: &Arc<User>) {
    let topics = match user.event_topics().await {
        Ok(topics) => topics,
        Err(error) => {
            log::warn!("events: unable to read topics: {error:#}; user={user}");
            return;
        }
    };
    for source in sources(app, user, &topics).await {
        let since = match user.polled_at(source.as_str()).await {
            Ok(Some(at)) => at - OVERLAP,
            Ok(None) => Utc::now() - FIRST_LOOKBACK,
            Err(error) => {
                log::warn!("events: unable to read a poll cursor: {error:#}");
                continue;
            }
        };
        let started = Utc::now();
        let polled = match source.mailbox() {
            None => poll_github(app, user, since).await,
            Some(mailbox) => poll_mail(app, user, mailbox, since).await,
        };
        match polled {
            Ok(found) => {
                log::debug!(
                    "events: polled {}; user={user} found={found}",
                    source.as_str()
                );
                if let Err(error) = user.mark_polled(source.as_str(), started).await {
                    log::warn!("events: unable to save a poll cursor: {error:#}");
                }
            }
            // The cursor stays, so the next sweep reads the same stretch.
            Err(error) => log::warn!(
                "events: poll of {} failed: {error:#}; user={user}",
                source.as_str()
            ),
        }
    }
}

/// The sources to poll for `user` now.
async fn sources(app: &App, user: &User, topics: &Topics) -> Vec<PollSource> {
    let mut sources = Vec::new();
    let wants_github = GITHUB_TOPICS.iter().any(|topic| topics.is_on(*topic));
    if wants_github && let Some(client) = user.composio_client() {
        match connected_slugs(&client, user.external_id()).await {
            Ok(slugs) if slugs.contains("github") => sources.push(PollSource::Github),
            Ok(_) => {}
            Err(error) => log::warn!("events: unable to list accounts: {error:#}"),
        }
    }
    if !topics.is_on(Topic::Mail) {
        return sources;
    }
    let realtime = app.events.realtime_up();
    for feed in user.feeds().await {
        let source = match feed {
            Feed::Mail(Mailbox::Fastmail) => PollSource::Fastmail,
            Feed::Mail(Mailbox::Gmail) => PollSource::Gmail,
            Feed::Mail(Mailbox::Outlook) => PollSource::Outlook,
            Feed::Slack => continue,
        };
        let triggered = match source.mailbox() {
            Some(Mailbox::Fastmail) | None => false,
            Some(mailbox) => {
                realtime
                    && triggers::has_mail_trigger(user, mailbox.slug())
                        .await
                        .unwrap_or_else(|error| {
                            log::warn!("events: unable to read triggers: {error:#}");
                            false
                        })
            }
        };
        if !triggered {
            sources.push(source);
        }
    }
    sources
}

/// Queues the GitHub notifications updated since `since`, and returns how
/// many it read.
async fn poll_github(app: &App, user: &User, since: DateTime<Utc>) -> Result<usize> {
    let Some(client) = user.composio_client() else {
        return Ok(0);
    };
    let mut found = 0;
    for page in 1..=GITHUB_MAX_PAGES {
        let arguments = json!({
            "since": since.to_rfc3339_opts(SecondsFormat::Secs, true),
            "per_page": GITHUB_PAGE,
            "page": page,
        });
        let response = client
            .execute_tool("GITHUB_LIST_NOTIFICATIONS", arguments)
            .await?;
        if response.successful == Some(false) {
            bail!("GitHub notifications failed");
        }
        let notifications = response.data["notifications"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let count = notifications.len();
        found += count;
        for notification in &notifications {
            if let Some(event) = normalize::from_github_notification(notification) {
                submit(app, user.id(), event).await;
            }
        }
        if u32::try_from(count).is_ok_and(|count| count < GITHUB_PAGE) {
            break;
        }
    }
    Ok(found)
}

/// Queues the unread mail of `mailbox` received since `since`.
async fn poll_mail(
    app: &App,
    user: &User,
    mailbox: Mailbox,
    since: DateTime<Utc>,
) -> Result<usize> {
    let unread = mail::unread_after(user, mailbox, since).await?;
    let found = unread.len();
    for email in &unread {
        if let Some(event) = normalize::from_email(mailbox, email) {
            submit(app, user.id(), event).await;
        }
    }
    Ok(found)
}
