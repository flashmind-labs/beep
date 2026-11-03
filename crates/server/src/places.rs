//! Saved places, such as Home or Gym, and the routines that start on
//! arriving at or leaving one.
//!
//! Every location fix the phone sends is checked against the user's places.
//! A place is entered once the fix is inside its radius, and only left once
//! the fix is clearly outside a wider ring, so GPS jitter at the edge does
//! not flap between the two.

use crate::{
    app::App,
    cron,
    protocol::PlaceCrossing,
    store::{Crossed, Location, SavedPlace},
    user::User,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::{fmt, sync::Arc};
use uuid::Uuid;

/// How near counts as being at a place, in metres, unless the user says.
pub const DEFAULT_RADIUS: u32 = 100;
/// The smallest radius a place can have; GPS is rarely better indoors.
pub const MIN_RADIUS: u32 = 25;
/// The largest radius a place can have.
pub const MAX_RADIUS: u32 = 1000;

/// The least a fix must be past the radius before it counts as leaving.
const EXIT_MARGIN_MIN: f64 = 50.0;
/// A fix less sure than this many metres, or twice the radius when that is
/// more, cannot count as arriving.
const ARRIVE_ACCURACY_MIN: f64 = 100.0;

/// Arriving at a place or leaving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceEvent {
    Arrive,
    Leave,
}

impl PlaceEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Arrive => "arrive",
            Self::Leave => "leave",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "arrive" => Some(Self::Arrive),
            "leave" => Some(Self::Leave),
            _ => None,
        }
    }

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Reads the tool's word for it: `always`, `daily` or `once`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "always" => Some(Self::Always),
            "daily" => Some(Self::Daily),
            "once" => Some(Self::Once),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.suffix().unwrap_or("always")
    }
}

/// A routine's schedule when it starts at a place instead of at a time:
/// `arrive:<place id>` or `leave:<place id>`, ending in `:daily` for once a
/// day at most or `:once` for only the next time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceTrigger {
    pub event: PlaceEvent,
    pub place: Uuid,
    pub repeat: PlaceRepeat,
}

impl PlaceTrigger {
    /// Reads a schedule; `None` for a time or anything else.
    pub fn parse(schedule: &str) -> Option<Self> {
        let mut parts = schedule.trim().split(':');
        let event = PlaceEvent::parse(parts.next()?)?;
        let place = parts.next()?.parse().ok()?;
        let repeat = match parts.next() {
            None => PlaceRepeat::Always,
            Some("always") => return None,
            Some(ending) => PlaceRepeat::parse(ending)?,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            event,
            place,
            repeat,
        })
    }

    /// Whether `schedule` means to be a place trigger, well formed or not.
    pub fn claims(schedule: &str) -> bool {
        schedule
            .split_once(':')
            .is_some_and(|(event, _)| PlaceEvent::parse(event.trim()).is_some())
    }
}

impl fmt::Display for PlaceTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.event.as_str(), self.place)?;
        if let Some(suffix) = self.repeat.suffix() {
            write!(f, ":{suffix}")?;
        }
        Ok(())
    }
}

/// A point on the map.
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

/// What `fix` means for `place`, given whether the phone was there:
/// arriving, leaving, or `None` for no change.
///
/// Arriving needs the fix inside the radius and sure enough to say so.
/// Leaving needs it past the radius by a margin, and its whole accuracy
/// circle outside the radius, so a fix wandering near the edge or a vague
/// one indoors keeps the phone where it was.
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

/// Checks `fix` against the user's places, and starts the routines of
/// every place it arrived at or left. Called with each fix the phone sends.
pub async fn track(app: &Arc<App>, user: &Arc<User>, fix: &Location) -> Result<()> {
    for crossing in user.cross_places(fix).await? {
        spawn_routines(app, user, crossing);
    }
    Ok(())
}

/// Starts the routines of place `id` when the phone's own geofencing saw it
/// arrive or leave. Returns whether that changed where the phone was.
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
        crossing.event.as_str(),
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
mod tests {
    use super::*;

