//! Saved places, whether the phone is at each, and routines that start there.

use super::{
    Location, Store,
    cron_jobs::{COLUMNS, job_from},
};
use crate::{
    account::user::UserKey,
    routines::places::{self, PlaceEvent, PlaceRepeat, PlaceTrigger},
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use flashmind::cron::{CronJob, JobSchedule};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewPlaceRoutine<'a> {
    pub label: &'a str,
    /// What the agent is told to do when it starts.
    pub task: &'a str,
    pub trigger: PlaceTrigger,
    /// The chat it was set up from, where its runs reply.
    pub chat: Option<Uuid>,
}

/// A place as the store keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedPlace {
    pub id: Uuid,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// Metres.
    pub radius: u32,
    pub address: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Whether the phone's last usable fix was there.
    pub inside: bool,
    pub last_here_at: Option<DateTime<Utc>>,
    /// How many routines start there.
    pub routines: u32,
}

/// A place to save. A place with the same name, ignoring case, moves here.
#[derive(Debug, Clone)]
pub struct NewPlace<'a> {
    pub name: &'a str,
    pub latitude: f64,
    pub longitude: f64,
    pub radius: u32,
    pub address: Option<&'a str>,
    /// Whether the phone is there now, so saving where it stands is not an arrival.
    pub inside: bool,
}

/// A place a fix arrived at or left.
#[derive(Debug, Clone)]
pub struct Crossed {
    pub place: SavedPlace,
    pub event: PlaceEvent,
}

#[derive(Debug, Clone)]
pub struct PlaceRoutine {
    pub job: CronJob,
    pub trigger: PlaceTrigger,
}

const PLACE_COLUMNS: &str = "id, name, latitude, longitude, radius, address, created_at, inside, \
    last_here_at, (SELECT COUNT(*) FROM crons c WHERE c.user_id=places.user_id AND c.place_id=places.id)";

fn place_from(row: &PgRow) -> Result<SavedPlace> {
    let id: String = row.try_get(0)?;
    let radius: i32 = row.try_get(4)?;
    let routines: i64 = row.try_get(9)?;
    Ok(SavedPlace {
        id: id.parse()?,
        name: row.try_get(1)?,
        latitude: row.try_get(2)?,
        longitude: row.try_get(3)?,
        radius: u32::try_from(radius)?,
        address: row.try_get(5)?,
        created_at: row.try_get(6)?,
        inside: row.try_get(7)?,
        last_here_at: row.try_get(8)?,
        routines: u32::try_from(routines)?,
    })
}

fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 100 {
        bail!("a place name must be 1-100 characters");
    }

    Ok(())
}

fn check_radius(radius: u32) -> Result<()> {
    if !(places::MIN_RADIUS..=places::MAX_RADIUS).contains(&radius) {
        bail!(
            "a place's radius must be {}-{} metres",
            places::MIN_RADIUS,
            places::MAX_RADIUS
        );
    }

    Ok(())
}

impl Store {
    /// The user's places, by name.
    pub(crate) async fn list_places(&self, user: &UserKey) -> Result<Vec<SavedPlace>> {
        let rows = sqlx::query(&format!(
            "SELECT {PLACE_COLUMNS} FROM places WHERE user_id=$1 ORDER BY lower(name) LIMIT 200"
        ))
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(place_from).collect()
    }

    pub(crate) async fn place(&self, user: &UserKey, id: Uuid) -> Result<Option<SavedPlace>> {
        let row = sqlx::query(&format!(
            "SELECT {PLACE_COLUMNS} FROM places WHERE user_id=$1 AND id=$2"
        ))
        .bind(user.as_ref())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(place_from).transpose()
    }

    /// The place called `name`, ignoring case.
    pub(crate) async fn place_named(
        &self,
        user: &UserKey,
        name: &str,
    ) -> Result<Option<SavedPlace>> {
        let row = sqlx::query(&format!(
            "SELECT {PLACE_COLUMNS} FROM places WHERE user_id=$1 AND lower(name)=lower($2)"
        ))
        .bind(user.as_ref())
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(place_from).transpose()
    }

