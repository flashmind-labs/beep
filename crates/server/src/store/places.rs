//! The user's saved places, whether the phone is at each, and the routines
//! that start at them.

use super::{
    Location, Store,
    cron_jobs::{COLUMNS, job_from},
};
use crate::{
    agents::AgentId,
    places::{self, PlaceEvent, PlaceRepeat, PlaceTrigger},
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use flashmind::cron::{CronJob, JobSchedule};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// A routine to start at a place.
#[derive(Debug, Clone)]
pub struct NewPlaceRoutine<'a> {
    pub label: &'a str,
    /// What the agent is told to do when it starts.
    pub task: &'a str,
    pub trigger: PlaceTrigger,
    /// The chat it was set up from, where its runs reply.
    pub chat: Option<Uuid>,
    /// The agent of the turn that set it up, which its runs answer as.
    pub agent: Option<AgentId>,
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
    /// Whether the phone is there now, so saving where it stands is not
    /// an arrival.
    pub inside: bool,
}

/// A place a fix arrived at or left.
#[derive(Debug, Clone)]
pub struct Crossed {
    pub place: SavedPlace,
    pub event: PlaceEvent,
}

/// A routine that starts at a place.
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
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(place_from).collect()
    }

    pub(crate) async fn place(&self, user: &UserKey, id: Uuid) -> Result<Option<SavedPlace>> {
        let row = sqlx::query(&format!(
            "SELECT {PLACE_COLUMNS} FROM places WHERE user_id=$1 AND id=$2"
        ))
        .bind(user.as_str())
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
        .bind(user.as_str())
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(place_from).transpose()
    }

    /// Saves a place, or moves the one with its name here, keeping its id
    /// and routines.
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
        .bind(user.as_str())
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

    /// Renames a place or changes its radius; `None` when there is no such
    /// place.
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
        .bind(user.as_str())
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

    /// Forgets a place and the routines that start there; false when there
    /// is no such place.
    pub(crate) async fn delete_place(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM places WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id.to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    /// Checks `fix` against every place, records which the phone is now at,
    /// and returns the places it arrived at or left. Each crossing is
    /// returned once, even when two fixes race.
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
            .bind(user.as_str())
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
            .bind(user.as_str())
            .bind(fix.updated_at)
            .execute(&self.pool)
            .await?;
        Ok(crossed)
    }

    /// Records that the phone's own geofencing saw it arrive at or leave
    /// place `id` at `at`. Returns the crossing when it changed where the
    /// phone was, `None` when it was already known or there is no such place.
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
        .bind(user.as_str())
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
        .bind(user.as_str())
        .bind(place.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut routines = Vec::new();
        for row in &rows {
            let job = job_from(row)?;
            let JobSchedule::Cron(schedule) = &job.schedule else {
                continue;
            };
            let Some(trigger) = PlaceTrigger::parse(schedule) else {
                continue;
            };
            if trigger.event == event {
                routines.push(PlaceRoutine { job, trigger });
            }
        }
        Ok(routines)
    }

    /// Records that routine `id` runs at `now`, unless it already ran at or
    /// after `since`. False when it did, so it is skipped.
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
        .bind(user.as_str())
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
            "INSERT INTO crons (user_id, id, label, prompt, schedule, run_once, chat_id, agent)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(user.as_str())
        .bind(id.to_string())
        .bind(label)
        .bind(task)
        .bind(routine.trigger.to_string())
        .bind(routine.trigger.repeat == PlaceRepeat::Once)
        .bind(routine.chat)
        .bind(routine.agent.map(AgentId::as_str))
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Fails when `schedule` is meant as a place trigger but is malformed or
    /// names a place the user does not have.
    pub(crate) async fn check_place_schedule(&self, user: &UserKey, schedule: &str) -> Result<()> {
        if !PlaceTrigger::claims(schedule) {
            return Ok(());
        }
        let Some(trigger) = PlaceTrigger::parse(schedule) else {
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
mod tests {
    use super::*;
    use crate::{protocol::Cron, store::TestDb};

    fn gym<'a>() -> NewPlace<'a> {
        NewPlace {
            name: "Gym",
            latitude: 41.4,
            longitude: 2.2,
            radius: 100,
            address: Some("Carrer de Pujades 102"),
            inside: false,
        }
    }

    /// A fix `metres` north of the gym.
    fn north(metres: f64) -> Location {
        Location {
            latitude: 41.4 + metres / 111_195.0,
            longitude: 2.2,
            accuracy: 15.0,
            timezone: "Europe/Madrid".to_owned(),
            place: None,
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn places_are_saved_moved_renamed_and_deleted_per_user() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");

        let saved = store.save_place(&alice, &gym()).await.unwrap();
        assert_eq!(saved.name, "Gym");
        assert_eq!(saved.radius, 100);
        assert_eq!(saved.address.as_deref(), Some("Carrer de Pujades 102"));
        assert!(!saved.inside);
        assert_eq!(saved.last_here_at, None);
        assert!(store.list_places(&bob).await.unwrap().is_empty());

        // The same name, in any case, moves it and keeps its id.
        let moved = NewPlace {
            name: "gym",
            latitude: 41.5,
            inside: true,
            ..gym()
        };
        let moved = store.save_place(&alice, &moved).await.unwrap();
        assert_eq!(moved.id, saved.id);
        assert_eq!(moved.latitude, 41.5);
        assert!(moved.inside);
        assert!(moved.last_here_at.is_some());
        assert_eq!(store.list_places(&alice).await.unwrap().len(), 1);

        let home = NewPlace {
            name: "Home",
            radius: 150,
            ..gym()
        };
        let home = store.save_place(&alice, &home).await.unwrap();
        let names: Vec<_> = (store.list_places(&alice).await.unwrap().into_iter())
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["gym", "Home"]);

        let renamed = store
            .update_place(&alice, saved.id, Some("Box"), Some(50))
            .await
            .unwrap()
            .expect("the place exists");
        assert_eq!((renamed.name.as_str(), renamed.radius), ("Box", 50));
        assert!(
            store
                .update_place(&alice, saved.id, Some("home"), None)
                .await
                .is_err(),
            "names stay unique"
        );
        assert!(
            store
                .update_place(&alice, saved.id, None, Some(5))
                .await
                .is_err(),
            "too small"
        );
        assert_eq!(
            store
                .update_place(&bob, saved.id, Some("Mine"), None)
                .await
                .unwrap(),
            None
        );

        assert!(!store.delete_place(&bob, home.id).await.unwrap());
        assert!(store.delete_place(&alice, home.id).await.unwrap());
        assert!(!store.delete_place(&alice, home.id).await.unwrap());
        assert!(
            store
                .save_place(&alice, &NewPlace { name: " ", ..gym() })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn fixes_cross_a_place_once_each_way() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let place = store.save_place(&alice, &gym()).await.unwrap();

        let events = async |metres| {
            (store
                .cross_places(&alice, &north(metres))
                .await
                .unwrap()
                .into_iter())
            .map(|c| (c.place.id, c.event))
            .collect::<Vec<_>>()
        };
        assert_eq!(events(500.0).await, []);
        assert_eq!(events(50.0).await, [(place.id, PlaceEvent::Arrive)]);
        assert_eq!(events(20.0).await, []);
        // Jitter just past the edge stays.
        assert_eq!(events(130.0).await, []);
        assert!(store.place(&alice, place.id).await.unwrap().unwrap().inside);
        assert_eq!(events(400.0).await, [(place.id, PlaceEvent::Leave)]);
        assert_eq!(events(450.0).await, []);
        let left = store.place(&alice, place.id).await.unwrap().unwrap();
        assert!(!left.inside);
        assert!(left.last_here_at.is_some());
    }

    #[tokio::test]
    async fn the_phone_crosses_a_place_once_each_way() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let place = store.save_place(&alice, &gym()).await.unwrap();
        let now = Utc::now();

        let cross = async |user: &UserKey, event| {
            (store.cross_place(user, place.id, event, now).await.unwrap())
                .map(|crossed| (crossed.place.id, crossed.event))
        };
        assert_eq!(cross(&bob, PlaceEvent::Arrive).await, None, "not bob's");
        assert_eq!(
            cross(&alice, PlaceEvent::Arrive).await,
            Some((place.id, PlaceEvent::Arrive))
        );
        assert_eq!(cross(&alice, PlaceEvent::Arrive).await, None, "once");
        // A fix that agrees changes nothing.
        assert!(
            store
                .cross_places(&alice, &north(10.0))
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            cross(&alice, PlaceEvent::Leave).await,
            Some((place.id, PlaceEvent::Leave))
        );
        let left = store.place(&alice, place.id).await.unwrap().unwrap();
        assert!(!left.inside);
        assert!(left.last_here_at.is_some());
        let missing = store
            .cross_place(&alice, Uuid::new_v4(), PlaceEvent::Arrive, now)
            .await
            .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn limits_count_characters_not_bytes() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let name = "é".repeat(100);
        let place = store
            .save_place(
                &alice,
                &NewPlace {
                    name: &name,
                    ..gym()
                },
            )
            .await
            .unwrap();
        let name = "é".repeat(101);
        assert!(
            store
                .save_place(
                    &alice,
                    &NewPlace {
                        name: &name,
                        ..gym()
                    }
                )
                .await
                .is_err()
        );
        let trigger = PlaceTrigger::parse(&format!("arrive:{}", place.id)).unwrap();
        let (label, task, long) = ("é".repeat(200), "é".repeat(4000), "é".repeat(4001));
        let fits = NewPlaceRoutine {
            label: &label,
            task: &task,
            trigger,
            chat: None,
            agent: None,
        };
        store.create_place_routine(&alice, &fits).await.unwrap();
        let too_long = NewPlaceRoutine {
            task: &long,
            ..fits
        };
        assert!(store.create_place_routine(&alice, &too_long).await.is_err());
    }

    #[tokio::test]
    async fn a_place_routine_replies_in_the_chat_and_as_the_agent_it_came_from() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let place = store.save_place(&alice, &gym()).await.unwrap();
        let chat = Uuid::new_v4();
        let routine = NewPlaceRoutine {
            label: "Shop",
            task: "Point the user to the shop",
            trigger: PlaceTrigger::parse(&format!("leave:{}:once", place.id)).unwrap(),
            chat: Some(chat),
            agent: Some(AgentId::Shopping),
        };
        store.create_place_routine(&alice, &routine).await.unwrap();
        let routines = store
            .place_routines(&alice, place.id, PlaceEvent::Leave)
            .await
            .unwrap();
        assert_eq!(
            routines[0].job.metadata["session_id"],
            chat.to_string().as_str()
        );
        assert_eq!(routines[0].job.metadata["agent"], "shopping");
    }

    #[tokio::test]
    async fn a_place_schedule_says_whether_it_runs_once() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let place = store.save_place(&alice, &gym()).await.unwrap();
        let once = format!("leave:{}:once", place.id);
        let id = store
            .create_cron(&alice, "Shop", &once, "Point me to the shop")
            .await
            .unwrap();
        let run_once = |crons: Vec<Cron>| crons.iter().find(|c| c.id == id).unwrap().run_once;
        assert!(run_once(store.list_crons(&alice).await.unwrap()));

        let always = format!("leave:{}", place.id);
        let edited = store.update_cron(&alice, &id, None, None, Some(&always));
        assert!(edited.await.unwrap());
        assert!(!run_once(store.list_crons(&alice).await.unwrap()));

        // Leaving a place for a clock schedule stops it running once.
        let edited = store.update_cron(&alice, &id, None, None, Some(&once));
        assert!(edited.await.unwrap());
        let edited = store.update_cron(&alice, &id, None, None, Some("0 9 * * *"));
        assert!(edited.await.unwrap());
        assert!(!run_once(store.list_crons(&alice).await.unwrap()));
    }

    #[tokio::test]
    async fn a_place_schedule_is_stored_trimmed() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let place = store.save_place(&alice, &gym()).await.unwrap();
        let schedule = format!(" arrive:{} ", place.id);
        store
            .create_cron(&alice, "Gym", &schedule, "Log the workout")
            .await
            .unwrap();
        let routines = store
            .place_routines(&alice, place.id, PlaceEvent::Arrive)
            .await
            .unwrap();
        assert_eq!(routines.len(), 1);
    }

    #[tokio::test]
    async fn place_routines_go_with_their_place() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let place = store.save_place(&alice, &gym()).await.unwrap();
        let arrive = format!("arrive:{}:daily", place.id);
        let leave = format!("leave:{}", place.id);

        // Only real places of the caller's.
        assert!(
            store
                .create_cron(&bob, "Weights", &arrive, "Log weights")
                .await
                .is_err()
        );
        assert!(
            store
                .create_cron(&alice, "Weights", "arrive:gym", "Log weights")
                .await
                .is_err()
        );
        let weights = store
            .create_cron(&alice, "Weights", &arrive, "Log weights")
            .await
            .unwrap();
        store
            .create_cron(&alice, "Stretch", &leave, "Stretch")
            .await
            .unwrap();
        store
            .create_cron(&alice, "Morning", "0 8 * * *", "Morning")
            .await
            .unwrap();

        let arrivals = store
            .place_routines(&alice, place.id, PlaceEvent::Arrive)
            .await
            .unwrap();
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].job.id.to_string(), weights);
        assert_eq!(arrivals[0].trigger.repeat, PlaceRepeat::Daily);
        let place_now = store.place(&alice, place.id).await.unwrap().unwrap();
        assert_eq!(place_now.routines, 2);

        // The scheduler never sees them; the user's own list does.
        let scheduled = store.cron_jobs(None).await.unwrap();
        assert_eq!(scheduled.len(), 1);
        assert_eq!(store.cron_jobs(Some("alice")).await.unwrap().len(), 3);

        // Once a day at most.
        let id = arrivals[0].job.id;
        let now = Utc::now();
        let today = now - chrono::TimeDelta::hours(1);
        assert!(
            store
                .claim_place_run(&alice, id, now, Some(today))
                .await
                .unwrap()
        );
        assert!(
            !store
                .claim_place_run(&alice, id, now, Some(today))
                .await
                .unwrap()
        );
        assert!(store.claim_place_run(&alice, id, now, None).await.unwrap());

        // A paused routine does not run.
        store.toggle_cron(&alice, &weights).await.unwrap();
        assert!(
            store
                .place_routines(&alice, place.id, PlaceEvent::Arrive)
                .await
                .unwrap()
                .is_empty()
        );

        assert!(store.delete_place(&alice, place.id).await.unwrap());
        let left: Vec<_> = (store.list_crons(&alice).await.unwrap().into_iter())
            .map(|c| c.label)
            .collect();
        assert_eq!(left, ["Morning"]);
    }
}
