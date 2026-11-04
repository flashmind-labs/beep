use super::image;
use super::products::{clip, public_host};
use crate::account::user::User;
use crate::routines::places::Point;
use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{str::FromStr, sync::Arc};
use strum::{EnumString, IntoStaticStr};

const NAME: &str = "show_cards";
const MAX_CARDS: usize = 6;
const MAX_TITLE: usize = 120;
const MAX_SUBTITLE: usize = 80;
const MAX_NOTE: usize = 120;
const MAX_FACTS: usize = 3;
const MAX_FACT: usize = 24;
const MAX_KIND: usize = 20;
/// A location older than this says nothing about how far a place is.
const FRESH_LOCATION_HOURS: i64 = 3;

/// Shows recommendations as cards under the reply; links, images and times are checked here.
pub(crate) struct ShowCardsTool {
    pub user: Arc<User>,
}

#[derive(Debug, Deserialize)]
struct Args {
    cards: Vec<Requested>,
}

/// A card as the model gives it.
#[derive(Debug, Deserialize)]
struct Requested {
    kind: String,
    title: String,
    url: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    facts: Vec<String>,
    #[serde(default)]
    rating: Option<f64>,
    #[serde(default)]
    reviews: Option<u64>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    place: Option<RequestedPlace>,
    #[serde(default)]
    media: Option<RequestedMedia>,
    #[serde(default)]
    event: Option<RequestedEvent>,
}

#[derive(Debug, Deserialize)]
struct RequestedPlace {
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default)]
    hours: Option<String>,
    #[serde(default)]
    open_now: Option<bool>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    booking_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RequestedMedia {
    #[serde(default)]
    year: Option<u16>,
    #[serde(default)]
    creator: Option<String>,
    #[serde(default)]
    watch: Option<String>,
    #[serde(default)]
    watch_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RequestedEvent {
    start: String,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    venue: Option<String>,
    #[serde(default)]
    tickets_url: Option<String>,
}

/// The kinds the apps draw their own part for; others get the base card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum KnownKind {
    Place,
    Media,
    Event,
}

/// A card's kind: lowercase letters and underscores, like `place` or `recipe`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
struct Kind(String);

impl Kind {
    fn parse(kind: &str) -> Option<Self> {
        let kind = kind.trim().to_ascii_lowercase();
        let valid = !kind.is_empty()
            && kind.len() <= MAX_KIND
            && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
        valid.then_some(Self(kind))
    }

    fn known(&self) -> Option<KnownKind> {
        KnownKind::from_str(&self.0).ok()
    }
}

/// A card as the apps draw it.
#[derive(Debug, PartialEq, Serialize)]
struct Card {
    kind: Kind,
    title: String,
    url: String,
    /// The link's domain, for an app that shows where a tap goes.
    site: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    facts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reviews: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    place: Option<PlacePart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    media: Option<MediaPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<EventPart>,
}

#[derive(Debug, Default, PartialEq, Serialize)]
struct PlacePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    longitude: Option<f64>,
    /// How far the user is from it, from a recent location.
    #[serde(skip_serializing_if = "Option::is_none")]
    distance_m: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hours: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_now: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    booking_url: Option<String>,
}

