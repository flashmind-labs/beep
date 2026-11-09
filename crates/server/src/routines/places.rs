//! Saved places and the routines that start on arriving or leaving, with a wider exit ring against GPS jitter.

use crate::{
    account::user::User,
    app::App,
    protocol::PlaceCrossing,
    routines::cron,
    store::{Crossed, Location, SavedPlace},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::{fmt, str::FromStr, sync::Arc};
use strum::ParseError;
use uuid::Uuid;

/// How near counts as being at a place, in metres, unless the user says.
pub const DEFAULT_RADIUS: u32 = 100;
/// The smallest radius a place can have; GPS is rarely better indoors.
pub const MIN_RADIUS: u32 = 25;
/// The largest radius a place can have.
pub const MAX_RADIUS: u32 = 1000;

/// The least a fix must be past the radius before it counts as leaving.
const EXIT_MARGIN_MIN: f64 = 50.0;
/// A fix less sure than this (or twice the radius) cannot count as arriving.
const ARRIVE_ACCURACY_MIN: f64 = 100.0;

/// Arriving at a place or leaving it.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::AsRefStr,
    strum::Display,
    strum::IntoStaticStr,
    strum::EnumString,
)]
#[strum(serialize_all = "snake_case")]
pub enum PlaceEvent {
    Arrive,
    Leave,
}

impl PlaceEvent {
    /// The event as the routine's message tells it, such as `arrived at Gym`.
    fn past(self, place: &str) -> String {
        match self {
            Self::Arrive => format!("arrived at {place}"),
            Self::Leave => format!("left {place}"),
        }
    }
}

impl From<PlaceCrossing> for PlaceEvent {
    fn from(crossing: PlaceCrossing) -> Self {
        match crossing {
            PlaceCrossing::Arrive {} => Self::Arrive,
            PlaceCrossing::Leave {} => Self::Leave,
        }
    }
}

/// How often a place routine starts, written as the end of its schedule.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::AsRefStr,
    strum::Display,
    strum::IntoStaticStr,
    strum::EnumString,
)]
#[strum(serialize_all = "snake_case")]
pub enum PlaceRepeat {
    /// On every arrival or departure; the schedule has no ending.
    Always,
    /// At most once a day in the user's timezone: `:daily`.
    Daily,
    /// The next time only, then the routine is deleted: `:once`.
    Once,
}

impl PlaceRepeat {
    /// The schedule's ending, or `None` for [`Self::Always`].
    fn suffix(self) -> Option<&'static str> {
        match self {
            Self::Always => None,
            Self::Daily => Some("daily"),
            Self::Once => Some("once"),
        }
    }
}

/// A place schedule: `arrive:<id>` or `leave:<id>`, ending in `:daily` or `:once`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceTrigger {
    pub event: PlaceEvent,
    pub place: Uuid,
    pub repeat: PlaceRepeat,
}

impl FromStr for PlaceTrigger {
    type Err = ParseError;

    fn from_str(schedule: &str) -> Result<Self, Self::Err> {
        let invalid = ParseError::VariantNotFound;
        let mut parts = schedule.trim().split(':');
        let event = parts.next().ok_or(invalid)?.parse()?;
        let place = parts.next().ok_or(invalid)?.parse().map_err(|_| invalid)?;
        let repeat = match parts.next() {
            None => PlaceRepeat::Always,
            Some("always") => return Err(invalid),
            Some(ending) => ending.parse()?,
        };

        if parts.next().is_some() {
            return Err(invalid);
        }

        Ok(Self {
            event,
            place,
            repeat,
        })
    }
}

impl PlaceTrigger {
    /// Whether `schedule` means to be a place trigger, well formed or not.
    pub fn claims(schedule: &str) -> bool {
        schedule
            .split_once(':')
            .is_some_and(|(event, _)| (event.trim()).parse::<PlaceEvent>().ok().is_some())
    }
}

impl fmt::Display for PlaceTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.event.as_ref(), self.place)?;

        if let Some(suffix) = self.repeat.suffix() {
            write!(f, ":{suffix}")?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub latitude: f64,
    pub longitude: f64,
}

impl Point {
    /// Great-circle distance to `other`, in metres.
    pub fn distance_m(self, other: Point) -> f64 {
        const EARTH_M: f64 = 6_371_000.0;
        let (lat1, lat2) = (self.latitude.to_radians(), other.latitude.to_radians());
        let dlat = lat2 - lat1;
        let dlon = (other.longitude - self.longitude).to_radians();
        let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        2.0 * EARTH_M * h.sqrt().asin()
    }
}

