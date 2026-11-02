//! `desktop`: see and use the screen of a machine that offers `desktop/1`,
//! a Mac the user lets Beep use.

use super::{Machines, Need, in_parameter};
use crate::protocol::node::desktop::{KeyPress, MouseButton, Permission, Reply, Request, Target};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::types::{
    ContentPart,
    tool::{Tool, ToolContext, ToolResult},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::Write;

/// The longer side of a screenshot the model gets, in pixels.
const SCREENSHOT_SIDE: u32 = 1280;
/// The most elements a tree lists.
const TREE_ELEMENTS: u32 = 300;
const DEFAULT_SCROLL_LINES: i32 = 5;
const MAX_SCROLL_LINES: i32 = 100;

const DESCRIPTION: &str = "See and use the screen of the user's Mac, when they turned on \
    \"Let Beep use this Mac\" there. `screenshot` shows the screen as an image; `tree` lists the \
    frontmost app's controls, each with an [index]; `click` presses `element` (an index from the \
    last tree) or the point `x`,`y` in the last screenshot's pixels, with `button: right` or \
    `double: true` if needed; `type` types `text` into what has focus; `key` presses `keys` in \
    turn, such as [\"cmd+l\", \"return\"]; `scroll` scrolls `direction` by `lines` (default 5), \
    over `element` or `x`,`y` if given; `open` opens `target`, an app name or URL; `windows` \
    lists open windows; `permissions` shows what macOS allows Beep. Prefer the tree for \
    clicking: indexes are exact. Look again after an action to check it worked. Only act as \
    the user asked. With several such Macs, name one with `in`.";

pub(super) struct DesktopTool {
    pub(super) machines: Machines,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Action {
    Screenshot,
    Tree,
    Click,
    Type,
    Key,
    Scroll,
    Open,
    Windows,
    Permissions,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Button {
    Left,
    Right,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Args {
    action: Action,
    #[serde(default)]
    element: Option<u32>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    button: Option<Button>,
    #[serde(default)]
    double: bool,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    keys: Vec<String>,
    #[serde(default)]
    direction: Option<Direction>,
    #[serde(default)]
    lines: Option<i32>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default, rename = "in")]
    machine: Option<String>,
}

impl Args {
    /// `element`, or the point `x`,`y`, if the call gave one.
    fn spot(&self) -> Result<Option<Target>, String> {
        match (self.element, self.x, self.y) {
            (Some(index), None, None) => Ok(Some(Target::Element { index })),
            (None, Some(x), Some(y)) => Ok(Some(Target::Point { x, y })),
            (None, None, None) => Ok(None),
            (Some(_), _, _) => Err("Give `element` or `x` and `y`, not both.".into()),
            _ => Err("A point needs both `x` and `y`.".into()),
        }
    }

    /// The requests this call sends, in order.
    fn requests(&self) -> Result<Vec<Request>, String> {
        let text = |field: &str, value: &Option<String>| {
            value
                .clone()
                .filter(|text| !text.is_empty())
                .ok_or_else(|| format!("{:?} needs `{field}`.", self.action))
        };
        let request = match self.action {
            Action::Screenshot => Request::Screenshot {
                max_side: SCREENSHOT_SIDE,
            },
            Action::Tree => Request::Tree {
                max_elements: TREE_ELEMENTS,
            },
            Action::Windows => Request::Windows {},
            Action::Permissions => Request::Permissions {},
            Action::Click => Request::Click {
                target: self
                    .spot()?
                    .ok_or("click needs `element` or `x` and `y`.")?,
                button: match self.button {
                    Some(Button::Right) => MouseButton::Right,
                    _ => MouseButton::Left,
                },
                count: if self.double { 2 } else { 1 },
            },
            Action::Type => Request::Type {
                text: text("text", &self.text)?,
            },
            Action::Key => {
                if self.keys.is_empty() {
                    return Err("key needs `keys`, such as [\"cmd+t\"].".into());
                }
                return self
                    .keys
                    .iter()
                    .map(|combo| {
                        combo
                            .parse::<KeyPress>()
                            .map(|press| Request::Key { press })
                    })
                    .collect();
            }
            Action::Scroll => {
                let lines = self
                    .lines
                    .unwrap_or(DEFAULT_SCROLL_LINES)
                    .clamp(1, MAX_SCROLL_LINES);
                let (dx, dy) = match self.direction.ok_or("scroll needs `direction`.")? {
                    Direction::Down => (0, lines),
                    Direction::Up => (0, -lines),
                    Direction::Right => (lines, 0),
                    Direction::Left => (-lines, 0),
                };
                Request::Scroll {
                    at: self.spot()?,
                    dx,
                    dy,
                }
            }
            Action::Open => Request::Open {
                target: text("target", &self.target)?,
            },
        };
        Ok(vec![request])
    }
}

#[async_trait]
impl Tool for DesktopTool {
    fn name(&self) -> &str {
        "desktop"
    }
    fn description(&self) -> &str {
        DESCRIPTION
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["screenshot", "tree", "click", "type", "key", "scroll", "open", "windows", "permissions"]
                },
                "element": { "type": "integer", "minimum": 0, "description": "An element's index in the last tree, for click and scroll" },
                "x": { "type": "number", "description": "A point in the last screenshot's pixels, with y" },
                "y": { "type": "number" },
                "button": { "type": "string", "enum": ["left", "right"] },
                "double": { "type": "boolean", "description": "Double-click" },
                "text": { "type": "string", "description": "What `type` types" },
                "keys": { "type": "array", "items": { "type": "string" }, "description": "Key combos `key` presses in turn, such as cmd+t or return" },
                "direction": { "type": "string", "enum": ["up", "down", "left", "right"] },
                "lines": { "type": "integer", "minimum": 1, "maximum": MAX_SCROLL_LINES },
                "target": { "type": "string", "description": "The app or URL `open` opens" },
                "in": in_parameter("Omit when only one machine offers desktop control.")
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }
    fn concurrent(&self) -> bool {
        false
    }
    fn humanize(&self, args: &Value) -> String {
        match args.get("action").and_then(Value::as_str) {
            Some("click") => "Clicking".into(),
            Some("type") => "Typing".into(),
            Some("key") => "Pressing keys".into(),
            Some("scroll") => "Scrolling".into(),
            Some("open") => "Opening an app".into(),
            _ => "Looking at the screen".into(),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let call_id = ctx.tool_call_id;
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(call_id, e.to_string())),
        };
        let requests = match args.requests() {
            Ok(requests) => requests,
            Err(e) => return Ok(ToolResult::failure(call_id, e)),
        };
        let picked = match self
            .machines
            .pick(args.machine.as_deref(), Need::Desktop)
            .await
        {
            Ok(picked) => picked,
            Err(e) => {
                return Ok(ToolResult::failure(
                    call_id,
                    format!(
                        "{e} On a Mac, the user turns on \"Let Beep use this Mac\" in the Beep \
                         app's Settings."
                    ),
                ));
            }
        };
        // Without the model's capabilities at hand, it is taken to see.
        let sees = match (ctx.llm, &ctx.provider) {
            (Some(llm), Some(provider)) => provider.capabilities(&llm.model).await.images,
            _ => true,
        };
        let mut lines = Vec::new();
        let mut images = Vec::new();
        for request in &requests {
            let reply = match picked.online.desktop(request).await {
                Ok(reply) => reply,
                Err(e) => return Ok(ToolResult::failure(call_id, format!("{e:#}"))),
            };
            match read_reply(&picked.name, reply, sees) {
                Ok(said) => {
                    lines.push(said.text);
                    images.extend(said.image);
                }
                Err(failed) => {
                    lines.push(failed);
                    return Ok(ToolResult::failure(call_id, lines.join("\n")));
                }
            }
        }
        Ok(ToolResult::success(call_id, lines.join("\n")).with_parts(images))
    }
}

