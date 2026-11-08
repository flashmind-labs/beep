use super::*;
use crate::store::TestDb;

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
    let trigger = format!("arrive:{}", place.id)
        .parse::<PlaceTrigger>()
        .ok()
        .unwrap();
    let (label, task, long) = ("é".repeat(200), "é".repeat(4000), "é".repeat(4001));
    let fits = NewPlaceRoutine {
        label: &label,
        task: &task,
        trigger,
        chat: None,
    };
    store.create_place_routine(&alice, &fits).await.unwrap();
    let too_long = NewPlaceRoutine {
        task: &long,
        ..fits
    };
    assert!(store.create_place_routine(&alice, &too_long).await.is_err());
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