#[derive(Debug, Default, PartialEq, Serialize)]
struct MediaPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    year: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    watch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    watch_url: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
struct EventPart {
    /// RFC 3339 with the event's own UTC offset.
    start: DateTime<FixedOffset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    venue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tickets_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct Shown {
    action: &'static str,
    cards: Vec<Card>,
    /// What was left out and why, for the model.
    dropped: Vec<String>,
}

#[async_trait]
impl Tool for ShowCardsTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Show recommendations to the user as cards under your reply: places to eat, drink or visit, films, shows and books (kind \"media\"), events, or any other kind of thing to pick from. Products go in show_products instead. Use it after searching, with only what a tool or page gave you: the real page URL, the hours and ratings you saw. Leave out a field you do not know. Fill the part that matches the kind: place for a place, media for media, event for an event. Links, images and times are checked, and anything dropped is reported back."
    }
    fn parameters(&self) -> Value {
        let url = |what: &str| json!({ "type": "string", "description": what });
        json!({
            "type": "object",
            "properties": {
                "cards": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_CARDS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "description": "place, media or event; any other single word, like recipe or podcast, gets the plain card" },
                            "title": { "type": "string", "maxLength": MAX_TITLE },
                            "url": url("The thing's own page: the place's website or listing, the film's page, the event page"),
                            "subtitle": { "type": "string", "maxLength": MAX_SUBTITLE, "description": "One short line, like \"Catalan · Plaça de la Llibertat\" or \"2022 · Charlotte Wells\"" },
                            "image": url("https URL of a photo or poster"),
                            "facts": {
                                "type": "array", "maxItems": MAX_FACTS,
                                "items": { "type": "string", "maxLength": MAX_FACT },
                                "description": "Up to three short chips, like \"€€\", \"Jazz\" or \"1h 42m\""
                            },
                            "rating": { "type": "number", "minimum": 0, "maximum": 5 },
                            "reviews": { "type": "integer", "minimum": 0 },
                            "note": { "type": "string", "maxLength": MAX_NOTE, "description": "Why pick this one, in a few words" },
                            "place": {
                                "type": "object",
                                "properties": {
                                    "address": { "type": "string" },
                                    "latitude": { "type": "number", "minimum": -90, "maximum": 90 },
                                    "longitude": { "type": "number", "minimum": -180, "maximum": 180 },
                                    "hours": { "type": "string", "description": "Today's hours as the page gave them, like \"Open until 23:00\"" },
                                    "open_now": { "type": "boolean" },
                                    "phone": { "type": "string", "description": "In international format, like +34 932 18 22 01" },
                                    "booking_url": url("A page to book a table or a visit")
                                },
                                "additionalProperties": false
                            },
                            "media": {
                                "type": "object",
                                "properties": {
                                    "year": { "type": "integer" },
                                    "creator": { "type": "string", "description": "Director, author or host" },
                                    "watch": { "type": "string", "description": "Where to watch, read or listen, like Filmin or \"Rent on Apple TV\"" },
                                    "watch_url": url("Its page on that service")
                                },
                                "additionalProperties": false
                            },
                            "event": {
                                "type": "object",
                                "properties": {
                                    "start": { "type": "string", "description": "RFC 3339 with the event's UTC offset, like 2026-10-18T21:00:00+02:00" },
                                    "end": { "type": "string", "description": "Same format as start" },
                                    "venue": { "type": "string" },
                                    "tickets_url": url("Where to get tickets")
                                },
                                "required": ["start"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["kind", "title", "url"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["cards"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, _args: &Value) -> String {
        "Showing cards…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: Args = match ctx.parse_args(NAME) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        if args.cards.is_empty() || args.cards.len() > MAX_CARDS {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Show between 1 and {MAX_CARDS} cards."),
            ));
        }
        let mut dropped = Vec::new();
        let mut cards = Vec::new();
        for requested in args.cards {
            match card(requested, &mut dropped) {
                Ok(card) => cards.push(card),
                Err(reason) => dropped.push(reason),
            }
        }
        if cards.is_empty() {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("No card could be shown. {}", dropped.join("; ")),
            ));
        }
        let here = match self.user.get_location().await {
            Ok(Some(fix))
                if Utc::now() - fix.updated_at < chrono::Duration::hours(FRESH_LOCATION_HOURS) =>
            {
                Some(Point::from(&fix))
            }
            Ok(_) => None,
            Err(e) => {
                log::warn!("show_cards could not read the location: {e}");
                None
            }
        };
        if let Some(here) = here {
            for place in cards.iter_mut().filter_map(|card| card.place.as_mut()) {
                place.distance_m = distance(here, place);
            }
        }
        let checks = cards.iter().map(|card| async move {
            match card.image.as_deref() {
                Some(url) => image::check(url).await.err(),
                None => None,
            }
        });
        let failures = futures::future::join_all(checks).await;
        for (card, failure) in cards.iter_mut().zip(failures) {
            if let Some(reason) = failure {
                dropped.push(format!("image of {}: {reason}", card.title));
                card.image = None;
            }
        }
        let shown = Shown {
            action: NAME,
            cards,
            dropped,
        };
        Ok(ToolResult::success(
            ctx.tool_call_id,
            serde_json::to_string(&shown)?,
        ))
    }
}

