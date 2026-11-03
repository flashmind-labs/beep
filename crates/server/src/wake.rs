//! Wakes the agent on an event in a new chat; without `notify_user` the chat is deleted silently.

use crate::{
    app::{App, Draws, Message, UserNotice},
    protocol::{DeviceKind, Run},
    store::{CreateRun, Location, RunStatus, SessionRef},
    user::User,
};
use anyhow::Result;
use chrono::{Offset, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use std::{
    fmt::{self, Write},
    sync::Arc,
};
use uuid::Uuid;

/// Something that happened, as the agent is told about it.
pub struct Event {
    /// A few words shown in place of the message and as the kept chat's title.
    pub label: String,
    /// What happened, for the model.
    pub description: String,
    /// The event's details, given to the model as JSON; `Null` for none.
    pub data: Value,
}

impl Event {
    /// The message the run starts on.
    fn message(&self) -> String {
        let mut message = self.description.trim().to_owned();
        if !self.data.is_null() {
            let _ = write!(message, "\n\nDetails: {}", self.data);
        }
        message
    }
}

/// How far the phone must move for a new place to count as an arrival.
const ARRIVAL_KM: f64 = 30.0;

/// The arrival in a new city or country between two fixes, if any.
pub fn arrival(before: &Location, now: &Location) -> Option<Event> {
    let (from, to) = (Place::parse(before)?, Place::parse(now)?);
    let km = distance_km(before, now);
    if from == to || km < ARRIVAL_KM {
        return None;
    }
    let new_country = from.country != to.country;
    let mut description =
        format!("The user's phone is now in {to}. Its last report before this was from {from}.");
    if let Some(shift) = clock_shift(before, now) {
        let _ = write!(description, " {shift}");
    }
    Some(Event {
        label: format!("Arrived in {}", to.city),
        description,
        data: json!({
            "place": now.place,
            "previous_place": before.place,
            "previous_report": before.updated_at.to_rfc3339(),
            "distance_km": km.round(),
            "new_country": new_country,
            "timezone": now.timezone,
            "previous_timezone": before.timezone,
        }),
    })
}

/// How far local time moved between two fixes, if it did.
fn clock_shift(before: &Location, now: &Location) -> Option<String> {
    let offset = |location: &Location| {
        let zone: Tz = location.timezone.parse().ok()?;
        Some(
            Utc::now()
                .with_timezone(&zone)
                .offset()
                .fix()
                .local_minus_utc(),
        )
    };
    let minutes = (offset(now)? - offset(before)?) / 60;
    if minutes == 0 {
        return None;
    }
    let (hours, rest) = (minutes.abs() / 60, minutes.abs() % 60);
    let amount = match (hours, rest) {
        (0, m) => format!("{m} minutes"),
        (1, 0) => "1 hour".to_owned(),
        (h, 0) => format!("{h} hours"),
        (h, m) => format!("{h}:{m:02} hours"),
    };
    let side = if minutes > 0 { "ahead of" } else { "behind" };
    Some(format!("Local time is {amount} {side} its last report."))
}

/// A reverse-geocoded place's city and country.
#[derive(Debug, PartialEq)]
struct Place<'a> {
    city: &'a str,
    country: &'a str,
}

impl<'a> Place<'a> {
    /// Reads the first and last parts of "City, Region, Country" (or just "Singapore").
    fn parse(location: &'a Location) -> Option<Self> {
        let mut parts = location
            .place
            .as_deref()?
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty());
        let city = parts.next()?;
        let country = parts.next_back().unwrap_or(city);
        Some(Self { city, country })
    }
}

impl fmt::Display for Place<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.city == self.country {
            f.write_str(self.city)
        } else {
            write!(f, "{}, {}", self.city, self.country)
        }
    }
}

/// Great-circle distance between two fixes.
fn distance_km(a: &Location, b: &Location) -> f64 {
    const EARTH_KM: f64 = 6371.0;
    let (lat1, lat2) = (a.latitude.to_radians(), b.latitude.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.longitude - a.longitude).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    // Rounding can put `h` a hair over 1, where asin is NaN.
    2.0 * EARTH_KM * h.sqrt().min(1.0).asin()
}

/// Starts the context line that marks a woken turn.
const WOKEN: &str = "Woken by an event";

/// How the agent is told to judge a woken turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The phone arrived somewhere new (see [`arrival`]).
    Place,
    /// Things happened in the user's connected accounts (see `crate::events`).
    Accounts,
    /// Account events the agent held earlier, handed back together.
    Digest,
    /// Event triage found events that need the user now.
    Triaged,
}

impl Rule {
    /// Whether the turn starts on outside text, so it only reads: no acting app tool and no machine.
    pub(crate) fn untrusted(self) -> bool {
        match self {
            Rule::Place => false,
            Rule::Accounts | Rule::Digest | Rule::Triaged => true,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Rule::Place => PLACE_RULE,
            Rule::Accounts => ACCOUNTS_RULE,
            Rule::Digest => DIGEST_RULE,
            Rule::Triaged => TRIAGED_RULE,
        }
    }
}

