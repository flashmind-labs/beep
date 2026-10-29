//! `node`: one tool, driven like a CLI, for the machines running
//! `beep-node`. The first word names a node (or `list`, `link`), the second
//! one of the protocols it offers, so a node that gains a tool or a coding
//! agent needs no new tool here.

use crate::{
    app::App,
    nodes::{self, Online, Ran},
    protocol::node::{
        agent,
        desktop::{self, KeyPress, MouseButton, Permission, Reply, Request, Target},
        shell,
    },
    store::NodeRow,
    tool_images::ToolImages,
    user::User,
};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use flashmind::types::{
    ContentPart,
    tool::{Tool, ToolContext, ToolResult},
};
use serde_json::{Value, json};
use std::{fmt::Write, sync::Arc, time::Duration};
use uuid::Uuid;

mod agents;
mod files;

pub(crate) const NAME: &str = "node";

const USAGE: &str = "Usage: `list`, `link LINK`, or `<node> help`, `<node> info`, \
    `<node> sessions`, `<node> shell [--timeout SECS] \"COMMAND\"`, `<node> desktop ...` and \
    `<node> <agent> ...` for a coding agent it offers, where <node> is a name from `list`.";

const DESKTOP_USAGE: &str = "desktop screenshot | tree | click <index|x,y> [--right] [--double] | \
    type \"TEXT\" | key <combo>... | scroll <up|down|left|right> [LINES] [--at <index|x,y>] | \
    open <app|url> | windows | permissions";

/// The longer side of a screenshot the model gets, in pixels.
const SCREENSHOT_SIDE: u32 = 1280;
/// The most elements a tree lists.
const TREE_ELEMENTS: u32 = 300;
const DEFAULT_SCROLL_LINES: i32 = 5;

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 30 * 60;

pub(crate) struct NodeTool {
    pub(crate) app: Arc<App>,
    pub(crate) user: Arc<User>,
    /// The chat the turn runs in, which followed sessions report to.
    pub(crate) chat: Uuid,
}