/// The card for `requested`, or why not; bad fields are noted in `dropped`.
fn card(requested: Requested, dropped: &mut Vec<String>) -> Result<Card, String> {
    let title = clip(&requested.title, MAX_TITLE);
    if title.is_empty() {
        return Err("a card without a title".to_owned());
    }
    let kind = Kind::parse(&requested.kind).ok_or_else(|| {
        format!(
            "{title}: kind {:?} is not one lowercase word",
            requested.kind
        )
    })?;
    if kind.0 == "product" {
        return Err(format!("{title}: products go in show_products"));
    }
    let url = requested.url.trim();
    let host = public_host(url).ok_or_else(|| format!("{title}: {url} is not a web page"))?;

    let mut facts: Vec<String> = requested
        .facts
        .iter()
        .map(|f| clip(f, MAX_FACT))
        .filter(|f| !f.is_empty())
        .collect();
    if facts.len() > MAX_FACTS {
        dropped.push(format!("facts of {title} past the first {MAX_FACTS}"));
        facts.truncate(MAX_FACTS);
    }

    let place = requested
        .place
        .filter(|_| fits(&kind, KnownKind::Place, &title, dropped))
        .map(|p| place_part(p, &title, dropped));
    let media = requested
        .media
        .filter(|_| fits(&kind, KnownKind::Media, &title, dropped))
        .map(|m| media_part(m, &title, dropped));
    let event = requested
        .event
        .filter(|_| fits(&kind, KnownKind::Event, &title, dropped))
        .and_then(|e| event_part(e, &title, dropped));

    Ok(Card {
        title: title.clone(),
        url: url.to_owned(),
        site: host.strip_prefix("www.").unwrap_or(&host).to_owned(),
        subtitle: text(requested.subtitle.as_deref(), MAX_SUBTITLE),
        image: requested
            .image
            .map(|i| i.trim().to_owned())
            .filter(|i| !i.is_empty()),
        facts,
        rating: requested.rating.filter(|r| (0.0..=5.0).contains(r)),
        reviews: requested.reviews,
        note: text(requested.note.as_deref(), MAX_NOTE),
        place,
        media,
        event,
        kind,
    })
}

/// Whether a card of `kind` takes `part`; otherwise noted in `dropped`.
fn fits(kind: &Kind, part: KnownKind, title: &str, dropped: &mut Vec<String>) -> bool {
    let fits = kind.known() == Some(part);
    if !fits {
        let name: &'static str = part.into();
        dropped.push(format!(
            "{name} part of {title}: the card's kind is {}",
            kind.0
        ));
    }
    fits
}

fn place_part(p: RequestedPlace, title: &str, dropped: &mut Vec<String>) -> PlacePart {
    let (latitude, longitude) = match (p.latitude, p.longitude) {
        (Some(lat), Some(lon))
            if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) =>
        {
            (Some(lat), Some(lon))
        }
        (None, None) => (None, None),
        _ => {
            dropped.push(format!("coordinates of {title}: need both, in range"));
            (None, None)
        }
    };
    PlacePart {
        address: text(p.address.as_deref(), 120),
        latitude,
        longitude,
        distance_m: None,
        hours: text(p.hours.as_deref(), 40),
        open_now: p.open_now,
        phone: p.phone.as_deref().and_then(|phone| {
            let phone = phone_number(phone);
            if phone.is_none() {
                dropped.push(format!("phone of {title}: not a phone number"));
            }
            phone
        }),
        booking_url: link(p.booking_url, "booking link", title, dropped),
    }
}

