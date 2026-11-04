//! The user's location, places, timezone and habits.

use super::User;
use crate::{
    routines::places::PlaceEvent,
    store::{Crossed, Location, NewPlace, NewPlaceRoutine, PlaceRoutine, SavedPlace},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

impl User {
    /// Saves the phone's latest location fix.
    pub async fn update_location(&self, fix: &Location) -> Result<()> {
        self.store.update_location(&self.key, fix).await
    }

    /// The phone's latest location, if it sent one.
    pub async fn get_location(&self) -> Result<Option<Location>> {
        self.store.get_location(&self.key).await
    }

    /// The user's saved places, by name.
    pub async fn list_places(&self) -> Result<Vec<SavedPlace>> {
        self.store.list_places(&self.key).await
    }

    /// The place called `name`, ignoring case.
    pub async fn place_named(&self, name: &str) -> Result<Option<SavedPlace>> {
        self.store.place_named(&self.key, name).await
    }

    /// Saves a place, or moves the one with its name.
    pub async fn save_place(&self, place: &NewPlace<'_>) -> Result<SavedPlace> {
        self.store.save_place(&self.key, place).await
    }

    /// Renames a place or changes its radius; `None` when there is no such place.
    pub async fn update_place(
        &self,
        id: Uuid,
        name: Option<&str>,
        radius: Option<u32>,
    ) -> Result<Option<SavedPlace>> {
        self.store.update_place(&self.key, id, name, radius).await
    }

    /// Forgets a place and its routines; `false` when there is no such place.
    pub async fn delete_place(&self, id: Uuid) -> Result<bool> {
        self.store.delete_place(&self.key, id).await
    }

    /// Records which places `fix` is at and returns those arrived at or left.
    pub async fn cross_places(&self, fix: &Location) -> Result<Vec<Crossed>> {
        self.store.cross_places(&self.key, fix).await
    }

    /// Records the phone arriving at or leaving place `id`; the crossing when it changed anything.
    pub async fn cross_place(&self, id: Uuid, event: PlaceEvent) -> Result<Option<Crossed>> {
        self.store
            .cross_place(&self.key, id, event, Utc::now())
            .await
    }

    /// Saves a routine that starts at a place and returns its id.
    pub async fn create_place_routine(&self, routine: &NewPlaceRoutine<'_>) -> Result<Uuid> {
        let id = self.store.create_place_routine(&self.key, routine).await?;
        self.changed("");
        Ok(id)
    }

    /// The enabled routines that start on `event` at `place`.
    pub async fn place_routines(
        &self,
        place: Uuid,
        event: PlaceEvent,
    ) -> Result<Vec<PlaceRoutine>> {
        self.store.place_routines(&self.key, place, event).await
    }

    /// Records that routine `id` runs now, unless it ran at or after `since`.
    pub async fn claim_place_run(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
        since: Option<DateTime<Utc>>,
    ) -> Result<bool> {
        self.store.claim_place_run(&self.key, id, now, since).await
    }

    /// Saves the timezone the phone is set to.
    pub async fn set_timezone(&self, timezone: &str) -> Result<()> {
        self.store.set_timezone(&self.key, timezone).await
    }

    /// The user's timezone name, `UTC` when unknown.
    pub async fn get_timezone(&self) -> Result<String> {
        self.store.get_timezone(&self.key).await
    }

    /// Starts tracking a habit and returns its id.
    pub async fn create_habit(&self, name: &str, frequency: &str) -> Result<String> {
        self.store.create_habit(&self.key, name, frequency).await
    }

    /// The user's habits that are not archived.
    pub async fn list_habits(&self) -> Result<Vec<Value>> {
        self.store.list_habits(&self.key).await
    }

    /// Logs one entry for a habit.
    pub async fn log_habit(&self, habit_name: &str, value: f64, note: Option<&str>) -> Result<()> {
        self.store
            .log_habit(&self.key, habit_name, value, note)
            .await
    }

    /// A habit's daily totals over the last `days`.
    pub async fn habit_status(&self, habit_name: &str, days: i64) -> Result<Value> {
        self.store.habit_status(&self.key, habit_name, days).await
    }

    /// Stops tracking a habit; `false` when the user has no such habit.
    pub async fn archive_habit(&self, habit_name: &str) -> Result<bool> {
        self.store.archive_habit(&self.key, habit_name).await
    }
}