#[async_trait]
impl Tool for NodeTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Use the user's machines that run beep-node, such as a dev server or their Mac. Write `command` like a \
         shell command line. `list` shows each machine, whether it is online, and what it offers. \
         `<node> info` shows what the machine has: OS, tools and versions, which CLIs are signed in, Android \
         emulators. `<node> help` shows the commands it takes. Check `info` before choosing where to run \
         something. `<node> shell \"COMMAND\"` runs a command in the machine's shell and returns its output \
         and exit code; the shell stays open, so `cd` and `export` carry over to the next command. It \
         starts in the home folder. Stdin is empty, so nothing interactive. Add `--timeout SECS` (default 60, \
         at most 1800) for slow commands; a command that times out is stopped along with its shell. \
         A machine that offers desktop/1 (the user's Mac) also takes `<node> desktop ...` to see and use \
         its screen: `desktop screenshot` shows the screen as an image; `desktop tree` lists the frontmost \
         app's controls, each with an [index]; `desktop click 12` presses element 12 of the last tree, \
         `desktop click 640,400` clicks a point in the last screenshot's pixels (add --right or --double); \
         `desktop type \"hello\"` types into what has focus; `desktop key cmd+t` presses keys (several combos \
         press in turn, such as `key cmd+l return`); `desktop scroll down 5 --at 12`; `desktop open Safari` \
         or `desktop open https://example.com`; `desktop windows` lists open windows. Prefer the tree for \
         clicking: indexes are exact. Look again after an action to check it worked. Only act as the user \
         asked.\n\
         A machine that offers files/1 shares some folders: `<node> files roots` lists them, \
         `files ls [PATH]`, `files stat PATH`, `files read PATH [--offset BYTES] [--limit BYTES]` \
         (64 KB unless --limit, at most 1 MB) and `files write PATH [--append] \"TEXT\"` (replaces \
         the file unless --append; its folder must exist). A relative path starts in the first \
         shared folder; nothing outside them is reachable.\n\
         Coding agents: a machine that offers `claude/1`, `codex/1` or `gemini/1` runs that agent \
         (Claude Code, Codex, Gemini CLI) in sessions that keep running between calls. \
         `<node> claude start [--cwd DIR] [--wait SECS] \"PROMPT\"` starts one; it waits up to --wait \
         (default 90) for the turn to end or a question, then returns the session id and its state: status, \
         what it is doing, its plan, files changed, its last message. `<node> claude status [ID] [--wait SECS]` \
         checks one (or lists them), `prompt ID \"TEXT\"` steers it (waiting behind a running turn), \
         `cancel ID` stops the turn, `close ID` ends the session, `transcript ID [--last N]` reads what \
         happened. Same for `codex` and `gemini`. `<node> sessions` lists every session. \
         A session started, prompted or taken over from this chat reports back here on its own when \
         a turn ends or it asks something after the call stopped waiting, so do not poll `status` \
         for it; its questions also reach the user as a card they can answer. \
         Claude Code sessions the user started themselves (in a terminal or IDE) on the machine: \
         `<node> claude sessions --all` lists them too, with whether they run now, \
         `transcript ID` reads one, `follow ID` posts in this chat when it finishes a turn or \
         waits for the user (`unfollow ID` stops), and `takeover ID [\"PROMPT\"]` resumes one \
         that nothing runs any more as a session here. One open in the user's terminal can only \
         be followed: Beep cannot type into it. When an agent asks \
         permission, answer it yourself with `answer ID OPTION` (option number, id or name) if the action \
         is clearly within what the user asked; ask the user in the chat when it is risky, destructive or \
         unclear. Text that is not an option declines and is sent to the agent as its next prompt.\n\
         `link LINK` links a new machine to the user's account from the `beep-link:` link that \
         `beep-node link` printed there; then that machine needs `beep-node run`."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Such as `list`, `build-box info` or `build-box shell \"df -h\"`" }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, args: &Value) -> String {
        let command = args.get("command").and_then(Value::as_str).unwrap_or("");
        let words = shlex::split(command).unwrap_or_default();
        match &words[..] {
            [] => "Checking your machines".into(),
            [list] if list == "list" => "Checking your machines".into(),
            [link, ..] if link == "link" => "Linking a machine".into(),
            [node, agent, command, ..]
                if !matches!(
                    agent.as_str(),
                    "shell" | "info" | "help" | "sessions" | "desktop" | "files"
                ) =>
            {
                match command.as_str() {
                    "start" => format!("Starting {agent} on {node}"),
                    "follow" => format!("Following {agent} on {node}"),
                    "unfollow" => format!("Unfollowing {agent} on {node}"),
                    "takeover" => format!("Taking over {agent} on {node}"),
                    "prompt" => format!("Messaging {agent} on {node}"),
                    "answer" => format!("Answering {agent} on {node}"),
                    "cancel" | "close" => format!("Stopping {agent} on {node}"),
                    _ => format!("Checking {agent} on {node}"),
                }
            }
            [node, tool, rest @ ..] if tool == "shell" => match shell_args(rest) {
                Ok(args) => format!("Running `{}` on {node}", args.command),
                Err(_) => format!("Running a command on {node}"),
            },
            [node, tool, action, ..] if tool == "desktop" => match action.as_str() {
                "screenshot" | "tree" | "windows" => format!("Looking at the screen on {node}"),
                "click" => format!("Clicking on {node}"),
                "type" => format!("Typing on {node}"),
                "key" => format!("Pressing keys on {node}"),
                "scroll" => format!("Scrolling on {node}"),
                "open" => format!("Opening an app on {node}"),
                _ => format!("Using {node}"),
            },
            [node, tool, action, ..] if tool == "files" => match action.as_str() {
                "write" => format!("Writing a file on {node}"),
                "read" => format!("Reading a file on {node}"),
                _ => format!("Looking at files on {node}"),
            },
            [node, ..] => format!("Checking {node}"),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let command = ctx
            .args
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("");
        let Some(words) = shlex::split(command) else {
            return Ok(ToolResult::failure(ctx.tool_call_id, "Unbalanced quotes."));
        };
        let nodes = self.user.nodes().await?;
        let output = match words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            ["list"] => list(&nodes, &self.app.nodes),
            ["link", ref rest @ ..] => return Ok(self.link(ctx.tool_call_id, rest).await),
            [name, "sessions"] => match find(&nodes, name) {
                Some(node) => {
                    let Some(online) = self.online(node) else {
                        return Ok(ToolResult::failure(
                            ctx.tool_call_id,
                            format!("{} is offline.", node.name),
                        ));
                    };
                    return Ok(finish(
                        ctx.tool_call_id,
                        agents::sessions(&online, &node.name).await,
                    ));
                }
                None => return Ok(unknown(ctx.tool_call_id, name, &nodes)),
            },
            [name, "info"] => match find(&nodes, name) {
                Some(node) => info(node),
                None => return Ok(unknown(ctx.tool_call_id, name, &nodes)),
            },
            [name, "help"] => match find(&nodes, name) {
                Some(node) => help(node, self.online(node).as_ref()),
                None => return Ok(unknown(ctx.tool_call_id, name, &nodes)),
            },
            [name, "shell", ref rest @ ..] => match find(&nodes, name) {
                Some(node) => return Ok(self.shell(ctx.tool_call_id, node, rest).await),
                None => return Ok(unknown(ctx.tool_call_id, name, &nodes)),
            },
            [name, "desktop", ref rest @ ..] => match find(&nodes, name) {
                Some(node) => return Ok(self.desktop(ctx.tool_call_id, node, rest).await),
                None => return Ok(unknown(ctx.tool_call_id, name, &nodes)),
            },
            [name, "files", ref rest @ ..] => match find(&nodes, name) {
                Some(node) => {
                    let Some(online) = self.online(node) else {
                        return Ok(ToolResult::failure(
                            ctx.tool_call_id,
                            format!("{} is offline.", node.name),
                        ));
                    };
                    let shown = command_name(&node.name);
                    let result = files::run(&online, &shown, rest).await;
                    return Ok(finish(ctx.tool_call_id, result));
                }
                None => return Ok(unknown(ctx.tool_call_id, name, &nodes)),
            },
            [name, tool, ref rest @ ..] => match find(&nodes, name) {
                Some(node) => {
                    if let Some(online) = self.online(node)
                        && online.offers(&agent::protocol(tool))
                    {
                        let shown = command_name(&node.name);
                        let asker = agents::Asker {
                            app: &self.app,
                            user: &self.user,
                            chat: self.chat,
                            endpoint: &node.endpoint,
                        };
                        let result = agents::run(&online, &shown, tool, rest, &asker).await;
                        return Ok(finish(ctx.tool_call_id, result));
                    }
                    return Ok(ToolResult::failure(
                        ctx.tool_call_id,
                        format!(
                            "`{tool}` is not available on {} yet.\n{}",
                            node.name,
                            help(node, self.online(node).as_ref())
                        ),
                    ));
                }
                None => return Ok(unknown(ctx.tool_call_id, name, &nodes)),
            },
            _ => return Ok(ToolResult::failure(ctx.tool_call_id, USAGE)),
        };
        Ok(ToolResult::success(ctx.tool_call_id, output))
    }
}