/// The label and rule of a woken turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Woken {
    pub label: String,
    pub rule: Rule,
}

/// How the model handles a turn woken by the phone's place.
const PLACE_RULE: &str = "The message is an event that just happened, not something the \
user said, and nobody is watching this chat. Look up what you need to judge it, such as \
memories, the calendar or connected apps, and search memories for what the event could \
change (plans, routines, people), not only for its place or name. Notify only when the event meets something you \
know and you can tell the user something useful now that they would not think of \
themselves: on arriving where a trip was planned, the hotel or the next commitment there; \
on a new time zone, what it shifts in a routine, such as when to take a daily medication; \
someone they know who lives there. Stay silent when coming back home, when you know \
nothing that connects to the event, or when all you could say is the event itself, such as \
where they are or a welcome. To notify, call notify_user, then write the message to the \
user in one or two short, specific sentences of plain text, without Markdown, using only \
facts from the context; it is sent as the notification. Otherwise do not call notify_user \
and end with one line on why; the user never sees it.";

/// How the model handles a turn woken by account events.
const ACCOUNTS_RULE: &str = "The message lists things that just happened in the user's \
connected accounts, not something the user said, and nobody is watching this chat. Decide \
what deserves their attention. Read what you need with the app tools and memories, but do \
not send, change or delete anything in this turn: the user decides that after reading your \
message. Notify now when something needs them soon or they would clearly want to know: a \
review request, a mention, a failed build of their work, mail from a real person that asks \
something of them, a meeting moved, cancelled or about to start. Then call notify_user and \
write one to three short sentences of plain text, without Markdown: what happened and what \
you can do about it, offered as a question, such as \"Ana opened a PR on org/repo. Want me \
to review it?\" or \"3 new emails: a lease renewal from Ana and two newsletters. Archive \
the newsletters and draft a reply to Ana?\". Group related events in one message. Events \
worth knowing but not now: call hold_events with their ids and they come back in a later \
digest. Anything else is dropped: do nothing with it. Without notify_user the user never \
sees this chat, so end with one line on what you decided.";

/// How the model handles held account events handed back together.
const DIGEST_RULE: &str = "The message lists account events you held earlier as worth \
knowing but not urgent, not something the user said, and nobody is watching this chat. \
Read what you need with the app tools, but do not send, change or delete anything in this \
turn. If any still matter, call notify_user and write a short digest in plain text, without \
Markdown: the ones that matter, most important first, one line each, ending with what you \
can do about them as a question. If none matter any more, do not call notify_user and end \
with one line on why; the user never sees it.";

/// How the model handles what the event triage agent passed on.
const TRIAGED_RULE: &str = "The message is a note from your event triage agent, which \
read new events from the user's connected accounts and judged this worth telling them now. \
It is not something the user said, and nobody is watching this chat. Check what you need \
with the app tools and memories, using the ids listed, but do not send, change or delete \
anything in this turn: the user decides that after reading your message. If it still \
deserves their attention, call notify_user and write one to three short sentences of plain \
text, without Markdown: what happened and what you can do about it, offered as a question. \
Mention the earlier, less urgent items only when they fit in a short line. If it does not \
deserve their attention after all, do not call notify_user and end with one line on why; \
the user never sees it.";

/// The context lines for a woken turn, read back by [`woken_label`].
pub(crate) fn woken_context(woken: &Woken) -> String {
    let label = woken.label.replace('\n', " ");
    format!("{WOKEN}: {}\n{}", label.trim(), woken.rule.text())
}

/// The event's label in a [`woken_context`], else `None`.
pub fn woken_label(context: &str) -> Option<&str> {
    context
        .lines()
        .find_map(|line| line.strip_prefix(WOKEN)?.strip_prefix(": "))
}

/// Wakes the agent on `event` in a new chat; `None` when no run was created.
pub async fn wake(app: &Arc<App>, user: &Arc<User>, event: Event) -> Result<Option<Run>> {
    wake_with(app, user, event, Rule::Place).await
}

/// Wakes the agent on `event` under `rule`, as [`wake`] does.
pub async fn wake_with(
    app: &Arc<App>,
    user: &Arc<User>,
    event: Event,
    rule: Rule,
) -> Result<Option<Run>> {
    let message = Message {
        session_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        message: event.message(),
        cron_id: None,
        scheduled: None,
        always_sent: false,
        woken: Some(Woken {
            label: event.label,
            rule,
        }),
        images: Vec::new(),
        files: Vec::new(),
        answer: None,
        model: None,
        thinking: None,
        origin: DeviceKind::Phone {},
        platform: None,
        draws: Draws::default(),
        finished_research: false,
        voice: None,
        spoken: false,
    };
    match app.submit(user, message).await? {
        CreateRun::Created(run) => {
            log::info!("wake: run started; run_id={} user={user}", run.id);
            Ok(Some(run))
        }
        _ => Ok(None),
    }
}