    /// Saves a place, or moves the same-named one here, keeping its id and routines.
    pub(crate) async fn save_place(
        &self,
        user: &UserKey,
        new: &NewPlace<'_>,
    ) -> Result<SavedPlace> {
        check_name(new.name)?;
        check_radius(new.radius)?;
        let valid =
            (-90.0..=90.0).contains(&new.latitude) && (-180.0..=180.0).contains(&new.longitude);
        if !valid {
            bail!("invalid coordinates");
        }

        let id: String = sqlx::query_scalar(
            "INSERT INTO places (user_id, id, name, latitude, longitude, radius, address, inside,
                 last_here_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CASE WHEN $8 THEN now() END)
             ON CONFLICT (user_id, (lower(name))) DO UPDATE SET name=excluded.name,
                 latitude=excluded.latitude, longitude=excluded.longitude,
                 radius=excluded.radius, address=excluded.address, inside=excluded.inside,
                 last_here_at=COALESCE(excluded.last_here_at, places.last_here_at)
             RETURNING id",
        )
        .bind(user.as_ref())
        .bind(Uuid::new_v4().to_string())
        .bind(new.name.trim())
        .bind(new.latitude)
        .bind(new.longitude)
        .bind(i32::try_from(new.radius)?)
        .bind(new.address.map(str::trim).filter(|a| !a.is_empty()))
        .bind(new.inside)
        .fetch_one(&self.pool)
        .await?;
        let place = self.place(user, id.parse()?).await?;
        place.ok_or_else(|| anyhow::anyhow!("the saved place is gone"))
    }

    /// Renames a place or changes its radius; `None` when missing.
    pub(crate) async fn update_place(
        &self,
        user: &UserKey,
        id: Uuid,
        name: Option<&str>,
        radius: Option<u32>,
    ) -> Result<Option<SavedPlace>> {
        if let Some(name) = name {
            check_name(name)?;
        }

        if let Some(radius) = radius {
            check_radius(radius)?;
        }

        let taken = match name {
            Some(name) => self
                .place_named(user, name)
                .await?
                .is_some_and(|other| other.id != id),
            None => false,
        };

        if taken {
            bail!("another place already has that name");
        }

        let radius = radius.map(i32::try_from).transpose()?;
        let updated = sqlx::query(
            "UPDATE places SET name=COALESCE($3, name), radius=COALESCE($4, radius)
             WHERE user_id=$1 AND id=$2",
        )
        .bind(user.as_ref())
        .bind(id.to_string())
        .bind(name.map(str::trim))
        .bind(radius)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if updated == 0 {
            return Ok(None);
        }

        self.place(user, id).await
    }

    /// Forgets a place and its routines; false when missing.
    pub(crate) async fn delete_place(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM places WHERE user_id=$1 AND id=$2")
            .bind(user.as_ref())
            .bind(id.to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    /// Checks `fix` against every place and returns crossings, each once even when fixes race.
    pub(crate) async fn cross_places(
        &self,
        user: &UserKey,
        fix: &Location,
    ) -> Result<Vec<Crossed>> {
        let mut crossed = Vec::new();

        for place in self.list_places(user).await? {
            let Some(event) = places::crossing(&place, place.inside, fix) else {
                continue;
            };
            let inside = event == PlaceEvent::Arrive;
            let changed = sqlx::query(
                "UPDATE places SET inside=$3,
                     last_here_at=CASE WHEN $3 THEN $4 ELSE last_here_at END
                 WHERE user_id=$1 AND id=$2 AND inside<>$3",
            )
            .bind(user.as_ref())
            .bind(place.id.to_string())
            .bind(inside)
            .bind(fix.updated_at)
            .execute(&self.pool)
            .await?
            .rows_affected();
            if changed > 0 {
                crossed.push(Crossed { place, event });
            }
        }

        sqlx::query("UPDATE places SET last_here_at=$2 WHERE user_id=$1 AND inside")
            .bind(user.as_ref())
            .bind(fix.updated_at)
            .execute(&self.pool)
            .await?;
        Ok(crossed)
    }

    /// Records a geofence crossing of place `id` at `at`; `None` if already known or no such place.
    pub(crate) async fn cross_place(
        &self,
        user: &UserKey,
        id: Uuid,
        event: PlaceEvent,
        at: DateTime<Utc>,
    ) -> Result<Option<Crossed>> {
        let inside = event == PlaceEvent::Arrive;
        let changed = sqlx::query(
            "UPDATE places SET inside=$3, last_here_at=$4
             WHERE user_id=$1 AND id=$2 AND inside<>$3",
        )
        .bind(user.as_ref())
        .bind(id.to_string())
        .bind(inside)
        .bind(at)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if changed == 0 {
            return Ok(None);
        }

        let place = self.place(user, id).await?;
        Ok(place.map(|place| Crossed { place, event }))
    }

    /// The enabled routines that start on `event` at place `place`.
    pub(crate) async fn place_routines(
        &self,
        user: &UserKey,
        place: Uuid,
        event: PlaceEvent,
    ) -> Result<Vec<PlaceRoutine>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM crons WHERE user_id=$1 AND place_id=$2 AND enabled
             ORDER BY created_at"
        ))
        .bind(user.as_ref())
        .bind(place.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut routines = Vec::new();

        for row in &rows {
            let job = job_from(row)?;
            let JobSchedule::Cron(schedule) = &job.schedule else {
                continue;
            };
            let Some(trigger) = schedule.parse::<PlaceTrigger>().ok() else {
                continue;
            };
            if trigger.event == event {
                routines.push(PlaceRoutine { job, trigger });
            }
        }

        Ok(routines)
    }

    /// Claims routine `id` to run at `now` unless it ran since `since`; false to skip.
    pub(crate) async fn claim_place_run(
        &self,
        user: &UserKey,
        id: Uuid,
        now: DateTime<Utc>,
        since: Option<DateTime<Utc>>,
    ) -> Result<bool> {
        let claimed = sqlx::query(
            "UPDATE crons SET last_run_at=$3
             WHERE user_id=$1 AND id=$2 AND ($4::timestamptz IS NULL OR last_run_at IS NULL
                 OR last_run_at < $4)",
        )
        .bind(user.as_ref())
        .bind(id.to_string())
        .bind(now)
        .bind(since)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(claimed > 0)
    }

    /// Saves a routine that starts at a place, and returns its id.
    pub(crate) async fn create_place_routine(
        &self,
        user: &UserKey,
        routine: &NewPlaceRoutine<'_>,
    ) -> Result<Uuid> {
        let label = routine.label.trim();
        let task = routine.task.trim();

        if label.is_empty() || label.chars().count() > 200 {
            bail!("a routine's label must be 1-200 characters");
        }

        if task.is_empty() || task.chars().count() > 4000 {
            bail!("a routine's task must be 1-4000 characters");
        }

        if self.place(user, routine.trigger.place).await?.is_none() {
            bail!("no such place");
        }

        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule, run_once, chat_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(user.as_ref())
        .bind(id.to_string())
        .bind(label)
        .bind(task)
        .bind(routine.trigger.to_string())
        .bind(routine.trigger.repeat == PlaceRepeat::Once)
        .bind(routine.chat)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Fails when a place `schedule` is malformed or names an unknown place.
    pub(crate) async fn check_place_schedule(&self, user: &UserKey, schedule: &str) -> Result<()> {
        if !PlaceTrigger::claims(schedule) {
            return Ok(());
        }

        let Some(trigger) = schedule.parse::<PlaceTrigger>().ok() else {
            bail!(
                "a place schedule is arrive:<place id> or leave:<place id>, ending in :daily for once a day at most or :once for only the next time"
            );
        };

        if self.place(user, trigger.place).await?.is_none() {
            bail!("no such place");
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/store_places.rs"]
mod tests;