impl NodeTool {
    /// The node's connection, only while it is signed in as this user.
    fn online(&self, node: &NodeRow) -> Option<Online> {
        self.app
            .nodes
            .online(&node.endpoint)
            .filter(|online| online.user == self.user.id())
    }

    /// Links the machine whose `beep-node link` printed `rest` to the user.
    async fn link(&self, call_id: &str, rest: &[&str]) -> ToolResult {
        match nodes::link_machine(&self.app.nodes, &self.user, &rest.join("")).await {
            Ok(name) => ToolResult::success(
                call_id,
                format!(
                    "Linked {name}. Next, on that machine run `beep-node run`, or \
                     `beep-node service install` to keep it running as a service (Linux). \
                     It shows up in `list` once it connects."
                ),
            ),
            Err(e) => ToolResult::failure(call_id, format!("{e:#}")),
        }
    }

    async fn shell(&self, call_id: &str, node: &NodeRow, rest: &[&str]) -> ToolResult {
        let args = match shell_args(rest) {
            Ok(args) => args,
            Err(e) => return ToolResult::failure(call_id, e),
        };
        let Some(online) = self.online(node) else {
            return ToolResult::failure(call_id, format!("{} is offline.", node.name));
        };
        if !online.offers(shell::PROTOCOL) {
            return ToolResult::failure(call_id, format!("{} does not offer a shell.", node.name));
        }
        match online.run_shell(&args.command, args.timeout).await {
            Ok(ran) => ToolResult::success(call_id, report(&ran, args.timeout)),
            Err(e) => ToolResult::failure(call_id, format!("The command did not run: {e:#}")),
        }
    }
}

