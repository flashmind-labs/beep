use crate::{
    places::{self, PlaceEvent, PlaceRepeat, PlaceTrigger, Point},
    store::{Location, NewPlace, NewPlaceRoutine, SavedPlace},
    user::User,
};
use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

/// How old the phone's location may be for "I'm here" to mean it.
const FRESH_MINUTES: i64 = 15;

/// Which of the place tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceToolKind {
    Save,
    List,
    Delete,
    Routine,
}

impl PlaceToolKind {
    pub(crate) const ALL: [Self; 4] = [Self::Save, Self::List, Self::Delete, Self::Routine];

    fn name(self) -> &'static str {
        match self {
            Self::Save => "place_save",
            Self::List => "place_list",
            Self::Delete => "place_delete",
            Self::Routine => "place_routine",
        }
    }
}

pub(crate) struct PlaceTool {
    pub user: Arc<User>,
    pub kind: PlaceToolKind,
    /// The chat the run is in, where a routine made here replies.
    pub chat: Option<Uuid>,
}

fn place_json(place: &SavedPlace, here: Option<&Location>) -> Value {
    json!({
        "id": place.id,
        "name": place.name,
        "address": place.address,
        "radius_m": place.radius,
        "latitude": place.latitude,
        "longitude": place.longitude,
        "here_now": place.inside,
        "last_here_at": place.last_here_at,
        "routines": place.routines,
        "distance_m": here.map(|fix| places::distance_to(place, fix).round()),
    })
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[async_trait]
impl Tool for PlaceTool {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        match self.kind {
            PlaceToolKind::Save => {
                "Save a place by name, such as Home or Gym, so routines can start when the user arrives there or leaves. Without coordinates it saves where the phone is now, as when the user says \"I'm at the gym, remember this place\". Saving a name that exists moves that place. If another saved place is close by, nothing is saved and you are told which: ask the user whether to move that one or save a new place."
            }
            PlaceToolKind::List => {
                "List the user's saved places with their address, radius, whether the phone is there now, when it was last there, and how far it is now."
            }
            PlaceToolKind::Delete => {
                "Forget a saved place by name. The routines that start there are deleted with it."
            }
            PlaceToolKind::Routine => {
                "Create a routine that starts when the user arrives at or leaves a saved place, such as \"when I get to the gym, remind me to log my weights\". Write the task as a note to yourself, as for other routines. A routine set to run once is deleted after it runs, so check the cron tools before telling the user one is still set. List, pause or delete it with the cron tools."
            }
        }
    }

    fn parameters(&self) -> Value {
        match self.kind {
            PlaceToolKind::Save => json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Short name, e.g. 'Gym' or 'Home'" },
                    "radius_m": { "type": "integer", "minimum": places::MIN_RADIUS, "maximum": places::MAX_RADIUS, "description": "How near counts as being there, in metres. Default 100; 250 or more for a park or campus" },
                    "address": { "type": "string", "description": "Street address or area. Default: the phone's place name" },
                    "latitude": { "type": "number", "description": "Only to save somewhere other than where the phone is" },
                    "longitude": { "type": "number" },
                    "new_place": { "type": "boolean", "description": "Save even with another place close by" }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            PlaceToolKind::List => {
                json!({"type": "object", "properties": {}, "additionalProperties": false})
            }
            PlaceToolKind::Delete => json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The saved place's name" }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            PlaceToolKind::Routine => json!({
                "type": "object",
                "properties": {
                    "place": { "type": "string", "description": "The saved place's name" },
                    "when": { "type": "string", "enum": ["arrive", "leave"] },
                    "label": { "type": "string", "description": "A few words naming the routine, e.g. 'Log my weights'" },
                    "task": { "type": "string", "description": "What to do when it starts, as a note to yourself" },
                    "repeat": { "type": "string", "enum": ["once", "daily", "always"], "description": "once: only the next time, then it is deleted, as for \"remind me when I leave\". daily: at most once a day, skipping a second visit the same day, as for a habit. always: every time, as for \"every time I get home\"" }
                },
                "required": ["place", "when", "label", "task", "repeat"],
                "additionalProperties": false
            }),
        }
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let id = ctx.tool_call_id;
        let output = match self.kind {
            PlaceToolKind::Save => match self.save(&ctx.args).await? {
                Ok(output) => output,
                Err(message) => return Ok(ToolResult::failure(id, message)),
            },
            PlaceToolKind::List => {
                let here = self.user.get_location().await?;
                let places = self.user.list_places().await?;
                let places: Vec<_> = places
                    .iter()
                    .map(|place| place_json(place, here.as_ref()))
                    .collect();
                json!({ "places": places })
            }
            PlaceToolKind::Delete => {
                let Some(name) = text(&ctx.args, "name") else {
                    return Ok(ToolResult::failure(id, "name is required"));
                };
                let Some(place) = self.user.place_named(name).await? else {
                    return Ok(ToolResult::failure(id, format!("no place named '{name}'")));
                };
                self.user.delete_place(place.id).await?;
                self.user.changed("");
                json!({"deleted": place.name, "routines_deleted": place.routines})
            }
            PlaceToolKind::Routine => match self.routine(&ctx.args).await? {
                Ok(output) => output,
                Err(message) => return Ok(ToolResult::failure(id, message)),
            },
        };
        Ok(ToolResult::success(id, output.to_string()))
    }
}