/// What a desktop reply tells the model.
struct Said {
    text: String,
    /// The screenshot, for a model that sees images.
    image: Option<ContentPart>,
}

impl From<String> for Said {
    fn from(text: String) -> Self {
        Self { text, image: None }
    }
}

/// A desktop reply for the model; `Err` when the node could not do it.
/// `sees` is whether the model sees images.
fn read_reply(node: &str, reply: Reply, sees: bool) -> Result<Said, String> {
    let text = match reply {
        Reply::Screenshot(shot) => {
            let text = format!(
                "Screenshot of {node}, {}x{} pixels (the screen is {}x{} points). Points to \
                 click are in these pixels.",
                shot.width, shot.height, shot.screen_width, shot.screen_height
            );
            if !sees {
                return Ok(Said::from(format!(
                    "{text} It cannot be shown: this model does not see images. Read the \
                     screen with `desktop tree` instead."
                )));
            }
            return Ok(Said {
                text,
                image: Some(ContentPart::Image {
                    media_type: "image/jpeg".into(),
                    data: STANDARD.encode(&shot.jpeg),
                }),
            });
        }
        Reply::Tree(tree) => {
            let mut out = match &tree.window {
                Some(window) => format!("{}, window {window:?}\n", tree.app),
                None => format!("{}\n", tree.app),
            };
            out.push_str(&tree.text);
            if tree.truncated {
                let _ = write!(
                    out,
                    "\n[cut at {} elements; a screenshot shows the rest]",
                    tree.elements
                );
            }
            out
        }
        Reply::Done { detail } => detail,
        Reply::Windows { windows } if windows.is_empty() => "No windows on screen.".into(),
        Reply::Windows { windows } => windows
            .iter()
            .map(|w| {
                let title = if w.title.is_empty() {
                    String::new()
                } else {
                    format!(" {:?}", w.title)
                };
                format!(
                    "- {}{title} at {},{} size {}x{}",
                    w.app, w.x, w.y, w.width, w.height
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Reply::Permissions(granted) => {
            let state = |on: bool| if on { "allowed" } else { "not allowed" };
            format!(
                "Screen Recording: {}. Accessibility: {}.",
                state(granted.screen_recording),
                state(granted.accessibility)
            )
        }
        Reply::Failed { reason, missing } => {
            return Err(match missing {
                Some(permission) => format!("{reason} {}", grant_hint(permission)),
                None => reason,
            });
        }
        other => format!("{other:?}"),
    };
    Ok(Said::from(text))
}

fn grant_hint(permission: Permission) -> String {
    format!(
        "Ask the user to allow {} for Beep: Beep's Settings, This Mac, or System Settings, \
         Privacy and Security, {}.",
        permission.as_str(),
        permission.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::desktop;

    fn requests(value: Value) -> Result<Vec<Request>, String> {
        serde_json::from_value::<Args>(value)
            .map_err(|e| e.to_string())?
            .requests()
    }

    #[test]
    fn actions_become_desktop_requests() {
        assert_eq!(
            requests(json!({ "action": "screenshot" })).unwrap(),
            [Request::Screenshot {
                max_side: SCREENSHOT_SIDE
            }]
        );
        assert_eq!(
            requests(json!({ "action": "click", "element": 12 })).unwrap(),
            [Request::Click {
                target: Target::Element { index: 12 },
                button: MouseButton::Left,
                count: 1
            }]
        );
        assert_eq!(
            requests(json!({ "action": "click", "x": 640.5, "y": 400, "button": "right", "double": true }))
                .unwrap(),
            [Request::Click {
                target: Target::Point { x: 640.5, y: 400.0 },
                button: MouseButton::Right,
                count: 2
            }]
        );
        assert_eq!(
            requests(json!({ "action": "type", "text": "hello world" })).unwrap(),
            [Request::Type {
                text: "hello world".into()
            }]
        );
        let keys = requests(json!({ "action": "key", "keys": ["cmd+l", "return"] })).unwrap();
        assert_eq!(keys.len(), 2);
        assert!(matches!(&keys[1], Request::Key { press } if press.key == "return"));
        assert_eq!(
            requests(json!({ "action": "scroll", "direction": "up", "lines": 3, "element": 7 }))
                .unwrap(),
            [Request::Scroll {
                at: Some(Target::Element { index: 7 }),
                dx: 0,
                dy: -3
            }]
        );
        assert_eq!(
            requests(json!({ "action": "scroll", "direction": "down", "in": "mac" })).unwrap(),
            [Request::Scroll {
                at: None,
                dx: 0,
                dy: DEFAULT_SCROLL_LINES
            }]
        );
        assert_eq!(
            requests(json!({ "action": "open", "target": "Visual Studio Code" })).unwrap(),
            [Request::Open {
                target: "Visual Studio Code".into()
            }]
        );
        for bad in [
            json!({}),
            json!({ "action": "click" }),
            json!({ "action": "click", "x": 3 }),
            json!({ "action": "click", "element": 1, "x": 3, "y": 4 }),
            json!({ "action": "key", "keys": ["hyper+x"] }),
            json!({ "action": "key" }),
            json!({ "action": "scroll", "direction": "sideways" }),
            json!({ "action": "type" }),
            json!({ "action": "dance" }),
        ] {
            assert!(requests(bad.clone()).is_err(), "{bad}");
        }
    }

    fn screenshot() -> Reply {
        Reply::Screenshot(desktop::Screenshot {
            jpeg: vec![0xff, 0xd8],
            width: 1280,
            height: 800,
            screen_width: 1512,
            screen_height: 945,
        })
    }

    #[test]
    fn a_screenshot_reaches_the_model_as_an_image_part() {
        let said = read_reply("mac", screenshot(), true).unwrap();
        assert!(
            said.text.starts_with("Screenshot of mac, 1280x800 pixels"),
            "{}",
            said.text
        );
        let result =
            ToolResult::success("call", said.text).with_parts(said.image.into_iter().collect());
        let images: Vec<_> = result
            .content()
            .iter()
            .filter_map(|part| match part {
                ContentPart::Image { media_type, data } => {
                    Some((media_type.as_str(), data.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            images,
            [("image/jpeg", STANDARD.encode([0xff, 0xd8]).as_str())]
        );
    }

    #[test]
    fn a_model_without_vision_gets_a_note_instead() {
        let said = read_reply("mac", screenshot(), false).unwrap();
        assert!(said.image.is_none());
        assert!(said.text.contains("does not see images"), "{}", said.text);
    }

    #[test]
    fn a_missing_permission_says_how_to_grant_it() {
        let failed = read_reply(
            "mac",
            Reply::Failed {
                reason: "Screen Recording is off.".into(),
                missing: Some(Permission::ScreenRecording),
            },
            true,
        )
        .err()
        .expect("the reply failed");
        assert!(
            failed.contains("allow Screen Recording for Beep"),
            "{failed}"
        );
    }
}
