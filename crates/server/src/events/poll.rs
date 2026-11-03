//! Sources without a Composio trigger are polled on each sweep: GitHub notifications, Fastmail,
//! and Gmail or Outlook while their trigger or the realtime channel is down.

use super::{Topic, normalize, submit, triggers};
use crate::{
    app::App,
    apps::{catalog_app, connected_slugs},
    feeds::mail::{self, Mailbox},
    store::Feed,
    user::{Topics, User},
};
use anyhow::{Result, bail};
use chrono::{DateTime, Duration as Days, SecondsFormat, Utc};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};
use strum::IntoStaticStr;

/// How far back a source's first poll reads, so connecting does not replay the past.
const FIRST_LOOKBACK: Days = Days::minutes(10);
/// Overlap with the previous poll; the dedupe key drops repeats.
const OVERLAP: Days = Days::minutes(2);
const GITHUB_PAGE: u32 = 50;
/// The most pages of notifications one poll reads.
const GITHUB_MAX_PAGES: u32 = 20;
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

    /// The catalog app the source reads.
    fn app(self) -> &'static str {
        match self.mailbox() {
            Some(mailbox) => mailbox.slug(),
            None => "github",
        }
    }

    /// Whether the source's app is still in the catalog; removed ones are not polled.
    fn listed(self) -> bool {
        catalog_app(self.app()).is_some()
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
    if wants_github
        && PollSource::Github.listed()
        && let Some(client) = user.composio_client()
    {
        match connected_slugs(&client, user.external_id()).await {
            Ok(slugs) if polls_github(&slugs) => sources.push(PollSource::Github),
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
        if !triggered && source.listed() {
            sources.push(source);
        }
    }
    sources
}

/// Whether GitHub notifications are polled for these connected accounts.
fn polls_github(connected: &HashSet<String>) -> bool {
    PollSource::Github.listed() && connected.contains(PollSource::Github.app())
}

/// Queues GitHub notifications updated since `since` and returns how many it read.
async fn poll_github(app: &App, user: &User, since: DateTime<Utc>) -> Result<usize> {
    let Some(client) = user.composio_client() else {
        return Ok(0);
    };
    let since = since.to_rfc3339_opts(SecondsFormat::Secs, true);
    let notifications = read_pages(|page| {
        let client = client.clone();
        let arguments = json!({
            "since": since,
            "per_page": GITHUB_PAGE,
            "page": page,
        });
        async move {
            let response = client
                .execute_tool("GITHUB_LIST_NOTIFICATIONS", arguments)
                .await?;
            if response.successful == Some(false) {
                bail!("GitHub notifications failed");
            }
            Ok(response.data["notifications"]
                .as_array()
                .cloned()
                .unwrap_or_default())
        }
    })
    .await?;
    for notification in &notifications {
        if let Some(event) = normalize::from_github_notification(notification) {
            submit(app, user.id(), event).await;
        }
    }
    Ok(notifications.len())
}

/// Every item from page 1 until an empty page or `GITHUB_MAX_PAGES`; a short page is not the last.
async fn read_pages<F, Fut>(mut fetch: F) -> Result<Vec<Value>>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<Vec<Value>>>,
{
    let mut items = Vec::new();
    for page in 1..=GITHUB_MAX_PAGES {
        let found = fetch(page).await?;
        if found.is_empty() {
            return Ok(items);
        }
        items.extend(found);
    }
    log::warn!("events: stopped after {GITHUB_MAX_PAGES} pages of GitHub notifications");
    Ok(items)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_left_out_of_the_catalog_are_not_polled() {
        let connected = HashSet::from(["github".to_owned(), "gmail".to_owned()]);
        assert!(!polls_github(&connected));
        assert!(!PollSource::Github.listed());
        for source in [PollSource::Gmail, PollSource::Outlook, PollSource::Fastmail] {
            assert!(source.listed(), "{}", source.as_str());
        }
    }

    #[tokio::test]
    async fn every_page_is_read_until_one_is_empty() {
        let pages = [vec![json!(1), json!(2)], vec![json!(3)], vec![json!(4)]];
        let mut asked = Vec::new();
        let read = read_pages(|page| {
            asked.push(page);
            let index = usize::try_from(page - 1).expect("a small page number");
            let found = pages.get(index).cloned().unwrap_or_default();
            async move { Ok(found) }
        })
        .await
        .expect("the pages read");
        assert_eq!(read, [json!(1), json!(2), json!(3), json!(4)]);
        assert_eq!(asked, [1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn reading_stops_at_the_page_limit() {
        let mut asked = 0;
        let read = read_pages(|_| {
            asked += 1;
            let found = vec![json!(asked)];
            async move { Ok(found) }
        })
        .await
        .expect("the pages read");
        assert_eq!(
            read.len(),
            usize::try_from(GITHUB_MAX_PAGES).expect("a small limit")
        );
        assert_eq!(asked, GITHUB_MAX_PAGES);
    }
}