impl From<&Location> for Point {
    fn from(fix: &Location) -> Self {
        Self {
            latitude: fix.latitude,
            longitude: fix.longitude,
        }
    }
}

impl From<&SavedPlace> for Point {
    fn from(place: &SavedPlace) -> Self {
        Self {
            latitude: place.latitude,
            longitude: place.longitude,
        }
    }
}

/// How far `fix` is from the middle of `place`, in metres.
pub fn distance_to(place: &SavedPlace, fix: &Location) -> f64 {
    Point::from(place).distance_m(Point::from(fix))
}

/// Arriving, leaving, or `None`. Leaving needs the whole accuracy circle past the radius by a margin.
pub fn crossing(place: &SavedPlace, was_inside: bool, fix: &Location) -> Option<PlaceEvent> {
    let radius = f64::from(place.radius);
    let distance = distance_to(place, fix);
    let accuracy = fix.accuracy.max(0.0);
    if was_inside {
        let margin = (radius / 2.0).max(EXIT_MARGIN_MIN);
        let left = distance > radius + margin && distance - accuracy > radius;
        left.then_some(PlaceEvent::Leave)
    } else {
        let sure = accuracy <= (radius * 2.0).max(ARRIVE_ACCURACY_MIN);
        (sure && distance <= radius).then_some(PlaceEvent::Arrive)
    }
}

/// The start of today in `timezone`, as UTC.
fn start_of_day(now: DateTime<Utc>, timezone: &str) -> DateTime<Utc> {
    let zone: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    let local = now.with_timezone(&zone).date_naive();
    // Where clocks skip midnight, the day starts at the first hour that exists.
    (0..24)
        .find_map(|hour| {
            local
                .and_hms_opt(hour, 0, 0)?
                .and_local_timezone(zone)
                .earliest()
        })
        .map_or(now, |start| start.with_timezone(&Utc))
}

/// Checks `fix` against the user's places and starts the routines it triggers.
pub async fn track(app: &Arc<App>, user: &Arc<User>, fix: &Location) -> Result<()> {
    for crossing in user.cross_places(fix).await? {
        spawn_routines(app, user, crossing);
    }

    Ok(())
}

/// Starts place `id`'s routines from the phone's own geofencing; returns whether that changed where it was.
pub async fn cross(app: &Arc<App>, user: &Arc<User>, id: Uuid, event: PlaceEvent) -> Result<bool> {
    let Some(crossing) = user.cross_place(id, event).await? else {
        return Ok(false);
    };
    spawn_routines(app, user, crossing);
    Ok(true)
}

/// Starts `crossing`'s routines in the background.
fn spawn_routines(app: &Arc<App>, user: &Arc<User>, crossing: Crossed) {
    log::info!(
        "places: {}; place_id={} user={user}",
        crossing.event.as_ref(),
        crossing.place.id
    );
    let (app, user) = (app.clone(), user.clone());
    tokio::spawn(async move {
        if let Err(error) = start_routines(&app, &user, &crossing).await {
            log::warn!("places: unable to start a routine: {error:#}; user={user}");
        }
    });
}

/// Starts the routines that run on `crossing`, and returns how many did.
pub async fn start_routines(app: &Arc<App>, user: &Arc<User>, crossing: &Crossed) -> Result<usize> {
    let now = Utc::now();
    let today = start_of_day(now, &user.get_timezone().await?);
    let mut started = 0;

    for routine in user
        .place_routines(crossing.place.id, crossing.event)
        .await?
    {
        let since = (routine.trigger.repeat == PlaceRepeat::Daily).then_some(today);
        // One routine that fails leaves the others to start.
        let result: Result<bool> = async {
            if !user.claim_place_run(routine.job.id, now, since).await? {
                log::debug!("places: ran today already; job_id={}", routine.job.id);
                return Ok(false);
            }

            let task = format!(
                "{}\n\n(This started because the user just {}.)",
                routine.job.task.trim(),
                crossing.event.past(&crossing.place.name)
            );
            // Deleted before it starts, so a second crossing cannot run it too.
            if routine.trigger.repeat == PlaceRepeat::Once {
                user.delete_cron(&routine.job.id.to_string()).await?;
            }

            Ok(
                cron::submit(app, user, &routine.job, &task, cron::WhenBusy::Wait)
                    .await?
                    .is_some(),
            )
        }
        .await;

        match result {
            Ok(true) => started += 1,
            Ok(false) => {}
            Err(error) => log::warn!(
                "places: unable to start a routine: {error:#}; job_id={} user={user}",
                routine.job.id
            ),
        }
    }

    Ok(started)
}

#[cfg(test)]
#[path = "../tests/routines_places.rs"]
mod tests;
