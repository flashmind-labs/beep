//! Learns about users from their Slack.
//!
//! Once a day the messages of the last day are read with one search across
//! every conversation the user is in. Nothing older is read, not even on the
//! first run. Only what concerns the user is kept: each message they wrote,
//! each message that mentions them and every direct message is an anchor,
//! and a window of the messages around it gives the context. Windows that
//! touch merge, and long ones split, so a busy day becomes several short
//! reads instead of one cut off. Jev scores each window, and the likely ones
//! go to the memory learner, oldest first. Authors and mentions are shown by
//! their Slack names.
//!
//! Processed message ids are kept so the overlap between runs is not read
//! twice. Message content is never logged.

use crate::{
    app::App,
    decision::{Criteria, DecisionTool},
    memories::{Reading, ReadingKind},
    store::Feed,
    user::User,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as Days, Utc};
use flashmind::tools::composio::ComposioClient;
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::{self, Write},
    ops::Range,
};

/// The Composio toolkit slug, also the key of its sync state.
pub const SLACK: &str = "slack";
/// How far back each run reads. Runs are a day apart, so this overlaps the
/// previous run; seen ids skip what it already read.
const LOOKBACK_DAYS: i64 = 2;
/// Jev calls, and name lookups, in flight at once.
const CONCURRENCY: usize = 8;
/// Slack's search page limit.
const PAGE_SIZE: u64 = 100;
/// Two days of a busy workspace fit well within this.
const MAX_PAGES: u64 = 30;
/// Messages before an anchor, for what it answers.
const CONTEXT_BEFORE: usize = 3;
/// Messages after an anchor, for the replies.
const CONTEXT_AFTER: usize = 2;
/// A merged window longer than this is split.
const MAX_WINDOW_MESSAGES: usize = 25;
/// What Jev sees of each window.
const JEV_WINDOW_CHARS: usize = 3000;
/// What the learner reads of a window Jev passed.
const LEARN_WINDOW_CHARS: usize = 5000;
/// Above this Jev score the learner reads the window.
const MEMORY_THRESHOLD: f64 = 0.8;
const SLACK_QUESTION: &str =
    "Does this Slack conversation show a fact about the user still worth remembering today?";
const SLACK_CRITERIA: Criteria = Criteria {
    yes: "A lasting fact about the user: their name, people in their life, where they live or work, \
their role, team or projects, what they like, or a trip or appointment still ahead; or a request to the \
user with a date or deadline still ahead",
    no: "Status updates, quick questions, links, jokes, small talk, tasks without a date, \
events already over, or facts only about other people",
};

/// One message of a search result.
#[derive(Debug, Clone)]
struct Message {
    /// Channel and timestamp, unique across the workspace.
    id: String,
    channel: String,
    /// Shown for channels; direct messages have no useful name.
    channel_name: String,
    direct: bool,
    at: DateTime<Utc>,
    /// The Slack handle, used when the name lookup fails.
    handle: String,
    author_id: String,
    text: String,
}

/// Slack names by user id, with the handle or id as fallback.
#[derive(Debug, Default)]
struct Names(HashMap<String, String>);

impl Names {
    fn get<'a>(&'a self, id: &'a str) -> &'a str {
        self.0.get(id).map_or(id, String::as_str)
    }

    /// `text` with each `<@U123>` or `<@U123|label>` written as `@Name`.
    fn resolve(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("<@") {
            let Some(end) = rest[start..].find('>') else {
                break;
            };
            out.push_str(&rest[..start]);
            let inner = &rest[start + 2..start + end];
            let id = inner.split('|').next().unwrap_or(inner);
            out.push('@');
            out.push_str(self.get(id));
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }
}

/// The user ids mentioned in `text`.
fn mentions(text: &str) -> impl Iterator<Item = &str> {
    text.split("<@")
        .skip(1)
        .filter_map(|part| part.split_once('>'))
        .map(|(inner, _)| inner.split('|').next().unwrap_or(inner))
}

/// A stretch of one conversation around messages that concern the user.
#[derive(Debug)]
struct Window {
    name: String,
    direct: bool,
    /// Whose messages are the user's.
    me: String,
    messages: Vec<Message>,
}

impl Window {
    fn started(&self) -> DateTime<Utc> {
        self.messages.first().map_or_else(Utc::now, |m| m.at)
    }

    /// For the log; never content.
    fn id(&self) -> String {
        let first = self.messages.first().map_or("", |m| m.id.as_str());
        first.to_owned()
    }

