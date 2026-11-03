//! Calendars, read once, five years back and half a year ahead.

use super::{Calendar, Source, Summary, extract, rfc3339, top};
use crate::{
    feeds::mail::Window, memories::clip, starters::Generator, store::ProfileNote, user::User,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as Days, NaiveDate, Utc};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write,
    iter,
};

/// How far the calendar read reaches back and ahead.
const CALENDAR_BACK_DAYS: i64 = 5 * 365;
const CALENDAR_AHEAD_DAYS: i64 = 183;
const CALENDAR_PAGES: usize = 200;
/// How much of the calendar one Fastmail search covers.
const FASTMAIL_SPAN_DAYS: i64 = 90;
/// Days in one Outlook calendar view; Graph refuses views over 1825 days.
const OUTLOOK_SPAN_DAYS: i64 = 365;
const CALENDAR_SYSTEM: &str = "You help a personal assistant app get to know its user. You get events \
from the user's own calendar. A repeating event is listed once with how many times it happened and between \
which days.";

/// One event as the pass reads it.
#[derive(Debug, Clone, PartialEq)]
struct Event {
    title: String,
    day: NaiveDate,
    location: String,
    people: Vec<String>,
}

pub(super) async fn read_calendar(
    generator: Generator<'_>,
    user: &User,
    calendar: Calendar,
    summary: &mut Summary,
) -> Result<()> {
    let now = Utc::now();
    let window = Window {
        after: now - Days::days(CALENDAR_BACK_DAYS),
        before: Some(now + Days::days(CALENDAR_AHEAD_DAYS)),
    };
    let events = match calendar {
        Calendar::Google => google_events(user, window).await?,
        Calendar::Outlook => outlook_events(user, window).await?,
        Calendar::Fastmail => fastmail_events(user, window).await?,
    };
    let key = Source::Calendar(calendar).key();
    summary.items = events.len();
    if events.is_empty() {
        return Ok(());
    }
    let mut notes = vec![calendar_counts(&events)];
    let texts = render_events(events);
    let header = format!(
        "Calendar from {} to {}. Today is {}.",
        window.after.format("%Y-%m-%d"),
        window.before.unwrap_or(now).format("%Y-%m-%d"),
        now.format("%Y-%m-%d")
    );
    notes.extend(extract(generator, CALENDAR_SYSTEM, &header, &texts).await?);
    user.add_profile_notes(key, &notes, None).await?;
    summary.notes = notes.len();
    Ok(())
}

/// Each title once, with how often and when it happened, where and with whom.
fn render_events(events: Vec<Event>) -> Vec<String> {
    let mut by_title: BTreeMap<String, Vec<Event>> = BTreeMap::new();
    for event in events {
        by_title
            .entry(event.title.trim().to_lowercase())
            .or_default()
            .push(event);
    }
    let mut lines: Vec<(NaiveDate, String)> = by_title
        .into_values()
        .map(|mut events| {
            events.sort_by_key(|event| event.day);
            let first = &events[0];
            let last = &events[events.len() - 1];
            let mut line = if events.len() == 1 {
                format!("{} {}", first.day, first.title)
            } else {
                format!(
                    "{} to {} {} ({} times)",
                    first.day,
                    last.day,
                    first.title,
                    events.len()
                )
            };
            let places: HashSet<&str> = events
                .iter()
                .map(|event| event.location.as_str())
                .filter(|place| !place.is_empty())
                .collect();
            if !places.is_empty() {
                let places: Vec<&str> = places.into_iter().take(3).collect();
                let _ = write!(line, "; at {}", places.join(" / "));
            }
            let people: HashSet<&str> = events
                .iter()
                .flat_map(|event| event.people.iter().map(String::as_str))
                .collect();
            if !people.is_empty() {
                let people: Vec<&str> = people.into_iter().take(8).collect();
                let _ = write!(line, "; with {}", people.join(", "));
            }
            (first.day, clip(&line, 600))
        })
        .collect();
    lines.sort();
    lines.into_iter().map(|(_, line)| line).collect()
}

/// The people and places that come up most.
fn calendar_counts(events: &[Event]) -> ProfileNote {
    let mut people: HashMap<String, usize> = HashMap::new();
    let mut places: HashMap<String, usize> = HashMap::new();
    for event in events {
        for person in &event.people {
            *people.entry(person.clone()).or_default() += 1;
        }
        if !event.location.is_empty() {
            *places.entry(event.location.clone()).or_default() += 1;
        }
    }
    let mut text = format!("Calendar: {} events.", events.len());
    if !people.is_empty() {
        let _ = write!(text, " Meets most with: {}.", top(people));
    }
    if !places.is_empty() {
        let _ = write!(text, " Most frequent places: {}.", top(places));
    }
    ProfileNote { date: None, text }
}

