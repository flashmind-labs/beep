//! Public transport and walking routes from Transitous, a free router over
//! open timetable data (https://transitous.org). The legs come back as
//! short text, so the agent answers about lines and stations from real data.
//! The same routes are saved as a card under the call's id, with each leg's
//! shape, for the apps to draw; the model never sees the coordinates.

use crate::{
    protocol::{RouteCard, RouteLeg, RouteOption, RoutePoint},
    user::User,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter, Write},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use uuid::Uuid;

const BASE: &str = "https://api.transitous.org/api";
/// Transitous asks callers to name themselves.
const USER_AGENT: &str = "Beep/0.1 (+https://flashmind.com)";
/// How long an answer is reused for the same question.
const CACHE_FOR: Duration = Duration::from_secs(5 * 60);
/// Requests in flight at once, across every user: it is a shared service.
const IN_FLIGHT: usize = 4;
/// Itineraries shown to the model.
const ROUTES: usize = 3;
/// The most points a leg's shape keeps for the card's sketch.
const SHAPE_POINTS: usize = 48;
/// A wait between legs past which a route is dropped when another has none,
/// like sitting at a station overnight for the first train.
const LONG_WAIT: Duration = Duration::from_secs(60 * 60);

/// The client every run's `get_route` shares, with its cache.
pub struct Transitous {
    client: Client,
    cache: Mutex<HashMap<String, Cached>>,
    slots: Semaphore,
}

struct Cached {
    at: Instant,
    body: Value,
}

impl Transitous {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .timeout(Duration::from_secs(20))
                .build()?,
            cache: Mutex::default(),
            slots: Semaphore::new(IN_FLIGHT),
        })
    }

    /// GETs `path` with `query`, from the cache when asked recently.
    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let url = Url::parse_with_params(&format!("{BASE}{path}"), query)?;
        let key = url.to_string();
        if let Some(body) = self.cached(&key) {
            return Ok(body);
        }
        let _slot = self.slots.acquire().await?;
        let response = self.client.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            bail!("Transitous answered {status}");
        }
        let body: Value = response.json().await?;
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.retain(|_, cached| cached.at.elapsed() < CACHE_FOR);
        cache.insert(
            key,
            Cached {
                at: Instant::now(),
                body: body.clone(),
            },
        );
        Ok(body)
    }

    fn cached(&self, key: &str) -> Option<Value> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .get(key)
            .filter(|cached| cached.at.elapsed() < CACHE_FOR)
            .map(|cached| cached.body.clone())
    }

    /// The best match for `text`, near `near` when given.
    async fn geocode(&self, text: &str, near: Option<Point>) -> Result<Option<Found>> {
        let mut query = vec![("text", text.to_owned())];
        if let Some(near) = near {
            query.push(("place", near.to_string()));
        }
        let body = self.get("/v1/geocode", &query).await?;
        let found: Vec<Found> = serde_json::from_value(body)?;
        Ok(found.into_iter().next())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    latitude: f64,
    longitude: f64,
}

impl Display for Point {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.latitude, self.longitude)
    }
}