    /// A header and one line per message, clipped to `chars`.
    fn render(&self, names: &Names, chars: usize) -> String {
        let mut text = String::new();
        if self.direct {
            let _ = writeln!(text, "Slack direct messages");
        } else {
            let _ = writeln!(text, "Slack channel #{}", self.name);
        }
        let mut day = None;
        for message in &self.messages {
            let date = message.at.date_naive();
            if day != Some(date) {
                let _ = writeln!(text, "\n{}", message.at.format("%A %Y-%m-%d"));
                day = Some(date);
            }
            let author = names.get(&message.author_id);
            let marker = if message.author_id == self.me {
                " (the user)"
            } else {
                ""
            };
            let _ = writeln!(
                text,
                "{} {author}{marker}: {}",
                message.at.format("%H:%M UTC"),
                names.resolve(message.text.trim())
            );
        }
        text.chars().take(chars).collect()
    }

    fn reading(&self, names: &Names) -> Reading {
        let query: Vec<String> = self
            .messages
            .iter()
            .map(|m| names.resolve(&m.text))
            .collect();
        Reading {
            kind: ReadingKind::Slack,
            id: self.id(),
            query: query.join("\n").chars().take(300).collect(),
            text: self.render(names, LEARN_WINDOW_CHARS),
        }
    }
}

/// How a run went.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    listed: usize,
    windows: usize,
    passed: usize,
    learned: usize,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Outcome {
            listed,
            windows,
            passed,
            learned,
        } = self;
        write!(
            f,
            "listed={listed} windows={windows} passed={passed} learned={learned}"
        )
    }
}

pub(crate) async fn sync(app: &App, decision: &DecisionTool, user: &User) -> Result<Outcome> {
    let client = user
        .composio_client()
        .context("Composio is not configured")?;
    let after = Utc::now() - Days::days(LOOKBACK_DAYS);
    user.prune_feed_seen(Feed::Slack, after).await?;
    let seen = user.feed_seen(Feed::Slack).await?;

    let Some(me) = own_id(&client).await? else {
        log::info!("slack: the user never wrote a message; user={user}");
        return Ok(Outcome::default());
    };
    let listed = list(&client, after).await?;
    let mut outcome = Outcome {
        listed: listed.len(),
        ..Outcome::default()
    };
    let fresh = listed.into_iter().filter(|m| !seen.contains(&m.id));
    let windows = windows(fresh, &me);
    outcome.windows = windows.len();
    log::info!(
        "slack: {} windows from {} messages after {after}; user={user}",
        outcome.windows,
        outcome.listed
    );
    let names = names(&client, &windows).await;

    let today = Utc::now().format("%A %Y-%m-%d").to_string();
    let mut scored = stream::iter(windows)
        .map(|window| score(decision, &names, &today, window))
        .buffered(CONCURRENCY);
    let generator = app.generator(user.id());
    while let Some((window, score)) = scored.next().await {
        let probability = score.context("Jev failed")?;
        log::debug!(
            "slack: scored {probability:.2}; user={user} id={}",
            window.id()
        );
        if probability >= MEMORY_THRESHOLD {
            outcome.passed += 1;
            outcome.learned += user
                .learn_reading(generator, &window.reading(&names))
                .await
                .len();
        }
        for message in &window.messages {
            user.mark_feed_seen(Feed::Slack, &message.id, message.at)
                .await?;
        }
    }
    Ok(outcome)
}

async fn score(
    decision: &DecisionTool,
    names: &Names,
    today: &str,
    window: Window,
) -> (Window, Result<f64>) {
    let state = format!("Today: {today}\n{}", window.render(names, JEV_WINDOW_CHARS));
    let score = decision
        .probability_with(&state, SLACK_QUESTION, &SLACK_CRITERIA)
        .await;
    (window, score)
}

/// Runs one Slack Web API tool through Composio and returns Slack's reply.
async fn slack(client: &ComposioClient, tool: &str, arguments: Value) -> Result<Value> {
    let response = client.execute_tool(tool, arguments).await?;
    if response.successful == Some(false) {
        bail!("Slack call failed");
    }
    if response.data["ok"] == json!(false) {
        // Slack's error codes, like missing_scope, carry no content.
        bail!("Slack call failed: {}", text(&response.data["error"]));
    }
    Ok(response.data)
}

/// Runs one Slack search and returns its `messages` object.
async fn search(client: &ComposioClient, query: &str, count: u64, page: u64) -> Result<Value> {
    let arguments = json!({
        "query": query,
        "count": count,
        "page": page,
        "sort": "timestamp",
        "sort_dir": "asc",
    });
    let mut data = slack(client, "SLACK_SEARCH_MESSAGES", arguments).await?;
    Ok(data["messages"].take())
}

/// The user's own Slack id, from a message they wrote. None when there is
/// none, and then there is nothing to learn either.
async fn own_id(client: &ComposioClient) -> Result<Option<String>> {
    let messages = search(client, "from:me", 1, 1).await?;
    Ok(messages["matches"]
        .as_array()
        .and_then(|matches| matches.first())
        .and_then(|m| m["user"].as_str())
        .map(str::to_owned))
}