async fn google_events(user: &User, window: Window) -> Result<Vec<Event>> {
    let client = user
        .composio_client()
        .context("Composio is not configured")?;
    let mut events = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..CALENDAR_PAGES {
        let mut arguments = json!({
            "calendarId": "primary",
            "timeMin": rfc3339(window.after),
            "timeMax": rfc3339(window.before.unwrap_or_else(Utc::now)),
            "maxResults": 250,
            "singleEvents": true,
        });
        if let Some(token) = &token {
            arguments["pageToken"] = json!(token);
        }
        let response = client
            .execute_tool("GOOGLECALENDAR_EVENTS_LIST", arguments)
            .await?;
        if response.successful == Some(false) {
            bail!("Google Calendar listing failed");
        }
        events.extend(event_list(&response.data).iter().filter_map(parse_event));
        token = find_string(&response.data, "nextPageToken");
        if token.is_none() {
            break;
        }
    }
    Ok(events)
}

async fn outlook_events(user: &User, window: Window) -> Result<Vec<Event>> {
    const TOP: usize = 100;
    let client = user
        .composio_client()
        .context("Composio is not configured")?;
    let mut events = Vec::new();
    for span in spans(window, OUTLOOK_SPAN_DAYS) {
        for page in 0..CALENDAR_PAGES {
            // The calendar view expands repeating events into each time they happen.
            let arguments = json!({
                "start_datetime": rfc3339(span.after),
                "end_datetime": rfc3339(span.before.unwrap_or_else(Utc::now)),
                "top": TOP,
                "skip": page * TOP,
                "orderby": "start/dateTime asc",
            });
            let response = client
                .execute_tool("OUTLOOK_GET_CALENDAR_VIEW", arguments)
                .await?;
            if response.successful == Some(false) {
                bail!(
                    "Outlook calendar listing failed: {}",
                    response.error.as_deref().unwrap_or("no error given")
                );
            }
            let listed = event_list(&response.data);
            events.extend(listed.iter().filter_map(parse_event));
            if listed.len() < TOP {
                break;
            }
        }
    }
    Ok(events)
}

/// Reads Fastmail events a season at a time, since it refuses to expand repeats over long windows.
async fn fastmail_events(user: &User, window: Window) -> Result<Vec<Event>> {
    const TOOL: &str = "search_events";
    // The profile reads the first account, as the mail feed does.
    let server = user.fastmail_server(None).await?;
    let mut schema = user.mcp_tool_schema(&server, TOOL).await;
    if schema.is_none() {
        // Any call connects the server, which lists its tools.
        let _ = user.call_mcp(&server, "list_calendars", json!({})).await;
        schema = user.mcp_tool_schema(&server, TOOL).await;
    }
    let schema = schema.context("Fastmail has no search_events tool")?;
    let arguments = EventArguments::from_schema(&schema)?;
    let mut events = Vec::new();
    for span in spans(window, FASTMAIL_SPAN_DAYS) {
        events.extend(fastmail_span(user, &arguments, span).await?);
    }
    Ok(events)
}

/// `window` cut into back-to-back spans of at most `days`, oldest first.
fn spans(window: Window, days: i64) -> impl Iterator<Item = Window> {
    let end = window.before.unwrap_or_else(Utc::now);
    let mut after = window.after;
    iter::from_fn(move || {
        if after >= end {
            return None;
        }
        let before = (after + Days::days(days)).min(end);
        let span = Window {
            after,
            before: Some(before),
        };
        after = before;
        Some(span)
    })
}

/// Fastmail's events in one span of at most `FASTMAIL_SPAN_DAYS`.
async fn fastmail_span(
    user: &User,
    arguments: &EventArguments,
    window: Window,
) -> Result<Vec<Event>> {
    const TOOL: &str = "search_events";
    const LIMIT: usize = 100;
    let mut events = Vec::new();
    let mut cursor: Option<String> = None;
    let mut offset = 0;
    for _ in 0..CALENDAR_PAGES {
        let call = arguments.build(window, LIMIT, cursor.as_deref(), offset);
        let value = user.call_fastmail(None, TOOL, call).await?;
        let listed = event_list(&value);
        events.extend(listed.iter().filter_map(parse_event));
        offset += listed.len();
        cursor = ["nextCursor", "cursor", "next", "nextPageToken"]
            .iter()
            .find_map(|key| find_string(&value, key));
        let more = match (&arguments.cursor, &arguments.offset) {
            (Some(_), _) => cursor.is_some(),
            (None, Some(_)) => listed.len() >= LIMIT,
            (None, None) => false,
        };
        if !more || listed.is_empty() {
            break;
        }
    }
    Ok(events)
}