impl NodeTool {
    async fn desktop(&self, call_id: &str, node: &NodeRow, rest: &[&str]) -> ToolResult {
        let requests = match desktop_requests(rest) {
            Ok(requests) => requests,
            Err(e) => return ToolResult::failure(call_id, e),
        };
        let Some(online) = self.online(node) else {
            return ToolResult::failure(call_id, format!("{} is offline.", node.name));
        };
        if !online.offers(desktop::PROTOCOL) {
            return ToolResult::failure(
                call_id,
                format!(
                    "{} does not offer desktop control. On a Mac, the user turns on \
                     \"Let Beep use this Mac\" in the Beep app's Settings.",
                    node.name
                ),
            );
        }
        let mut lines = Vec::new();
        for request in &requests {
            let reply = match online.desktop(request).await {
                Ok(reply) => reply,
                Err(e) => return ToolResult::failure(call_id, format!("{e:#}")),
            };
            match read_reply(&node.name, reply, &self.app.tool_images) {
                Ok(line) => lines.push(line),
                Err(failed) => {
                    lines.push(failed);
                    return ToolResult::failure(call_id, lines.join("\n"));
                }
            }
        }
        ToolResult::success(call_id, lines.join("\n"))
    }
}

/// The requests a `desktop ...` command sends, in order.
fn desktop_requests(words: &[&str]) -> Result<Vec<Request>, String> {
    let usage = || format!("Usage: <node> {DESKTOP_USAGE}");
    let Some((action, rest)) = words.split_first() else {
        return Err(usage());
    };
    let request = match (*action, rest) {
        ("screenshot", []) => Request::Screenshot {
            max_side: SCREENSHOT_SIDE,
        },
        ("tree", []) => Request::Tree {
            max_elements: TREE_ELEMENTS,
        },
        ("windows", []) => Request::Windows {},
        ("permissions", []) => Request::Permissions {},
        ("click", [target, flags @ ..]) => {
            let mut button = MouseButton::Left;
            let mut count = 1;
            for flag in flags {
                match *flag {
                    "--right" => button = MouseButton::Right,
                    "--double" => count = 2,
                    other => return Err(format!("click does not take {other:?}. {}", usage())),
                }
            }
            Request::Click {
                target: target_arg(target)?,
                button,
                count,
            }
        }
        ("type", [_, ..]) => Request::Type {
            text: rest.join(" "),
        },
        ("key", [_, ..]) => {
            return rest
                .iter()
                .map(|combo| {
                    combo
                        .parse::<KeyPress>()
                        .map(|press| Request::Key { press })
                })
                .collect();
        }
        ("scroll", [direction, more @ ..]) => scroll_request(direction, more)?,
        ("open", [_, ..]) => Request::Open {
            target: rest.join(" "),
        },
        _ => return Err(usage()),
    };
    Ok(vec![request])
}

/// `12` is an element of the last tree, `640,400` a point.
fn target_arg(word: &str) -> Result<Target, String> {
    if let Ok(index) = word.parse::<u32>() {
        return Ok(Target::Element { index });
    }
    let point = word.split_once(',').and_then(|(x, y)| {
        Some(Target::Point {
            x: x.trim().parse().ok()?,
            y: y.trim().parse().ok()?,
        })
    });
    point.ok_or_else(|| format!("{word:?} is neither an element index nor x,y."))
}