/// Every message by a person after `after`, oldest first.
async fn list(client: &ComposioClient, after: DateTime<Utc>) -> Result<Vec<Message>> {
    // Slack's `after:` takes a day and leaves that day out.
    let query = format!("after:{}", (after - Days::days(1)).format("%Y-%m-%d"));
    let mut messages = Vec::new();
    for page in 1..=MAX_PAGES {
        let result = search(client, &query, PAGE_SIZE, page)
            .await
            .with_context(|| format!("page {page}"))?;
        let pages = result["paging"]["pages"].as_u64().unwrap_or(1);
        messages.extend(parse_matches(&result).into_iter().filter(|m| m.at >= after));
        if page >= pages {
            break;
        }
        if page == MAX_PAGES {
            log::warn!("slack: stopped listing after {MAX_PAGES} pages");
        }
    }
    messages.sort_by_key(|m| m.at);
    Ok(messages)
}

/// Names for every author and mention in `windows`. A failed lookup falls
/// back to the handle the search gave, or the id.
async fn names(client: &ComposioClient, windows: &[Window]) -> Names {
    let mut names = Names::default();
    let mut ids = BTreeSet::new();
    for message in windows.iter().flat_map(|w| &w.messages) {
        names
            .0
            .insert(message.author_id.clone(), message.handle.clone());
        ids.insert(message.author_id.clone());
        ids.extend(mentions(&message.text).map(str::to_owned));
    }
    let lookups = stream::iter(ids)
        .map(|id| async move {
            let name = real_name(client, &id).await;
            (id, name)
        })
        .buffer_unordered(CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (id, name) in lookups {
        match name {
            Ok(Some(name)) => {
                names.0.insert(id, name);
            }
            Ok(None) => {}
            Err(error) => log::debug!("slack: unable to look up a name: {error:#}"),
        }
    }
    names
}

/// The name a Slack user goes by: real name, else display name.
async fn real_name(client: &ComposioClient, id: &str) -> Result<Option<String>> {
    let data = slack(
        client,
        "SLACK_RETRIEVE_DETAILED_USER_INFORMATION",
        json!({"user": id}),
    )
    .await?;
    let user = &data["user"];
    Ok([&user["real_name"], &user["profile"]["display_name"]]
        .into_iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map(str::to_owned))
}

/// Messages by people; bots and integrations have no user.
fn parse_matches(messages: &Value) -> Vec<Message> {
    let Some(matches) = messages["matches"].as_array() else {
        return Vec::new();
    };
    matches
        .iter()
        .filter_map(|m| {
            let author_id = m["user"].as_str()?;
            let channel = &m["channel"];
            let channel_id = channel["id"].as_str()?;
            let ts = m["ts"].as_str()?;
            let at = timestamp(ts)?;
            let direct = channel["is_im"].as_bool().unwrap_or(false)
                || channel["is_mpim"].as_bool().unwrap_or(false);
            let handle = m["username"].as_str().unwrap_or(author_id);
            Some(Message {
                id: format!("{channel_id}:{ts}"),
                channel: channel_id.to_owned(),
                channel_name: text(&channel["name"]),
                direct,
                at,
                handle: handle.to_owned(),
                author_id: author_id.to_owned(),
                text: text(&m["text"]),
            })
        })
        .collect()
}

/// Whether `message` concerns the user `me`.
fn is_anchor(message: &Message, me: &str) -> bool {
    message.direct || message.author_id == me || mentions(&message.text).any(|id| id == me)
}

/// The ranges of `messages` around anchors, merged where they touch and
/// split where they run long.
fn spans(messages: &[Message], me: &str) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::new();
    for (index, _) in messages
        .iter()
        .enumerate()
        .filter(|(_, m)| is_anchor(m, me))
    {
        let span =
            index.saturating_sub(CONTEXT_BEFORE)..(index + CONTEXT_AFTER + 1).min(messages.len());
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }
    merged
        .into_iter()
        .flat_map(|span| {
            span.clone()
                .step_by(MAX_WINDOW_MESSAGES)
                .map(move |start| start..(start + MAX_WINDOW_MESSAGES).min(span.end))
        })
        .collect()
}

/// Windows around what concerns `me`, oldest first.
fn windows(messages: impl Iterator<Item = Message>, me: &str) -> Vec<Window> {
    let mut by_channel: BTreeMap<String, Vec<Message>> = BTreeMap::new();
    for message in messages {
        by_channel
            .entry(message.channel.clone())
            .or_default()
            .push(message);
    }
    let mut windows = Vec::new();
    for mut messages in by_channel.into_values() {
        messages.sort_by_key(|m| m.at);
        for span in spans(&messages, me) {
            let messages = messages[span].to_vec();
            windows.push(Window {
                name: messages[0].channel_name.clone(),
                direct: messages[0].direct,
                me: me.to_owned(),
                messages,
            });
        }
    }
    windows.sort_by_key(Window::started);
    windows
}