/// The argument names a search tool takes for its window, size and paging.
#[derive(Debug, Default, PartialEq)]
struct EventArguments {
    after: Option<Argument>,
    before: Option<Argument>,
    limit: Option<String>,
    cursor: Option<String>,
    offset: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Argument {
    name: String,
    /// Takes `YYYY-MM-DD` rather than a full time.
    date_only: bool,
}

impl EventArguments {
    fn from_schema(schema: &Value) -> Result<Self> {
        let Some(properties) = schema["properties"].as_object() else {
            bail!("search_events has no properties");
        };
        let mut found = EventArguments::default();
        for (name, property) in properties {
            let lower = name.to_lowercase();
            let date_only = property["format"].as_str() == Some("date");
            let argument = || Argument {
                name: name.clone(),
                date_only,
            };
            let has = |words: &[&str]| words.iter().any(|word| lower.contains(word));
            if has(&[
                "limit",
                "maxresults",
                "max_results",
                "count",
                "pagesize",
                "page_size",
            ]) {
                found.limit = Some(name.clone());
            } else if has(&["cursor", "token"]) {
                found.cursor = Some(name.clone());
            } else if has(&["offset", "position", "skip"]) {
                found.offset = Some(name.clone());
            } else if has(&["after", "start", "from", "min", "since"]) {
                found.after.get_or_insert_with(argument);
            } else if has(&["before", "end", "until", "max", "to"]) {
                found.before.get_or_insert_with(argument);
            }
        }
        if found.after.is_none() {
            let names: Vec<&String> = properties.keys().collect();
            bail!("no start argument among {names:?}");
        }
        Ok(found)
    }