fn media_part(m: RequestedMedia, title: &str, dropped: &mut Vec<String>) -> MediaPart {
    MediaPart {
        year: m.year.filter(|y| (1850..=2100).contains(y)),
        creator: text(m.creator.as_deref(), 60),
        watch: text(m.watch.as_deref(), 40),
        watch_url: link(m.watch_url, "watch link", title, dropped),
    }
}

/// The event part, or `None` when its start is not a usable time.
fn event_part(e: RequestedEvent, title: &str, dropped: &mut Vec<String>) -> Option<EventPart> {
    let Ok(start) = DateTime::parse_from_rfc3339(e.start.trim()) else {
        dropped.push(format!(
            "event part of {title}: start {:?} is not RFC 3339 with an offset",
            e.start
        ));
        return None;
    };
    let end = match e
        .end
        .as_deref()
        .map(|end| DateTime::parse_from_rfc3339(end.trim()))
    {
        Some(Ok(end)) if end > start => Some(end),
        Some(_) => {
            dropped.push(format!("end of {title}: not a time after the start"));
            None
        }
        None => None,
    };
    Some(EventPart {
        start,
        end,
        venue: text(e.venue.as_deref(), 80),
        tickets_url: link(e.tickets_url, "tickets link", title, dropped),
    })
}

/// `url` when it is a public web page; otherwise noted in `dropped`.
fn link(url: Option<String>, what: &str, title: &str, dropped: &mut Vec<String>) -> Option<String> {
    let url = url?.trim().to_owned();
    if public_host(&url).is_some() {
        Some(url)
    } else {
        dropped.push(format!("{what} of {title}: {url} is not a web page"));
        None
    }
}

fn text(value: Option<&str>, max: usize) -> Option<String> {
    value.map(|v| clip(v, max)).filter(|v| !v.is_empty())
}

/// A dialable phone number: digits, an optional leading `+`, and separators.
fn phone_number(phone: &str) -> Option<String> {
    let phone = phone.trim();
    let digits = phone.bytes().filter(u8::is_ascii_digit).count();
    let allowed = phone
        .bytes()
        .enumerate()
        .all(|(i, b)| b.is_ascii_digit() || b" -.()".contains(&b) || (b == b'+' && i == 0));
    (allowed && (6..=16).contains(&digits)).then(|| phone.to_owned())
}

/// How far `here` is from `place`, rounded to ten metres.
fn distance(here: Point, place: &PlacePart) -> Option<u32> {
    let there = Point {
        latitude: place.latitude?,
        longitude: place.longitude?,
    };
    let metres = here.distance_m(there);
    (metres < 1_000_000.0).then(|| ((metres / 10.0).round() * 10.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requested(json: Value) -> Requested {
        serde_json::from_value(json).expect("test card is valid")
    }

    fn card_of(json: Value) -> (Card, Vec<String>) {
        let mut dropped = Vec::new();
        let card = card(requested(json), &mut dropped).expect("a valid card");
        (card, dropped)
    }

    #[test]
    fn rejects_bad_kinds_products_and_links() {
        for (kind, url) in [
            ("two words", "https://a.example/"),
            ("", "https://a.example/"),
            ("product", "https://a.example/"),
            ("place", "javascript:alert(1)"),
            ("place", "https://localhost/a"),
        ] {
            let result = card(
                requested(json!({ "kind": kind, "title": "Thing", "url": url })),
                &mut Vec::new(),
            );
            assert!(result.is_err(), "{kind} {url}");
        }
    }

    #[test]
    fn drops_bad_place_details() {
        let (card, dropped) = card_of(json!({
            "kind": "place", "title": "Bar", "url": "https://bar.example/",
            "place": { "latitude": 41.4, "phone": "call us", "booking_url": "file:///etc/passwd" }
        }));
        assert_eq!(card.place.expect("a place part"), PlacePart::default());
        assert_eq!(dropped.len(), 3, "{dropped:?}");
    }
}
