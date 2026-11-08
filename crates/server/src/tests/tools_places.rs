use super::*;
use crate::store::TestDb;
use flashmind::core::CancellationToken;

async fn call(user: &Arc<User>, kind: PlaceToolKind, args: Value) -> ToolResult {
    let tool = PlaceTool {
        user: user.clone(),
        kind,
        chat: None,
    };
    let cancel = CancellationToken::new();
    tool.execute(ToolContext::new("call", args, None, &cancel))
        .await
        .expect("tool runs")
}

fn at_gym() -> Location {
    Location {
        latitude: 41.4,
        longitude: 2.2,
        accuracy: 15.0,
        timezone: "Europe/Madrid".to_owned(),
        place: Some("Poblenou, Barcelona, Spain".to_owned()),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn im_at_the_gym_saves_where_the_phone_is() {
    let db = TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");

    let off = call(&user, PlaceToolKind::Save, json!({"name": "Gym"})).await;
    assert!(!off.is_success(), "no location yet");

    user.update_location(&at_gym()).await.unwrap();
    let saved = call(&user, PlaceToolKind::Save, json!({"name": "Gym"})).await;
    assert!(saved.is_success(), "{}", saved.output());
    let place = user.place_named("gym").await.unwrap().expect("saved");
    assert_eq!(place.address.as_deref(), Some("Poblenou, Barcelona, Spain"));
    assert_eq!(place.radius, places::DEFAULT_RADIUS);
    assert!(place.inside, "saving where it stands is not an arrival");

    // Another name 40 m away asks first.
    let mut near = at_gym();
    near.latitude += 40.0 / 111_195.0;
    user.update_location(&near).await.unwrap();
    let asked = call(&user, PlaceToolKind::Save, json!({"name": "Climbing"})).await;
    let output: Value = serde_json::from_str(&asked.output()).unwrap();
    assert_eq!(output["saved"], false);
    assert_eq!(output["nearby"], "Gym");
    assert_eq!(output["distance_m"], 40.0);
    let forced = call(
        &user,
        PlaceToolKind::Save,
        json!({"name": "Climbing", "new_place": true}),
    )
    .await;
    assert!(forced.is_success());
    assert_eq!(user.list_places().await.unwrap().len(), 2);

    let routine = call(
            &user,
            PlaceToolKind::Routine,
            json!({"place": "gym", "when": "arrive", "label": "Log my weights", "task": "Remind me to log weights", "repeat": "daily"}),
        )
        .await;
    assert!(routine.is_success(), "{}", routine.output());
    let crons = user.list_crons().await.unwrap();
    assert_eq!(crons.len(), 1);
    assert_eq!(
        crons[0].schedule.parse::<PlaceTrigger>().ok(),
        Some(PlaceTrigger {
            event: PlaceEvent::Arrive,
            place: place.id,
            repeat: PlaceRepeat::Daily,
        })
    );

    let listed = call(&user, PlaceToolKind::List, json!({})).await;
    let output: Value = serde_json::from_str(&listed.output()).unwrap();
    assert_eq!(output["places"][1]["name"], "Gym");
    assert_eq!(output["places"][1]["routines"], 1);

    let deleted = call(&user, PlaceToolKind::Delete, json!({"name": "Gym"})).await;
    assert!(deleted.is_success());
    assert!(user.list_crons().await.unwrap().is_empty());
    let missing = call(&user, PlaceToolKind::Delete, json!({"name": "Gym"})).await;
    assert!(!missing.is_success());
}