/// `down [LINES] [--at TARGET]`.
fn scroll_request(direction: &str, words: &[&str]) -> Result<Request, String> {
    let mut lines = DEFAULT_SCROLL_LINES;
    let mut at = None;
    let mut words = words.iter();
    while let Some(word) = words.next() {
        match *word {
            "--at" => {
                let target = words.next().ok_or("--at needs an index or x,y.")?;
                at = Some(target_arg(target)?);
            }
            number => {
                lines = number
                    .parse::<i32>()
                    .map_err(|_| format!("scroll takes a number of lines, not {number:?}."))?
                    .clamp(1, 100);
            }
        }
    }
    let (dx, dy) = match direction {
        "down" => (0, lines),
        "up" => (0, -lines),
        "right" => (lines, 0),
        "left" => (-lines, 0),
        other => return Err(format!("Scroll up, down, left or right, not {other:?}.")),
    };
    Ok(Request::Scroll { at, dx, dy })
}

/// A desktop reply for the model; `Err` when the node could not do it.
fn read_reply(node: &str, reply: Reply, images: &ToolImages) -> Result<String, String> {
    Ok(match reply {
        Reply::Screenshot(shot) => {
            let mark = images.add(ContentPart::Image {
                media_type: "image/jpeg".into(),
                data: STANDARD.encode(&shot.jpeg),
            });
            format!(
                "Screenshot of {node}, {}x{} pixels (the screen is {}x{} points). Points to \
                 click are in these pixels. {mark}",
                shot.width, shot.height, shot.screen_width, shot.screen_height
            )
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
    })
}

fn grant_hint(permission: Permission) -> String {
    format!(
        "Ask the user to allow {} for Beep: Beep's Settings, This Mac, or System Settings, \
         Privacy and Security, {}.",
        permission.as_str(),
        permission.as_str()
    )
}

struct ShellArgs {
    command: String,
    timeout: Duration,
}

/// `[--timeout SECS] COMMAND...`. Several words after the options are one
/// command, quoted back together.
fn shell_args<S: AsRef<str>>(words: &[S]) -> Result<ShellArgs, String> {
    let mut words: Vec<&str> = words.iter().map(AsRef::as_ref).collect();
    let mut secs = DEFAULT_TIMEOUT_SECS;
    if let Some(flag) = words.first().copied()
        && (flag == "--timeout" || flag.starts_with("--timeout="))
    {
        let value = match flag.strip_prefix("--timeout=") {
            Some(value) => {
                words.remove(0);
                value.to_owned()
            }
            None if words.len() > 1 => {
                let value = words[1].to_owned();
                words.drain(..2);
                value
            }
            None => return Err("--timeout needs a number of seconds.".into()),
        };
        secs = value
            .parse::<u64>()
            .map_err(|_| format!("--timeout takes seconds, not {value:?}."))?
            .clamp(1, MAX_TIMEOUT_SECS);
    }
    let command = match &words[..] {
        [] => return Err(format!("No command. {USAGE}")),
        [one] => (*one).to_owned(),
        many => shlex::try_join(many.iter().copied()).map_err(|e| e.to_string())?,
    };
    Ok(ShellArgs {
        command,
        timeout: Duration::from_secs(secs),
    })
}

/// A command's result for the model.
fn report(ran: &Ran, limit: Duration) -> String {
    let mut out = String::new();
    if ran.fresh {
        out.push_str("[new shell, started in the home folder]\n");
    }
    out.push_str(&ran.output);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let status = match (ran.exit_code, ran.timed_out) {
        (_, true) => format!(
            "timed out after {}s and was stopped with its shell; the next command starts a new one",
            limit.as_secs()
        ),
        (Some(code), false) => format!("exit {code}"),
        (None, false) => "the shell ended; the next command starts a new one".to_owned(),
    };
    let _ = write!(out, "[{status}");
    if let Some(cwd) = &ran.cwd {
        let _ = write!(out, ", cwd {cwd}");
    }
    if ran.cut {
        out.push_str(", output cut");
    }
    out.push(']');
    out
}

/// The node `name` names: by name, ignoring case and spaces against dashes.
pub(super) fn find<'a>(nodes: &'a [NodeRow], name: &str) -> Option<&'a NodeRow> {
    nodes
        .iter()
        .find(|node| command_name(&node.name) == command_name(name))
}

