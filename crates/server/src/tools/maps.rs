use crate::account::user::User;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct OpenMapsTool {
    pub user: Arc<User>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum MapsMode {
    /// A search or a single place.
    Search,
    Directions,
}

/// The card the phone shows. It geocodes `query` or `destination` itself.
#[derive(Serialize)]
struct Place {
    name: Option<String>,
    query: Option<String>,
    mode: MapsMode,
    /// Directions only: the place name or `lat,lng` to navigate to.
    destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    longitude: Option<f64>,
    /// Directions only: where to start, when not from the user's location.
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    /// Directions only: driving, walking, transit or bicycling.
    #[serde(skip_serializing_if = "Option::is_none")]
    travel_mode: Option<String>,
    /// Search only: where to look, the given point or else the user's last location.
    #[serde(skip_serializing_if = "Option::is_none")]
    near: Option<Point>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
struct Point {
    latitude: f64,
    longitude: f64,
}

#[async_trait]
impl Tool for OpenMapsTool {
    fn name(&self) -> &str {
        "open_maps"
    }

    // Acts on the phone, which handles one request at a time.
    fn concurrent(&self) -> bool {
        false
    }
    fn description(&self) -> &str {
        "Show a place, a place search or directions as a map card in the chat. Use it whenever the user asks where something is. The card has Directions and Open in Maps buttons; Google Maps does not open by itself. For directions, query is the destination alone and origin is the starting point; never write \"X from Y\" in query. The card shows the destination, not the drawn route. Afterwards, say the place is on the map card below, never that Maps opened. To open the Google Maps app itself, use open_app with app \"maps\"."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "directions", "place"],
                    "description": "search: search for places; place: show a named place; directions: get navigation directions"
                },
                "query": {
                    "type": "string",
                    "description": "Search query or place name, e.g. 'italian restaurant', 'ramen near Hotel Gracery Shinjuku' or 'central market Lisbon'. Searches run around the user's location unless latitude/longitude are given."
                },
                "latitude": {
                    "type": "number",
                    "description": "Latitude for place or directions destination, or the center of a search"
                },
                "longitude": {
                    "type": "number",
                    "description": "Longitude for place or directions destination, or the center of a search"
                },
                "origin": {
                    "type": "string",
                    "description": "Directions only: the starting place, e.g. 'Changi Airport Terminal 3'. Leave out to start from the user's location."
                },
                "travel_mode": {
                    "type": "string",
                    "enum": ["driving", "walking", "transit", "bicycling"],
                    "description": "Travel mode for directions"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let action = ctx.args.get("action").and_then(Value::as_str).unwrap_or("");
        let query = ctx.args.get("query").and_then(Value::as_str).unwrap_or("");
        let latitude = ctx.args.get("latitude").and_then(Value::as_f64);
        let longitude = ctx.args.get("longitude").and_then(Value::as_f64);
        let travel_mode = ctx
            .args
            .get("travel_mode")
            .and_then(Value::as_str)
            .unwrap_or("driving");
        let origin = ctx
            .args
            .get("origin")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|o| !o.is_empty() && action == "directions");

        let encoded_query = query.replace(' ', "+");
        let near = match (action, latitude, longitude) {
            ("search", Some(latitude), Some(longitude)) => Some(Point {
                latitude,
                longitude,
            }),
            ("search", _, _) => self
                .user
                .get_location()
                .await
                .ok()
                .flatten()
                .map(|loc| Point {
                    latitude: loc.latitude,
                    longitude: loc.longitude,
                }),
            _ => None,
        };

        let uri = match action {
            "search" => {
                if query.is_empty() {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "query is required for search action",
                    ));
                }
                match near {
                    Some(p) => format!("geo:{},{}?q={encoded_query}", p.latitude, p.longitude),
                    None => format!("geo:0,0?q={encoded_query}"),
                }
            }
            "place" => {
                if let (Some(lat), Some(lng)) = (latitude, longitude) {
                    format!("geo:{lat},{lng}?q={encoded_query}")
                } else if !query.is_empty() {
                    format!("geo:0,0?q={encoded_query}")
                } else {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "query or latitude/longitude required for place action",
                    ));
                }
            }
            "directions" => {
                let dest = if let (Some(lat), Some(lng)) = (latitude, longitude) {
                    format!("{lat},{lng}")
                } else if !query.is_empty() {
                    encoded_query.clone()
                } else {
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        "query or latitude/longitude required for directions",
                    ));
                };
                if let Some(origin) = origin {
                    // google.navigation always starts from the phone, so use the Maps URL.
                    format!(
                        "https://www.google.com/maps/dir/?api=1&origin={}&destination={dest}&travelmode={travel_mode}",
                        origin.replace(' ', "+")
                    )
                } else {
                    let mode_flag = match travel_mode {
                        "walking" => "w",
                        "transit" => "r",
                        "bicycling" => "b",
                        _ => "d",
                    };
                    format!("google.navigation:q={dest}&mode={mode_flag}")
                }
            }
            _ => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "action must be 'search', 'place', or 'directions'",
                ));
            }
        };

        let directions = action == "directions";
        let place = Place {
            name: (!query.is_empty()).then(|| query.to_owned()),
            query: (!query.is_empty()).then(|| query.to_owned()),
            mode: if directions {
                MapsMode::Directions
            } else {
                MapsMode::Search
            },
            destination: directions.then(|| match (latitude, longitude) {
                (Some(lat), Some(lng)) => format!("{lat},{lng}"),
                _ => query.to_owned(),
            }),
            latitude: latitude.filter(|_| action != "search"),
            longitude: longitude.filter(|_| action != "search"),
            origin: origin.map(str::to_owned),
            travel_mode: directions.then(|| travel_mode.to_owned()),
            near,
        };
        let output = json!({
            "action": "open_maps",
            "uri": uri,
            "place": place,
            "message": "Shown in the chat as a map card with Directions and Open in Maps buttons. Google Maps has not opened.",
        });

        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}