    fn build(&self, window: Window, limit: usize, cursor: Option<&str>, offset: usize) -> Value {
        let time = |argument: &Argument, time: DateTime<Utc>| {
            if argument.date_only {
                time.format("%Y-%m-%d").to_string()
            } else {
                rfc3339(time)
            }
        };
        let mut arguments = json!({});
        if let Some(after) = &self.after {
            arguments[&after.name] = json!(time(after, window.after));
        }
        if let (Some(before), Some(end)) = (&self.before, window.before) {
            arguments[&before.name] = json!(time(before, end));
        }
        if let Some(name) = &self.limit {
            arguments[name] = json!(limit);
        }
        if let (Some(name), Some(cursor)) = (&self.cursor, cursor) {
            arguments[name] = json!(cursor);
        }
        if let Some(name) = &self.offset {
            arguments[name] = json!(offset);
        }
        arguments
    }
}

/// The events of a listing, wherever the provider put them.
fn event_list(value: &Value) -> Vec<Value> {
    fn find(value: &Value, depth: usize) -> Option<&Vec<Value>> {
        if let Some(list) = value.as_array() {
            return Some(list);
        }
        let object = value.as_object()?;
        for key in [
            "items",
            "value",
            "events",
            "results",
            "data",
            "response_data",
        ] {
            if let Some(list) = object.get(key).and_then(|value| find_at(value, depth)) {
                return Some(list);
            }
        }
        None
    }
    fn find_at(value: &Value, depth: usize) -> Option<&Vec<Value>> {
        if depth == 0 {
            None
        } else {
            find(value, depth - 1)
        }
    }
    find(value, 4).cloned().unwrap_or_default()
}

/// A string field `key` at most a few levels down.
fn find_string(value: &Value, key: &str) -> Option<String> {
    fn find(value: &Value, key: &str, depth: usize) -> Option<String> {
        let object = value.as_object()?;
        if let Some(found) = object.get(key).and_then(Value::as_str) {
            return Some(found.to_owned()).filter(|found| !found.is_empty());
        }
        if depth == 0 {
            return None;
        }
        object
            .values()
            .filter(|value| value.is_object())
            .find_map(|value| find(value, key, depth - 1))
    }
    find(value, key, 3)
}

/// Reads a Google, Outlook or JMAP-style event.
fn parse_event(value: &Value) -> Option<Event> {
    let cancelled = value["status"].as_str() == Some("cancelled")
        || value["isCancelled"].as_bool() == Some(true);
    if cancelled {
        return None;
    }
    let title = ["summary", "subject", "title"]
        .iter()
        .find_map(|key| value[key].as_str())
        .unwrap_or_default()
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }
    let start = &value["start"];
    let start = start["dateTime"]
        .as_str()
        .or_else(|| start["date"].as_str())
        .or_else(|| start.as_str())
        .or_else(|| value["startTime"].as_str())
        .or_else(|| value["utcStart"].as_str())?;
    let day = NaiveDate::parse_from_str(start.get(..10)?, "%Y-%m-%d").ok()?;
    let location = &value["location"];
    let location = location
        .as_str()
        .or_else(|| location["displayName"].as_str())
        .map(str::to_owned)
        .or_else(|| {
            value["locations"]
                .as_object()?
                .values()
                .find_map(|place| place["name"].as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let people = people(&value["attendees"])
        .into_iter()
        .chain(people(&value["participants"]))
        .collect();
    Some(Event {
        title: clip(&title, 200),
        day,
        location: clip(location.trim(), 200),
        people,
    })
}

/// Names, or addresses when there is no name, of an attendee list or map.
fn people(value: &Value) -> Vec<String> {
    let entries: Vec<&Value> = match value {
        Value::Array(list) => list.iter().collect(),
        Value::Object(map) => map.values().collect(),
        _ => return Vec::new(),
    };
    entries
        .into_iter()
        .filter(|entry| entry["self"].as_bool() != Some(true))
        .filter_map(|entry| {
            let email = &entry["emailAddress"];
            [
                entry["displayName"].as_str(),
                entry["name"].as_str(),
                email["name"].as_str(),
                entry["email"].as_str(),
                email["address"].as_str(),
            ]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|name| !name.is_empty())
            .map(str::to_owned)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_cover_the_window_back_to_back() {
        let after = DateTime::parse_from_rfc3339("2021-10-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let before = after + Days::days(CALENDAR_BACK_DAYS + CALENDAR_AHEAD_DAYS);
        let window = Window {
            after,
            before: Some(before),
        };
        let spans: Vec<_> = spans(window, OUTLOOK_SPAN_DAYS).collect();
        assert_eq!(spans.len(), 6);
        assert_eq!(spans[0].after, after);
        assert_eq!(spans[5].before, Some(before));
        for pair in spans.windows(2) {
            assert_eq!(pair[0].before, Some(pair[1].after));
        }
        assert!(
            spans
                .iter()
                .all(|span| { span.before.unwrap() - span.after <= Days::days(OUTLOOK_SPAN_DAYS) })
        );
    }

    #[test]
    fn reads_google_outlook_and_jmap_events() {
        let google = json!({"summary": "Gym", "start": {"dateTime": "2025-03-04T18:00:00+01:00"},
            "location": "Basic-Fit", "attendees": [{"email": "me@x.com", "self": true}, {"displayName": "Ana"}]});
        let outlook = json!({"subject": "Standup", "start": {"dateTime": "2025-03-05T09:00:00.0000000"},
            "location": {"displayName": "Office"}, "attendees": [{"emailAddress": {"name": "Bo", "address": "bo@x.com"}}]});
        let jmap = json!({"title": "Dinner", "start": "2025-03-06T20:00:00",
            "locations": {"1": {"name": "Casa"}}, "participants": {"p": {"name": "Cy"}}});
        let cancelled =
            json!({"summary": "Gone", "status": "cancelled", "start": {"date": "2025-03-07"}});
        let events: Vec<Event> = [google, outlook, jmap, cancelled]
            .iter()
            .filter_map(parse_event)
            .collect();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].people, ["Ana"]);
        assert_eq!(events[1].location, "Office");
        assert_eq!(events[2].day, NaiveDate::from_ymd_opt(2025, 3, 6).unwrap());
        assert_eq!(events[2].location, "Casa");
    }

    #[test]
    fn repeating_events_are_listed_once() {
        let event = |day: u32| Event {
            title: "Gym".into(),
            day: NaiveDate::from_ymd_opt(2025, 1, day).unwrap(),
            location: "Basic-Fit".into(),
            people: Vec::new(),
        };
        let lines = render_events(vec![event(3), event(10), event(17)]);
        assert_eq!(
            lines,
            ["2025-01-03 to 2025-01-17 Gym (3 times); at Basic-Fit"]
        );
    }

    #[test]
    fn search_arguments_come_from_the_schema() {
        let schema = json!({"properties": {
            "after": {"type": "string", "format": "date"},
            "before": {"type": "string", "format": "date"},
            "limit": {"type": "integer"},
            "position": {"type": "integer"},
        }});
        let arguments = EventArguments::from_schema(&schema).unwrap();
        let day = |d| {
            NaiveDate::from_ymd_opt(2025, 1, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
        };
        let window = Window {
            after: day(1),
            before: Some(day(31)),
        };
        assert_eq!(
            arguments.build(window, 100, None, 200),
            json!({"after": "2025-01-01", "before": "2025-01-31", "limit": 100, "position": 200})
        );
        assert!(EventArguments::from_schema(&json!({"properties": {"query": {}}})).is_err());
    }
}