/// Slack timestamps are Unix seconds with microseconds after the dot.
fn timestamp(ts: &str) -> Option<DateTime<Utc>> {
    let (seconds, micros) = ts.split_once('.').unwrap_or((ts, "0"));
    let micros: u32 = format!("{micros:0<6}").get(..6)?.parse().ok()?;
    DateTime::from_timestamp(seconds.parse().ok()?, micros * 1000)
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(channel: &str, direct: bool, author: &str, n: i64, text: &str) -> Message {
        Message {
            id: format!("{channel}:{n}"),
            channel: channel.to_owned(),
            channel_name: channel.to_lowercase(),
            direct,
            at: DateTime::from_timestamp(1_790_000_000 + n * 60, 0).unwrap(),
            handle: format!("handle-{author}"),
            author_id: author.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn parses_matches_and_drops_bots() {
        let channel = json!({"id": "D1", "name": "U2", "is_im": true, "is_mpim": false});
        let result = json!({"matches": [
            {"channel": channel, "user": "U2", "username": "sam", "ts": "1790000000.000100", "text": "hi"},
            {"channel": channel, "user": null, "username": "alerts", "ts": "1790000001.000000", "text": "up"},
        ]});
        let messages = parse_matches(&result);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].direct);
        assert_eq!(messages[0].id, "D1:1790000000.000100");
        assert_eq!(messages[0].handle, "sam");
    }

    #[test]
    fn windows_surround_what_concerns_the_user() {
        // A busy channel: the user writes at 10 and is mentioned at 30.
        let mut messages: Vec<Message> = (0..40)
            .map(|n| message("C1", false, "U2", n, "chatter"))
            .collect();
        messages[10].author_id = "ME".to_owned();
        messages[30].text = "<@ME|me> send the numbers tomorrow".to_owned();
        // A channel the user only reads.
        messages.extend((0..5).map(|n| message("C2", false, "U3", n, "elsewhere")));
        // A direct message is always read.
        messages.push(message("D1", true, "U4", 50, "lunch?"));

        let windows = windows(messages.into_iter(), "ME");
        let spans: Vec<(&str, i64, i64)> = windows
            .iter()
            .map(|w| {
                let n = |m: &Message| (m.at.timestamp() - 1_790_000_000) / 60;
                (
                    w.messages[0].channel.as_str(),
                    n(&w.messages[0]),
                    n(w.messages.last().unwrap()),
                )
            })
            .collect();
        assert_eq!(spans, [("C1", 7, 12), ("C1", 27, 32), ("D1", 50, 50)]);
    }

    #[test]
    fn long_runs_split_into_windows() {
        let messages: Vec<Message> = (0..60)
            .map(|n| message("D1", true, "U2", n, "hi"))
            .collect();
        let sizes: Vec<usize> = windows(messages.into_iter(), "ME")
            .iter()
            .map(|w| w.messages.len())
            .collect();
        assert_eq!(sizes, [25, 25, 10]);
    }

    #[test]
    fn renders_names_and_marks_the_user() {
        let mut names = Names::default();
        names.0.insert("ME".to_owned(), "Dana Lee".to_owned());
        names.0.insert("U2".to_owned(), "Sam Roe".to_owned());
        let window = Window {
            name: "general".to_owned(),
            direct: false,
            me: "ME".to_owned(),
            messages: vec![
                message(
                    "C1",
                    false,
                    "U2",
                    0,
                    "<@ME> can you send it by Friday? cc <@U9>",
                ),
                message("C1", false, "ME", 1, "sure"),
            ],
        };
        let rendered = window.render(&names, 1000);
        assert!(rendered.starts_with("Slack channel #general\n"));
        assert!(rendered.contains("Sam Roe: @Dana Lee can you send it by Friday? cc @U9"));
        assert!(rendered.contains("Dana Lee (the user): sure"));
    }

    #[test]
    fn reads_slack_timestamps_and_mentions() {
        let at = timestamp("1790000000.123456").unwrap();
        assert_eq!(at.timestamp(), 1_790_000_000);
        assert_eq!(at.timestamp_subsec_micros(), 123_456);
        assert_eq!(timestamp("1790000000").unwrap().timestamp(), 1_790_000_000);
        assert!(timestamp("soon").is_none());
        let ids: Vec<&str> = mentions("hi <@U1> and <@U2|sam>, <@broken").collect();
        assert_eq!(ids, ["U1", "U2"]);
    }
}