/// How a woken run ended.
pub(crate) struct Finished<'a> {
    pub label: &'a str,
    pub status: RunStatus,
    /// Whether the model called `notify_user`.
    pub notified: bool,
    pub reply: &'a str,
}

/// Sends a woken run's reply and keeps its chat if asked, else deletes it silently.
pub(crate) async fn finish(app: &App, user: &User, run: &Run, finished: Finished<'_>) {
    let Ok(chat) = Uuid::parse_str(&run.session_id) else {
        return;
    };
    match finished.status {
        // The approval notice asks the user; the chat holds the question.
        RunStatus::Waiting => {}
        RunStatus::Completed if finished.notified => {
            let session = SessionRef {
                user: user.id().to_owned(),
                id: chat,
            };
            match app
                .store
                .set_generated_title(&session, finished.label)
                .await
            {
                // Nobody listening just means no phone is online.
                Ok(_) => drop(app.notifications.send(UserNotice::changed(user.id(), ""))),
                Err(error) => {
                    log::warn!("wake: unable to title the chat: {error}; run_id={}", run.id)
                }
            }
            app.notify_run_complete(user, run, finished.status, finished.reply)
                .await;
        }
        status => {
            log::info!(
                "wake: nothing sent, chat deleted; run_id={} status={}",
                run.id,
                status.as_str()
            );
            if let Err(error) = user.delete_session(chat).await {
                log::warn!(
                    "wake: unable to delete the chat: {error}; run_id={}",
                    run.id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::f64::consts::PI;

    #[test]
    fn woken_context_reads_back_its_label() {
        for rule in [Rule::Place, Rule::Accounts, Rule::Digest, Rule::Triaged] {
            let woken = Woken {
                label: "Arrived in\nSingapore".to_owned(),
                rule,
            };
            assert_eq!(
                woken_label(&woken_context(&woken)),
                Some("Arrived in Singapore")
            );
        }
        assert_eq!(woken_label("Time: Monday"), None);
        assert_eq!(woken_label(&crate::cron::scheduled_context("Plants")), None);
    }

    fn at(place: &str, latitude: f64, longitude: f64) -> Location {
        Location {
            latitude,
            longitude,
            accuracy: 20.0,
            timezone: "Europe/Madrid".to_owned(),
            place: Some(place.to_owned()),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn antipodes_are_half_the_earth_apart() {
        let km = distance_km(&at("South", -82.0, 0.0), &at("North", 82.0, -180.0));
        assert!((km - 6371.0 * PI).abs() < 1.0, "{km}");
    }

    #[test]
    fn a_new_city_far_away_is_an_arrival() {
        let barcelona = at("Barcelona, Catalonia, Spain", 41.39, 2.16);
        let girona = at("Girona, Catalonia, Spain", 41.98, 2.82);
        let singapore = at("Singapore", 1.28, 103.85);
        let event = arrival(&barcelona, &girona).expect("a new city");
        assert_eq!(event.label, "Arrived in Girona");
        assert_eq!(event.data["new_country"], false);
        let event = arrival(&girona, &singapore).expect("a new country");
        assert_eq!(event.label, "Arrived in Singapore");
        assert_eq!(event.data["new_country"], true);
        assert!(!event.description.contains("Local time"));
    }

    #[test]
    fn a_new_time_zone_says_how_far_the_clock_moved() {
        let mut lisbon = at("Lisbon, Lisbon, Portugal", 38.72, -9.14);
        lisbon.timezone = "Europe/Lisbon".to_owned();
        let barcelona = at("Barcelona, Catalonia, Spain", 41.39, 2.16);
        let event = arrival(&lisbon, &barcelona).expect("a new country");
        assert!(
            event
                .description
                .ends_with("Local time is 1 hour ahead of its last report."),
            "{}",
            event.description
        );
    }

    #[test]
    fn nearby_or_unknown_places_are_not_arrivals() {
        let barcelona = at("Barcelona, Catalonia, Spain", 41.39, 2.16);
        let gracia = at("Barcelona, Catalonia, Spain", 41.40, 2.15);
        let hospitalet = at("L'Hospitalet de Llobregat, Catalonia, Spain", 41.36, 2.10);
        let mut unknown = at("", 40.42, -3.70);
        unknown.place = None;
        assert!(arrival(&barcelona, &gracia).is_none());
        assert!(arrival(&barcelona, &hospitalet).is_none());
        assert!(arrival(&barcelona, &unknown).is_none());
    }

    #[test]
    fn the_message_carries_the_details() {
        let event = |data| Event {
            label: "Arrived".to_owned(),
            description: "User is now in Singapore.".to_owned(),
            data,
        };
        assert_eq!(event(Value::Null).message(), "User is now in Singapore.");
        assert_eq!(
            event(json!({"city": "Singapore"})).message(),
            "User is now in Singapore.\n\nDetails: {\"city\":\"Singapore\"}"
        );
    }
}
