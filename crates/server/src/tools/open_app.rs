use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use reqwest::Url;
use serde_json::{Value, json};

const APPS: &str =
    "spotify, youtube, whatsapp, maps, phone, clock, browser, chrome, gmail, calendar, uber";

/// Uber's universal link. It opens the Uber app when installed and the
/// mobile site otherwise.
const UBER_LINK: &str = "https://m.uber.com/ul/";

pub(crate) struct OpenAppTool;

#[async_trait]
impl Tool for OpenAppTool {
    fn name(&self) -> &str {
        "open_app"
    }
    fn description(&self) -> &str {
        "Ask the user's Android phone to open an app. The phone gets the request but never reports back whether the app opened, so say you are opening it, never that it opened. To set an alarm use set_alarm; clock only opens the alarm list. For a ride, use uber with destination: it opens Uber with the trip filled in from the user's location, and the user still picks the ride and confirms it, so never say a ride is booked."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "enum": APPS.split(", ").collect::<Vec<_>>()
                },
                "action": {
                    "type": "string",
                    "description": "What to do, e.g. 'play song X', 'call X'"
                },
                "uri": {
                    "type": "string",
                    "description": "Optional deep link, only one you know is valid for the app. Never guess a path. For Maps: geo:0,0?q=Dubai+Mall for a place, https://maps.google.com/location-sharing for people sharing their location. Not for uber; use destination."
                },
                "destination": {
                    "type": "string",
                    "description": "Uber only: where the ride goes, as a full address or place name with its city, e.g. 'Dubai Mall, Dubai'."
                },
                "latitude": {
                    "type": "number",
                    "description": "Uber only: the destination's latitude, when you know it exactly. Uber needs coordinates to fill in the drop-off reliably."
                },
                "longitude": {
                    "type": "number",
                    "description": "Uber only: the destination's longitude, given with latitude."
                }
            },
            "required": ["app", "action"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let app = ctx.args.get("app").and_then(Value::as_str).unwrap_or("");
        let action = ctx.args.get("action").and_then(Value::as_str).unwrap_or("");
        let uri_override = ctx.args.get("uri").and_then(Value::as_str);

        let Some(target) = target(app) else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Unknown app {app:?}. Supported: {APPS}."),
            ));
        };

        let uri = if app.eq_ignore_ascii_case("uber") {
            uber_link(&Dropoff {
                address: ctx.args.get("destination").and_then(Value::as_str),
                latitude: ctx.args.get("latitude").and_then(Value::as_f64),
                longitude: ctx.args.get("longitude").and_then(Value::as_f64),
            })
        } else {
            uri_override.unwrap_or(target.uri).to_owned()
        };

        let output = json!({
            "action": "open_app",
            "app": app,
            "intent_action": target.intent_action,
            "uri": uri,
            "fallback_url": target.fallback_url,
            "package": target.package,
            "requested_action": action,
            "status": "sent to the phone, not confirmed",
        });
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

/// Where an Uber ride goes.
struct Dropoff<'a> {
    address: Option<&'a str>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

/// An Uber link that sets pickup to the phone's location and, when given,
/// the drop-off.
fn uber_link(dropoff: &Dropoff) -> String {
    let mut params = vec![
        ("action", "setPickup".to_owned()),
        ("pickup", "my_location".to_owned()),
    ];
    let address = dropoff.address.map(str::trim).filter(|a| !a.is_empty());
    if let Some(address) = address {
        params.push(("dropoff[nickname]", address.to_owned()));
        params.push(("dropoff[formatted_address]", address.to_owned()));
    }
    if let (Some(latitude), Some(longitude)) = (dropoff.latitude, dropoff.longitude) {
        params.push(("dropoff[latitude]", latitude.to_string()));
        params.push(("dropoff[longitude]", longitude.to_string()));
    }
    match Url::parse_with_params(UBER_LINK, &params) {
        Ok(url) => url.into(),
        Err(_) => UBER_LINK.to_owned(),
    }
}

/// How the phone opens an app.
struct Target {
    intent_action: &'static str,
    uri: &'static str,
    fallback_url: &'static str,
    package: &'static str,
}

fn target(app: &str) -> Option<Target> {
    Some(match app.to_lowercase().as_str() {
        "spotify" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "spotify:",
            fallback_url: "https://open.spotify.com/",
            package: "com.spotify.music",
        },
        "youtube" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "vnd.youtube:",
            fallback_url: "https://www.youtube.com/",
            package: "com.google.android.youtube",
        },
        "whatsapp" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "whatsapp://send",
            fallback_url: "https://wa.me/",
            package: "com.whatsapp",
        },
        "maps" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "geo:0,0",
            fallback_url: "https://maps.google.com/",
            package: "com.google.android.apps.maps",
        },
        "phone" => Target {
            intent_action: "android.intent.action.DIAL",
            uri: "tel:",
            fallback_url: "tel:",
            package: "com.android.dialer",
        },
        "clock" => Target {
            intent_action: "android.intent.action.SHOW_ALARMS",
            uri: "",
            fallback_url: "",
            package: "com.google.android.deskclock",
        },
        "browser" | "chrome" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "https://www.google.com/",
            fallback_url: "https://www.google.com/",
            package: "com.android.chrome",
        },
        "gmail" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "mailto:",
            fallback_url: "https://mail.google.com/",
            package: "com.google.android.gm",
        },
        "calendar" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: "content://com.android.calendar/time/",
            fallback_url: "https://calendar.google.com/",
            package: "com.google.android.calendar",
        },
        "uber" => Target {
            intent_action: "android.intent.action.VIEW",
            uri: UBER_LINK,
            fallback_url: UBER_LINK,
            package: "com.ubercab",
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_app_opens() {
        for app in APPS.split(", ") {
            assert!(target(app).is_some(), "{app} has no target");
        }
    }

    #[test]
    fn uber_link_fills_in_the_dropoff() {
        let link = uber_link(&Dropoff {
            address: Some("Dubai Mall, Dubai"),
            latitude: Some(25.1972),
            longitude: Some(55.2796),
        });
        assert_eq!(
            link,
            "https://m.uber.com/ul/?action=setPickup&pickup=my_location\
             &dropoff%5Bnickname%5D=Dubai+Mall%2C+Dubai\
             &dropoff%5Bformatted_address%5D=Dubai+Mall%2C+Dubai\
             &dropoff%5Blatitude%5D=25.1972&dropoff%5Blongitude%5D=55.2796"
        );
        let bare = uber_link(&Dropoff {
            address: None,
            latitude: Some(25.1972),
            longitude: None,
        });
        assert_eq!(
            bare,
            "https://m.uber.com/ul/?action=setPickup&pickup=my_location"
        );
    }
}