#[derive(Deserialize)]
struct Found {
    name: String,
    lat: f64,
    lon: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    #[serde(default)]
    itineraries: Vec<Itinerary>,
    #[serde(default)]
    direct: Vec<Itinerary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Itinerary {
    duration: i64,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    transfers: i64,
    legs: Vec<Leg>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Leg {
    mode: String,
    from: Stop,
    to: Stop,
    duration: i64,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    #[serde(default)]
    real_time: bool,
    distance: Option<f64>,
    headsign: Option<String>,
    agency_name: Option<String>,
    route_short_name: Option<String>,
    route_long_name: Option<String>,
    route_color: Option<String>,
    route_text_color: Option<String>,
    #[serde(default)]
    intermediate_stops: Vec<Value>,
    leg_geometry: Option<Geometry>,
}

/// An encoded polyline, as Google writes them, at `precision` decimals.
#[derive(Deserialize)]
struct Geometry {
    points: String,
    precision: u32,
}

#[derive(Deserialize)]
struct Stop {
    name: String,
    lat: Option<f64>,
    lon: Option<f64>,
    tz: Option<String>,
    track: Option<String>,
}

#[derive(Deserialize)]
struct Args {
    to: Option<String>,
    to_latitude: Option<f64>,
    to_longitude: Option<f64>,
    from: Option<String>,
    from_latitude: Option<f64>,
    from_longitude: Option<f64>,
    time: Option<String>,
    #[serde(default)]
    arrive_by: bool,
    #[serde(default)]
    walk_only: bool,
}

pub(crate) struct GetRouteTool {
    pub user: Arc<User>,
    pub transitous: Arc<Transitous>,
    /// The chat the run is in, whose deletion takes the cards with it.
    pub chat: Uuid,
}

/// A plan: the text the model reads and the card the apps draw.
struct Planned {
    text: String,
    card: RouteCard,
}

#[async_trait]
impl Tool for GetRouteTool {
    fn name(&self) -> &str {
        "get_route"
    }
    fn description(&self) -> &str {
        "Plan a public transport or walking route: lines, operators, headsigns, stations, times, transfers and walks. Use it before naming a line, station or connection. Starts from the user's last location unless a start is given. Pass coordinates when you have them; a place name is matched by a geocoder that can pick the wrong place, so check the matched name in the result. Timetable data, usually without live delays. It does not know station exits; search the web for those."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {"type": "string", "description": "Destination name with the city, like 'Central Station, Amsterdam', when you have no coordinates."},
                "to_latitude": {"type": "number"},
                "to_longitude": {"type": "number"},
                "from": {"type": "string", "description": "Start name with the city, only when not starting from the user's location."},
                "from_latitude": {"type": "number"},
                "from_longitude": {"type": "number"},
                "time": {"type": "string", "description": "When to leave (or arrive with arrive_by), ISO 8601 local time at the start, like 2026-10-12T18:30:00. Defaults to now."},
                "arrive_by": {"type": "boolean", "description": "time is when to arrive."},
                "walk_only": {"type": "boolean", "description": "Walk the whole way."}
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(error) => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("Invalid arguments: {error}"),
                ));
            }
        };
        Ok(match self.route(args).await {
            Ok(Planned { text, card }) => {
                // Without the card the answer still stands; the app shows none.
                if let Err(error) = self
                    .user
                    .save_route_card(self.chat, ctx.tool_call_id, &card)
                    .await
                {
                    log::warn!("unable to save route card: {error:#}");
                }
                ToolResult::success(ctx.tool_call_id, text)
            }
            Err(error) => ToolResult::failure(ctx.tool_call_id, format!("{error:#}")),
        })
    }
}

impl GetRouteTool {
    async fn route(&self, args: Args) -> Result<Planned> {
        let location = self.user.get_location().await?;
        let here = location.as_ref().map(|location| Point {
            latitude: location.latitude,
            longitude: location.longitude,
        });
        let mut out = String::new();
        let from = match (args.from_latitude, args.from_longitude, &args.from) {
            (Some(latitude), Some(longitude), _) => Point {
                latitude,
                longitude,
            },
            (_, _, Some(name)) => self.find(name, here, "Start", &mut out).await?,
            _ => here.context(
                "No start: the user's location is unknown. Ask where they are starting from.",
            )?,
        };
        let to = match (args.to_latitude, args.to_longitude, &args.to) {
            (Some(latitude), Some(longitude), _) => Point {
                latitude,
                longitude,
            },
            (_, _, Some(name)) => self.find(name, Some(from), "Destination", &mut out).await?,
            _ => bail!("Give a destination: to, or to_latitude and to_longitude."),
        };
        let tz: Option<Tz> = location.and_then(|location| location.timezone.parse().ok());
        let mut query = vec![
            ("fromPlace", from.to_string()),
            ("toPlace", to.to_string()),
            ("arriveBy", args.arrive_by.to_string()),
        ];
        if let Some(time) = &args.time {
            query.push(("time", utc(time, tz)?));
        }
        let body = self.transitous.get("/v5/plan", &query).await?;
        let plan: Plan =
            serde_json::from_value(body).context("unexpected answer from Transitous")?;
        let routes: Vec<&Itinerary> = if args.walk_only || plan.itineraries.is_empty() {
            plan.direct.iter().take(1).collect()
        } else {
            useful(&plan.itineraries).into_iter().take(ROUTES).collect()
        };
        if routes.is_empty() {
            bail!(
                "No route found. The area may lack open timetable data; say so and fall back to a web search."
            );
        }
        for (i, route) in routes.iter().enumerate() {
            describe(&mut out, i + 1, route);
        }
        Ok(Planned {
            text: out,
            card: RouteCard {
                routes: routes.into_iter().map(option).collect(),
            },
        })
    }