    fn gym(radius: u32) -> SavedPlace {
        SavedPlace {
            id: Uuid::nil(),
            name: "Gym".to_owned(),
            latitude: 41.4,
            longitude: 2.2,
            radius,
            address: None,
            created_at: Utc::now(),
            inside: false,
            last_here_at: None,
            routines: 0,
        }
    }

    /// A fix `metres` north of the gym.
    fn north(metres: f64, accuracy: f64) -> Location {
        Location {
            latitude: 41.4 + metres / 111_195.0,
            longitude: 2.2,
            accuracy,
            timezone: "Europe/Madrid".to_owned(),
            place: None,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn arriving_needs_a_fix_inside_the_radius() {
        let place = gym(100);
        assert_eq!(
            crossing(&place, false, &north(80.0, 20.0)),
            Some(PlaceEvent::Arrive)
        );
        assert_eq!(crossing(&place, false, &north(130.0, 20.0)), None);
        // Too vague to say.
        assert_eq!(crossing(&place, false, &north(10.0, 500.0)), None);
        // Already there.
        assert_eq!(crossing(&place, true, &north(10.0, 20.0)), None);
    }

    #[test]
    fn leaving_needs_a_fix_clearly_outside() {
        let place = gym(100);
        // Past the radius, but inside the margin of 50 m.
        assert_eq!(crossing(&place, true, &north(140.0, 10.0)), None);
        // Past the margin, but the fix could still be inside.
        assert_eq!(crossing(&place, true, &north(160.0, 80.0)), None);
        assert_eq!(
            crossing(&place, true, &north(160.0, 20.0)),
            Some(PlaceEvent::Leave)
        );
        assert_eq!(
            crossing(&place, true, &north(5000.0, 1000.0)),
            Some(PlaceEvent::Leave)
        );
        // A big place leaves at half its radius past the edge.
        let park = gym(400);
        assert_eq!(crossing(&park, true, &north(590.0, 10.0)), None);
        assert_eq!(
            crossing(&park, true, &north(610.0, 10.0)),
            Some(PlaceEvent::Leave)
        );
    }

    #[test]
    fn place_triggers_read_back() {
        let place = Uuid::new_v4();
        for trigger in [
            PlaceTrigger {
                event: PlaceEvent::Arrive,
                place,
                repeat: PlaceRepeat::Daily,
            },
            PlaceTrigger {
                event: PlaceEvent::Leave,
                place,
                repeat: PlaceRepeat::Always,
            },
            PlaceTrigger {
                event: PlaceEvent::Leave,
                place,
                repeat: PlaceRepeat::Once,
            },
        ] {
            assert_eq!(PlaceTrigger::parse(&trigger.to_string()), Some(trigger));
        }
        assert_eq!(PlaceTrigger::parse("0 9 * * *"), None);
        assert_eq!(PlaceTrigger::parse("2026-10-05T09:00:00+00:00"), None);
        assert_eq!(PlaceTrigger::parse("arrive:gym"), None);
        assert_eq!(PlaceTrigger::parse(&format!("arrive:{place}:weekly")), None);
        assert_eq!(PlaceTrigger::parse(&format!("arrive:{place}:always")), None);
        assert!(PlaceTrigger::claims("arrive:gym"));
        assert!(!PlaceTrigger::claims("2026-10-05T09:00:00+00:00"));
        assert!(!PlaceTrigger::claims("0 9 * * *"));
    }

    #[test]
    fn a_day_starts_at_local_midnight() {
        let now = "2026-10-05T23:30:00Z".parse::<DateTime<Utc>>().unwrap();
        let madrid = start_of_day(now, "Europe/Madrid");
        assert_eq!(madrid.to_rfc3339(), "2026-10-05T22:00:00+00:00");
        let utc = start_of_day(now, "UTC");
        assert_eq!(utc.to_rfc3339(), "2026-10-05T00:00:00+00:00");
        // Santiago skips from 00:00 to 01:00 on 2026-09-06.
        let now = "2026-09-06T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let santiago = start_of_day(now, "America/Santiago");
        assert_eq!(santiago.to_rfc3339(), "2026-09-06T04:00:00+00:00");
    }
}