impl PlaceTool {
    /// Saves a place; the inner error is a message for the model.
    async fn save(&self, args: &Value) -> anyhow::Result<Result<Value, String>> {
        let Some(name) = text(args, "name") else {
            return Ok(Err("name is required".into()));
        };
        let here = self.user.get_location().await?;
        let given = match (
            args.get("latitude").and_then(Value::as_f64),
            args.get("longitude").and_then(Value::as_f64),
        ) {
            (Some(latitude), Some(longitude)) => Some(Point {
                latitude,
                longitude,
            }),
            (None, None) => None,
            _ => return Ok(Err("give both latitude and longitude, or neither".into())),
        };
        let point = match (given, &here) {
            (Some(point), _) => point,
            (None, None) => {
                return Ok(Err(
                    "the phone has not shared its location, so location may be \
                    off for Beep. Ask the user to turn it on and say it again."
                        .into(),
                ));
            }
            (None, Some(fix)) => {
                let age = Utc::now() - fix.updated_at;
                if age > TimeDelta::minutes(FRESH_MINUTES) {
                    return Ok(Err(format!(
                        "the phone's last location is {} minutes old, so it may not be \
                         where the user is. Ask them to open the app there and say it again.",
                        age.num_minutes()
                    )));
                }
                Point::from(fix)
            }
        };
        let radius = match args.get("radius_m").and_then(Value::as_u64) {
            Some(radius) => u32::try_from(radius).unwrap_or(u32::MAX),
            None => places::DEFAULT_RADIUS,
        };
        let new_place = args
            .get("new_place")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let saved = self.user.list_places().await?;
        let moved = saved.iter().any(|p| p.name.eq_ignore_ascii_case(name));
        let nearby = saved
            .iter()
            .filter(|p| !p.name.eq_ignore_ascii_case(name))
            .map(|p| (p, Point::from(p).distance_m(point)))
            .filter(|(p, distance)| *distance <= f64::from(p.radius.max(radius)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let (Some((other, distance)), false) = (nearby, new_place) {
            return Ok(Ok(json!({
                "saved": false,
                "nearby": other.name,
                "distance_m": distance.round(),
                "next": format!(
                    "Ask the user whether to move {0} here (place_save with name \"{0}\"), \
                     save a new place anyway (new_place true), or leave it.",
                    other.name
                ),
            })));
        }

        let address = text(args, "address").or(match given {
            None => here.as_ref().and_then(|fix| fix.place.as_deref()),
            Some(_) => None,
        });
        let inside = here
            .as_ref()
            .is_some_and(|fix| Point::from(fix).distance_m(point) <= f64::from(radius));
        let place = NewPlace {
            name,
            latitude: point.latitude,
            longitude: point.longitude,
            radius,
            address,
            inside,
        };
        let place = match self.user.save_place(&place).await {
            Ok(place) => place,
            Err(error) => return Ok(Err(error.to_string())),
        };
        self.user.changed("");
        Ok(Ok(json!({
            "saved": true,
            "moved": moved,
            "place": place_json(&place, here.as_ref()),
        })))
    }

    /// Creates a place routine; the inner error is a message for the model.
    async fn routine(&self, args: &Value) -> anyhow::Result<Result<Value, String>> {
        let (Some(name), Some(when), Some(label), Some(task)) = (
            text(args, "place"),
            text(args, "when"),
            text(args, "label"),
            text(args, "task"),
        ) else {
            return Ok(Err("place, when, label and task are required".into()));
        };
        let Some(event) = PlaceEvent::parse(when) else {
            return Ok(Err("when is arrive or leave".into()));
        };
        let Some(place) = self.user.place_named(name).await? else {
            return Ok(Err(format!(
                "no place named '{name}'. Save it first with place_save, or check place_list."
            )));
        };
        let Some(repeat) = text(args, "repeat").and_then(PlaceRepeat::parse) else {
            return Ok(Err("repeat is once, daily or always".into()));
        };
        let routine = NewPlaceRoutine {
            label,
            task,
            trigger: PlaceTrigger {
                event,
                place: place.id,
                repeat,
            },
            chat: self.chat,
        };
        let id = match self.user.create_place_routine(&routine).await {
            Ok(id) => id,
            Err(error) => return Ok(Err(error.to_string())),
        };
        Ok(Ok(json!({
            "created": true,
            "id": id,
            "label": label,
            "place": place.name,
            "when": event.as_str(),
            "repeat": repeat.as_str(),
        })))
    }
}

#[cfg(test)]
mod tests {
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
            PlaceTrigger::parse(&crons[0].schedule),
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
}