    /// Geocodes `name` near `near`, noting the match in `out`.
    async fn find(
        &self,
        name: &str,
        near: Option<Point>,
        label: &str,
        out: &mut String,
    ) -> Result<Point> {
        let found = self
            .transitous
            .geocode(name, near)
            .await?
            .with_context(|| format!("{label} {name:?} not found; give coordinates."))?;
        let _ = writeln!(
            out,
            "{label} matched: {} ({:.5}, {:.5})",
            found.name, found.lat, found.lon
        );
        Ok(Point {
            latitude: found.lat,
            longitude: found.lon,
        })
    }
}

/// `time` in UTC for Transitous: as given when it has an offset, else as a
/// local time in `tz`.
fn utc(time: &str, tz: Option<Tz>) -> Result<String> {
    let at = match DateTime::parse_from_rfc3339(time) {
        Ok(at) => at.with_timezone(&Utc),
        Err(_) => {
            let naive = NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M:%S")
                .or_else(|_| NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M"))
                .with_context(|| format!("time {time:?} is not ISO 8601"))?;
            match tz {
                Some(tz) => tz
                    .from_local_datetime(&naive)
                    .earliest()
                    .context("that local time does not exist")?
                    .with_timezone(&Utc),
                None => Utc.from_utc_datetime(&naive),
            }
        }
    };
    Ok(at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// `at` as a local clock time at the stop.
fn clock(at: DateTime<Utc>, stop: &Stop) -> String {
    match stop.tz.as_deref().and_then(|tz| tz.parse::<Tz>().ok()) {
        Some(tz) => at.with_timezone(&tz).format("%H:%M").to_string(),
        None => format!("{} UTC", at.format("%H:%M")),
    }
}

fn describe(out: &mut String, n: usize, route: &Itinerary) {
    let Some(first) = route.legs.first() else {
        return;
    };
    let last = route.legs.last().unwrap_or(first);
    let live = route.legs.iter().any(|leg| leg.real_time);
    let _ = writeln!(
        out,
        "\nRoute {n}: {} to {}, {} min, {} transfer(s){}",
        clock(route.start_time, &first.from),
        clock(route.end_time, &last.to),
        route.duration / 60,
        route.transfers,
        if live { "" } else { ", timetable times" }
    );
    for leg in &route.legs {
        let from = place(&leg.from.name, "start");
        let to = place(&leg.to.name, "destination");
        if leg.mode == "WALK" {
            let metres = leg
                .distance
                .map(|d| format!(", {d:.0} m"))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "- Walk {} min{metres}, {from} to {to}",
                minutes(leg.duration)
            );
            continue;
        }
        let mut line = leg.mode.to_lowercase().replace('_', " ");
        if let Some(agency) = &leg.agency_name {
            let _ = write!(line, ", {agency}");
        }
        if let Some(name) = route_name(leg) {
            let _ = write!(line, " {name}");
        }
        if let Some(color) = leg.route_color.as_deref().filter(|c| !c.is_empty()) {
            let _ = write!(line, " (line colour #{color})");
        }
        if let Some(headsign) = leg.headsign.as_deref().filter(|h| !h.is_empty()) {
            let _ = write!(line, " toward {headsign}");
        }
        let track = |stop: &Stop| {
            stop.track
                .as_deref()
                .filter(|t| !t.is_empty())
                .map(|t| format!(" platform {t}"))
                .unwrap_or_default()
        };
        let _ = writeln!(
            out,
            "- {line}: {} from {from}{} to {} at {to}{}, {} stop(s)",
            clock(leg.start_time, &leg.from),
            track(&leg.from),
            clock(leg.end_time, &leg.to),
            track(&leg.to),
            leg.intermediate_stops.len() + 1
        );
    }
}

/// The routes worth showing: none that another leaves no earlier for and
/// arrives no later by, and none with a long wait while a route without
/// one exists.
fn useful(routes: &[Itinerary]) -> Vec<&Itinerary> {
    let beaten = |a: &Itinerary| {
        routes.iter().any(|b| {
            b.start_time >= a.start_time
                && b.end_time <= a.end_time
                && (b.start_time > a.start_time || b.end_time < a.end_time)
        })
    };
    let kept: Vec<&Itinerary> = routes.iter().filter(|a| !beaten(a)).collect();
    let short = |route: &&Itinerary| longest_wait(route) <= LONG_WAIT;
    if kept.iter().any(short) {
        kept.into_iter().filter(short).collect()
    } else {
        kept
    }
}

/// The longest gap between one leg's end and the next one's start.
fn longest_wait(route: &Itinerary) -> Duration {
    route
        .legs
        .windows(2)
        .filter_map(|pair| (pair[1].start_time - pair[0].end_time).to_std().ok())
        .max()
        .unwrap_or_default()
}

/// `route` as the card shows it.
fn option(route: &Itinerary) -> RouteOption {
    let first = route.legs.first();
    let last = route.legs.last();
    RouteOption {
        departs: first.map_or_else(String::new, |leg| clock(route.start_time, &leg.from)),
        arrives: last.map_or_else(String::new, |leg| clock(route.end_time, &leg.to)),
        minutes: count(route.duration / 60),
        transfers: count(route.transfers),
        live: route.legs.iter().any(|leg| leg.real_time),
        legs: route.legs.iter().map(card_leg).collect(),
    }
}

fn card_leg(leg: &Leg) -> RouteLeg {
    let walk = leg.mode == "WALK";
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let named = |value: &Option<String>| {
        value
            .as_deref()
            .filter(|name| readable(name))
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let color = |value: &Option<String>| {
        value
            .as_deref()
            .filter(|c| c.len() == 6 && c.chars().all(|c| c.is_ascii_hexdigit()))
            .map(|c| format!("#{c}"))
            .unwrap_or_default()
    };
    let line = named(&leg.route_short_name);
    let mut name = named(&leg.route_long_name);
    // The apps fall back to the mode as it is, like "regional_rail".
    if !walk && line.is_empty() && name.is_empty() {
        name = mode_name(&leg.mode);
    }
    RouteLeg {
        mode: leg.mode.to_lowercase(),
        line,
        name,
        color: color(&leg.route_color),
        text_color: color(&leg.route_text_color),
        headsign: text(&leg.headsign),
        from: place(&leg.from.name, "").to_owned(),
        to: place(&leg.to.name, "").to_owned(),
        from_track: text(&leg.from.track),
        to_track: text(&leg.to.track),
        departs: clock(leg.start_time, &leg.from),
        arrives: clock(leg.end_time, &leg.to),
        minutes: count(minutes(leg.duration)),
        stops: if walk {
            0
        } else {
            count(leg.intermediate_stops.len() as i64 + 1)
        },
        meters: leg
            .distance
            .map_or(0, |d| d.round().clamp(0.0, u32::MAX as f64) as u32),
        points: shape(leg),
    }
}

/// A mode as a rider reads it: "REGIONAL_RAIL" becomes "Regional rail".
fn mode_name(mode: &str) -> String {
    let words = mode.to_lowercase().replace('_', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn count(n: i64) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(u32::MAX)
}

/// The leg's shape for the sketch: its polyline, simplified, or else a
/// straight line between its stops.
fn shape(leg: &Leg) -> Vec<RoutePoint> {
    let line = leg
        .leg_geometry
        .as_ref()
        .and_then(|geometry| decode(&geometry.points, geometry.precision))
        .filter(|points| points.len() >= 2);
    match line {
        Some(points) => simplify(&points, SHAPE_POINTS),
        None => [&leg.from, &leg.to]
            .into_iter()
            .filter_map(|stop| {
                Some(RoutePoint {
                    latitude: stop.lat?,
                    longitude: stop.lon?,
                })
            })
            .collect(),
    }
}

/// Decodes a Google encoded polyline written at `precision` decimals.
fn decode(encoded: &str, precision: u32) -> Option<Vec<RoutePoint>> {
    let scale = 10f64.powi(i32::try_from(precision).ok()?);
    let mut bytes = encoded.bytes();
    let mut next = || -> Option<Option<i64>> {
        let mut result = 0i64;
        let mut shift = 0;
        loop {
            let Some(byte) = bytes.next() else {
                // The end of the string, which is fine only between numbers.
                return if shift == 0 { Some(None) } else { None };
            };
            let chunk = i64::from(byte.checked_sub(63)?);
            result |= (chunk & 0x1f) << shift;
            shift += 5;
            if shift > 60 {
                return None;
            }
            if chunk < 0x20 {
                break;
            }
        }
        Some(Some(if result & 1 == 1 {
            !(result >> 1)
        } else {
            result >> 1
        }))
    };
    let (mut latitude, mut longitude) = (0i64, 0i64);
    let mut points = Vec::new();
    while let Some(dlat) = next()? {
        latitude += dlat;
        longitude += next()??;
        points.push(RoutePoint {
            latitude: latitude as f64 / scale,
            longitude: longitude as f64 / scale,
        });
    }
    Some(points)
}

/// At most `most` of `points`, keeping the corners: Douglas-Peucker, with a
/// tolerance raised until few enough remain.
fn simplify(points: &[RoutePoint], most: usize) -> Vec<RoutePoint> {
    if points.len() <= most {
        return points.to_vec();
    }
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for p in points {
        lo = lo.min(p.latitude).min(p.longitude);
        hi = hi.max(p.latitude).max(p.longitude);
    }
    let mut tolerance = (hi - lo).max(1e-6) / 400.0;
    loop {
        let mut keep = vec![false; points.len()];
        keep[0] = true;
        keep[points.len() - 1] = true;
        mark(points, 0, points.len() - 1, tolerance, &mut keep);
        let kept: Vec<RoutePoint> = points
            .iter()
            .zip(&keep)
            .filter(|(_, keep)| **keep)
            .map(|(point, _)| *point)
            .collect();
        if kept.len() <= most {
            return kept;
        }
        tolerance *= 2.0;
    }
}

/// Marks the point between `start` and `end` farthest from their line, and
/// recurses, while it is farther than `tolerance`.
fn mark(points: &[RoutePoint], start: usize, end: usize, tolerance: f64, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let (a, b) = (points[start], points[end]);
    let (dx, dy) = (b.longitude - a.longitude, b.latitude - a.latitude);
    let length = dx.hypot(dy);
    let far = (start + 1..end)
        .map(|i| {
            let p = points[i];
            let distance = if length == 0.0 {
                (p.longitude - a.longitude).hypot(p.latitude - a.latitude)
            } else {
                (dy * (p.longitude - a.longitude) - dx * (p.latitude - a.latitude)).abs() / length
            };
            (i, distance)
        })
        .max_by(|x, y| x.1.total_cmp(&y.1));
    if let Some((i, _)) = far.filter(|(_, distance)| *distance > tolerance) {
        keep[i] = true;
        mark(points, start, i, tolerance, keep);
        mark(points, i, end, tolerance, keep);
    }
}

/// The line's name, unless it is only a feed's internal number.
fn route_name(leg: &Leg) -> Option<String> {
    let short = leg.route_short_name.as_deref().unwrap_or_default();
    let long = leg.route_long_name.as_deref().unwrap_or_default();
    match (readable(short), readable(long)) {
        (true, true) if short != long => Some(format!("{short} ({long})")),
        (true, _) => Some(short.to_owned()),
        (false, true) => Some(long.to_owned()),
        (false, false) => None,
    }
}

/// Whether a line name means something to a rider, rather than being empty
/// or a feed's internal number.
fn readable(name: &str) -> bool {
    !name.is_empty() && !(name.len() > 5 && name.chars().all(|c| c.is_ascii_digit()))
}

fn place<'a>(name: &'a str, fallback: &'a str) -> &'a str {
    match name {
        "START" | "END" => fallback,
        name => name,
    }
}

fn minutes(seconds: i64) -> i64 {
    (seconds + 59) / 60
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_times_read_in_the_users_zone() {
        let tokyo: Tz = "Asia/Tokyo".parse().expect("a known zone");
        assert_eq!(
            utc("2026-10-12T19:00:00", Some(tokyo)).unwrap(),
            "2026-10-12T10:00:00Z"
        );
        assert_eq!(
            utc("2026-10-12T19:00:00+09:00", None).unwrap(),
            "2026-10-12T10:00:00Z"
        );
        assert!(utc("tomorrow", None).is_err());
    }

    #[test]
    fn a_plan_reads_as_legs() {
        let plan: Plan = serde_json::from_value(json!({
            "itineraries": [{
                "duration": 300, "startTime": "2026-10-12T10:00:00Z", "endTime": "2026-10-12T10:05:00Z", "transfers": 0,
                "legs": [
                    {"mode": "WALK", "from": {"name": "START", "tz": "Asia/Tokyo"}, "to": {"name": "Central", "tz": "Asia/Tokyo"},
                     "duration": 60, "startTime": "2026-10-12T10:00:00Z", "endTime": "2026-10-12T10:01:00Z", "distance": 73.0},
                    {"mode": "SUBWAY", "from": {"name": "Central", "tz": "Asia/Tokyo"}, "to": {"name": "Riverside", "tz": "Asia/Tokyo"},
                     "duration": 180, "startTime": "2026-10-12T10:01:00Z", "endTime": "2026-10-12T10:04:00Z",
                     "headsign": "Airport", "agencyName": "City Metro", "routeShortName": "30211664", "routeColor": "ff9501",
                     "intermediateStops": [{}]}
                ]
            }]
        }))
        .unwrap();
        let mut out = String::new();
        describe(&mut out, 1, &plan.itineraries[0]);
        assert!(out.contains("Route 1: 19:00 to 19:05, 5 min, 0 transfer(s), timetable times"));
        assert!(out.contains("- Walk 1 min, 73 m, start to Central"));
        assert!(out.contains(
            "- subway, City Metro (line colour #ff9501) toward Airport: 19:01 from Central to 19:04 at Riverside, 2 stop(s)"
        ));
    }

    #[test]
    fn a_polyline_decodes_at_its_precision() {
        // Google's own example.
        let points = decode("_p~iF~ps|U_ulLnnqC_mqNvxq`@", 5).unwrap();
        let pairs: Vec<(f64, f64)> = points.iter().map(|p| (p.latitude, p.longitude)).collect();
        assert_eq!(pairs, [(38.5, -120.2), (40.7, -120.95), (43.252, -126.453)]);
        // Cut off inside a number.
        assert!(decode("_p~iF~ps|U_ulL", 5).is_none());
        assert_eq!(decode("", 5).unwrap(), []);
    }

    #[test]
    fn a_long_shape_keeps_its_ends_and_corners() {
        let point = |latitude, longitude| RoutePoint {
            latitude,
            longitude,
        };
        // East for 100 points, then north for 100: one corner.
        let mut points: Vec<RoutePoint> =
            (0..100).map(|i| point(0.0, f64::from(i) * 0.001)).collect();
        points.extend((1..=100).map(|i| point(f64::from(i) * 0.001, 0.099)));
        let kept = simplify(&points, 10);
        assert_eq!(
            kept,
            [point(0.0, 0.0), point(0.0, 0.099), point(0.1, 0.099)]
        );
        // Few enough already: unchanged.
        assert_eq!(simplify(&points[..5], 10), &points[..5]);
    }

    #[test]
    fn a_plan_becomes_a_card() {
        let plan: Plan = serde_json::from_value(json!({
            "itineraries": [{
                "duration": 300, "startTime": "2026-10-12T10:00:00Z", "endTime": "2026-10-12T10:05:00Z", "transfers": 0,
                "legs": [
                    {"mode": "WALK", "from": {"name": "START", "lat": 35.0, "lon": 139.0, "tz": "Asia/Tokyo"},
                     "to": {"name": "Central", "lat": 35.001, "lon": 139.001, "tz": "Asia/Tokyo"},
                     "duration": 60, "startTime": "2026-10-12T10:00:00Z", "endTime": "2026-10-12T10:01:00Z", "distance": 73.4},
                    {"mode": "SUBWAY", "from": {"name": "Central", "tz": "Asia/Tokyo", "track": "2"}, "to": {"name": "Riverside", "tz": "Asia/Tokyo"},
                     "duration": 180, "startTime": "2026-10-12T10:01:00Z", "endTime": "2026-10-12T10:04:00Z",
                     "headsign": "Airport", "routeShortName": "30211664", "routeLongName": "Harbour Line",
                     "routeColor": "ff9501", "routeTextColor": "not a colour", "intermediateStops": [{}],
                     "legGeometry": {"points": "_p~iF~ps|U_ulLnnqC_mqNvxq`@", "precision": 5, "length": 3}}
                ]
            }]
        }))
        .unwrap();
        let card = option(&plan.itineraries[0]);
        assert_eq!(
            (card.departs.as_str(), card.arrives.as_str(), card.minutes),
            ("19:00", "19:05", 5)
        );
        let [walk, ride] = &card.legs[..] else {
            panic!("two legs");
        };
        assert_eq!(
            (
                walk.mode.as_str(),
                walk.from.as_str(),
                walk.stops,
                walk.meters
            ),
            ("walk", "", 0, 73)
        );
        // No geometry: a line between the stops.
        assert_eq!(walk.points.len(), 2);
        assert_eq!(ride.line, "", "an internal number is no name");
        assert_eq!(ride.name, "Harbour Line");
        assert_eq!(
            (ride.color.as_str(), ride.text_color.as_str()),
            ("#ff9501", "")
        );
        assert_eq!(
            (ride.from_track.as_str(), ride.departs.as_str(), ride.stops),
            ("2", "19:01", 2)
        );
        assert_eq!(ride.points.len(), 3);
    }

    #[test]
    fn an_overnight_wait_gives_way_to_the_first_train() {
        let leg = |start: &str, end: &str| {
            json!({"mode": "SUBWAY", "from": {"name": "A"}, "to": {"name": "B"}, "duration": 60,
                   "startTime": start, "endTime": end})
        };
        let plan: Plan = serde_json::from_value(json!({
            "itineraries": [
                {"duration": 16320, "startTime": "2026-10-12T15:23:00Z", "endTime": "2026-10-12T19:55:00Z", "transfers": 1,
                 "legs": [leg("2026-10-12T15:23:00Z", "2026-10-12T15:44:00Z"), leg("2026-10-12T19:34:00Z", "2026-10-12T19:55:00Z")]},
                {"duration": 2100, "startTime": "2026-10-12T19:20:00Z", "endTime": "2026-10-12T19:55:00Z", "transfers": 1,
                 "legs": [leg("2026-10-12T19:20:00Z", "2026-10-12T19:30:00Z"), leg("2026-10-12T19:34:00Z", "2026-10-12T19:55:00Z")]},
                {"duration": 2160, "startTime": "2026-10-12T19:30:00Z", "endTime": "2026-10-12T20:06:00Z", "transfers": 1,
                 "legs": [leg("2026-10-12T19:30:00Z", "2026-10-12T19:40:00Z"), leg("2026-10-12T19:45:00Z", "2026-10-12T20:06:00Z")]}
            ]
        }))
        .unwrap();
        let kept: Vec<i64> = useful(&plan.itineraries)
            .iter()
            .map(|r| r.duration)
            .collect();
        assert_eq!(kept, [2100, 2160]);
        assert_eq!(mode_name("REGIONAL_RAIL"), "Regional rail");
    }
}