/// A node's name as one word for a command, such as `build-box`.
fn command_name(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

fn finish(call_id: &str, result: Result<String, String>) -> ToolResult {
    match result {
        Ok(text) => ToolResult::success(call_id, text),
        Err(text) => ToolResult::failure(call_id, text),
    }
}

fn unknown(call_id: &str, name: &str, nodes: &[NodeRow]) -> ToolResult {
    let names: Vec<_> = nodes.iter().map(|node| node.name.as_str()).collect();
    let known = if names.is_empty() {
        "The user has no machines running beep-node.".to_owned()
    } else {
        format!("Machines: {}.", names.join(", "))
    };
    ToolResult::failure(call_id, format!("No machine called {name}. {known}"))
}

fn list(nodes: &[NodeRow], online: &nodes::Nodes) -> String {
    if nodes.is_empty() {
        return "The user has no machines running beep-node.".into();
    }
    let mut out = String::new();
    for node in nodes {
        let state = match online.online(&node.endpoint) {
            Some(live) => format!("online, offers {}", live.offers.join(", ")),
            None => format!("offline, last seen {}", ago(node)),
        };
        let os = node
            .env
            .as_ref()
            .map(|env| env.os.as_str())
            .unwrap_or("unknown OS");
        let _ = writeln!(out, "- {} ({os}): {state}", node.name);
    }
    out
}

fn info(node: &NodeRow) -> String {
    match &node.env {
        Some(env) => format!("{}\n{}", node.name, nodes::describe(env)),
        None => format!("{} has not reported what it has yet.", node.name),
    }
}

fn help(node: &NodeRow, online: Option<&Online>) -> String {
    let name = command_name(&node.name);
    let mut out = format!("{name} info\n{name} help\n");
    if online.is_some_and(|live| live.offers(shell::PROTOCOL)) {
        let _ = writeln!(out, "{name} shell [--timeout SECS] \"COMMAND\"");
    }
    if let Some(live) = online {
        let agents: Vec<&str> = live
            .offers
            .iter()
            .filter_map(|offer| agent::agent_of(offer))
            .collect();
        if !agents.is_empty() {
            let _ = writeln!(out, "{name} sessions");
        }
        for agent in agents {
            out.push_str(&agents::usage(&name, agent));
        }
    }
    if online.is_some_and(|live| live.offers(desktop::PROTOCOL)) {
        let _ = writeln!(out, "{name} {DESKTOP_USAGE}");
    }
    if online.is_some_and(|live| live.offers(crate::protocol::node::files::PROTOCOL)) {
        out.push_str(&files::usage(&name));
    }
    match online {
        Some(live) if !live.offers.is_empty() => {
            let _ = write!(out, "It offers: {}.", live.offers.join(", "));
        }
        Some(_) => out.push_str("It offers no tools yet."),
        None => out.push_str("It is offline, so it takes no commands now."),
    }
    out
}

fn ago(node: &NodeRow) -> String {
    let minutes = (Utc::now() - node.seen_at).num_minutes();
    match minutes {
        ..1 => "just now".into(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str) -> NodeRow {
        NodeRow {
            endpoint: "x".into(),
            name: name.into(),
            version: "0.1.0".into(),
            env: None,
            seen_at: Utc::now(),
        }
    }

    #[test]
    fn a_node_is_found_by_its_name_with_dashes() {
        let nodes = [row("Build Box"), row("mac")];
        assert_eq!(
            find(&nodes, "build-box").map(|n| n.name.as_str()),
            Some("Build Box")
        );
        assert!(find(&nodes, "laptop").is_none());
    }

    #[test]
    fn shell_args_take_a_timeout_and_rejoin_words() {
        let args = shell_args(&["df -h"]).unwrap();
        assert_eq!(args.command, "df -h");
        assert_eq!(args.timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        let args = shell_args(&["--timeout", "300", "ls", "my dir"]).unwrap();
        assert_eq!(args.command, "ls 'my dir'");
        assert_eq!(args.timeout, Duration::from_secs(300));
        let args = shell_args(&["--timeout=99999", "make"]).unwrap();
        assert_eq!(args.timeout, Duration::from_secs(MAX_TIMEOUT_SECS));
        assert!(shell_args::<&str>(&[]).is_err());
        assert!(shell_args(&["--timeout", "soon", "make"]).is_err());
    }

    #[test]
    fn a_report_ends_with_the_exit_code_and_folder() {
        let ran = Ran {
            output: "ok".into(),
            exit_code: Some(0),
            timed_out: false,
            cwd: Some("/srv/project".into()),
            fresh: true,
            cut: false,
        };
        assert_eq!(
            report(&ran, Duration::from_secs(60)),
            "[new shell, started in the home folder]\nok\n[exit 0, cwd /srv/project]"
        );
    }

    #[test]
    fn desktop_commands_parse() {
        assert_eq!(
            desktop_requests(&["screenshot"]).unwrap(),
            [Request::Screenshot {
                max_side: SCREENSHOT_SIDE
            }]
        );
        assert_eq!(
            desktop_requests(&["click", "12"]).unwrap(),
            [Request::Click {
                target: Target::Element { index: 12 },
                button: MouseButton::Left,
                count: 1
            }]
        );
        assert_eq!(
            desktop_requests(&["click", "640.5,400", "--right", "--double"]).unwrap(),
            [Request::Click {
                target: Target::Point { x: 640.5, y: 400.0 },
                button: MouseButton::Right,
                count: 2
            }]
        );
        assert_eq!(
            desktop_requests(&["type", "hello", "world"]).unwrap(),
            [Request::Type {
                text: "hello world".into()
            }]
        );
        let keys = desktop_requests(&["key", "cmd+l", "return"]).unwrap();
        assert_eq!(keys.len(), 2);
        assert!(matches!(&keys[1], Request::Key { press } if press.key == "return"));
        assert_eq!(
            desktop_requests(&["scroll", "up", "3", "--at", "7"]).unwrap(),
            [Request::Scroll {
                at: Some(Target::Element { index: 7 }),
                dx: 0,
                dy: -3
            }]
        );
        assert_eq!(
            desktop_requests(&["scroll", "down"]).unwrap(),
            [Request::Scroll {
                at: None,
                dx: 0,
                dy: DEFAULT_SCROLL_LINES
            }]
        );
        assert_eq!(
            desktop_requests(&["open", "Visual", "Studio", "Code"]).unwrap(),
            [Request::Open {
                target: "Visual Studio Code".into()
            }]
        );
        for bad in [
            &[][..],
            &["click"],
            &["click", "here"],
            &["click", "3", "--triple"],
            &["key", "hyper+x"],
            &["scroll", "sideways"],
            &["screenshot", "now"],
            &["dance"],
        ] {
            assert!(desktop_requests(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_screenshot_reaches_the_model_as_a_marked_image() {
        let images = ToolImages::default();
        let text = read_reply(
            "mac",
            Reply::Screenshot(desktop::Screenshot {
                jpeg: vec![0xff, 0xd8],
                width: 1280,
                height: 800,
                screen_width: 1512,
                screen_height: 945,
            }),
            &images,
        )
        .unwrap();
        assert!(
            text.starts_with("Screenshot of mac, 1280x800 pixels"),
            "{text}"
        );
        assert!(text.contains("[image "), "{text}");
    }

    #[test]
    fn a_missing_permission_says_how_to_grant_it() {
        let failed = read_reply(
            "mac",
            Reply::Failed {
                reason: "Screen Recording is off.".into(),
                missing: Some(Permission::ScreenRecording),
            },
            &ToolImages::default(),
        )
        .unwrap_err();
        assert!(
            failed.contains("allow Screen Recording for Beep"),
            "{failed}"
        );
    }

    #[test]
    fn an_offline_node_says_so_in_help() {
        assert!(help(&row("mac"), None).ends_with("It is offline, so it takes no commands now."));
    }
}
