//! The tools `tools/list` gives, with their arguments.

use serde_json::{Value, json};

/// The tool the node answers itself rather than the server.
pub(super) const ASK_USER: &str = "ask_user";

/// The tools that keep what the user told the agents, which only the
/// server holds.
const MEMORY: [&str; 4] = ["remember", "recall", "update_memory", "forget"];

/// Every tool, in the order agents see them; without the memory tools when
/// `memory` is off.
pub(super) fn tools(memory: bool) -> Value {
    let mut all = all();
    if !memory && let Value::Array(list) = &mut all {
        list.retain(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .is_none_or(|name| !MEMORY.contains(&name))
        });
    }
    all
}

fn all() -> Value {
    let machine = |what: &str| json!({ "type": "string", "description": what });
    let scope = json!({
        "type": "string",
        "description": "What it is about: user (the default), machine:<name>, project:<name> or account:<app>"
    });
    json!([
        {
            "name": "list_machines",
            "description": "List the user's machines: whether each is online, its OS, CPU, \
                memory, GPUs, installed tools and apps, what it can do (xcode, android, gpu, \
                cuda, docker, blender, unreal) and the coding agents on it. The machine you run \
                on is marked. Use the names it gives with the other tools.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "title": "List machines", "readOnlyHint": true }
        },
        {
            "name": "run_on",
            "description": "Run a shell command on one of the user's machines and return its \
                exit code and the last 200 lines of output (stops after timeout_secs, default \
                600, at most 1800), or with background=true start it there and return a job id \
                at once. Follow a background command with job_status and cancel. It starts in \
                the machine's home folder, or in `cwd`. To start a coding agent use start_agent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "machine": machine("The machine, by its name in list_machines"),
                    "command": { "type": "string", "description": "The shell command to run" },
                    "cwd": { "type": "string", "description": "The folder on that machine to run in" },
                    "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 1800 },
                    "background": { "type": "boolean", "description": "Starts the command and returns a job id without waiting" }
                },
                "required": ["machine"],
                "additionalProperties": false
            },
            "annotations": { "title": "Run on a machine", "destructiveHint": false, "openWorldHint": false }
        },
        {
            "name": "sync_project",
            "description": "Sync a project's files between the user's machines now, as Beep \
                keeps synced projects in step: scans for changes here and on the others, then \
                waits up to 90 seconds until `machine` (or every machine that syncs it) has \
                them, and says how each stands. Use it before running work on another machine \
                that needs files you just changed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "The synced project's name" },
                    "machine": machine("Wait only for this machine")
                },
                "required": ["project"],
                "additionalProperties": false
            },
            "annotations": { "title": "Sync a project", "idempotentHint": true }
        },
        {
            "name": "send_files",
            "description": "Copy files or folders from this machine to another of the user's \
                machines, into `dest` there (default its home folder). A name already taken \
                there gets a number, so nothing is replaced. Relative paths start in this \
                machine's home folder; give absolute paths for files in your project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "machine": machine("The machine to send to, by its name in list_machines"),
                    "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 20 },
                    "dest": { "type": "string", "description": "The folder on that machine" }
                },
                "required": ["machine", "paths"],
                "additionalProperties": false
            },
            "annotations": { "title": "Send files", "openWorldHint": true }
        },
        {
            "name": "job_status",
            "description": "How a background command run_on started stands: its state and latest \
                output. For a coding agent use agent_status.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "The job id run_on gave" } },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Job status", "readOnlyHint": true }
        },
        {
            "name": "cancel",
            "description": "Stop a background command run_on started, with everything it \
                started. For a coding agent use stop_agent.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "The job id run_on gave" } },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Cancel a job" }
        },
        {
            "name": "machine_log",
            "description": "Read the recent log of beep-node on one of the user's machines, to \
                debug the machine itself: its link to Beep, sync, agent sessions. Gives the \
                last `lines` lines (default 200, at most 1000), oldest first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "machine": machine("The machine, by its name in list_machines"),
                    "lines": { "type": "integer", "minimum": 1, "maximum": 1000 }
                },
                "required": ["machine"],
                "additionalProperties": false
            },
            "annotations": { "title": "Read a machine's log", "readOnlyHint": true }
        },
        {
            "name": "start_agent",
            "description": "Start another coding agent, of any harness, on any of the user's \
                machines, linked to this session: it shows under this session for the user, \
                who answers any permission question it asks. It starts in your folder (on \
                another machine, in the same project's checkout there) unless you give `cwd`, \
                and never above your own permission mode. Agents may start agents at most 3 \
                levels deep, and a session may have 8 running. Returns its id, or with \
                `wait` its answer once its turn ends; `wait` returns after about 45 seconds \
                at the latest with its state, so check long jobs with agent_status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "harness": { "type": "string", "description": "The coding agent, such as claude or codex, as list_machines names it" },
                    "prompt": { "type": "string", "description": "What the agent should do; it has no other context" },
                    "machine": machine("The machine, by its name in list_machines; this machine when left out"),
                    "cwd": { "type": "string", "description": "The folder to work in on that machine" },
                    "model": { "type": "string", "description": "The model, when not the agent's own" },
                    "wait": { "type": "boolean", "description": "Wait for its first turn to end, up to about 45 seconds" }
                },
                "required": ["harness", "prompt"],
                "additionalProperties": false
            },
            "annotations": { "title": "Start an agent", "destructiveHint": false, "openWorldHint": false }
        },
        {
            "name": "agent_status",
            "description": "How an agent you started stands: its state (working, waiting_on_user, \
                idle or ended), title, last message, how many files it changed and, when it \
                waits on a question, the question. You cannot answer its questions; the user \
                does. Only your own agents.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "The id start_agent gave" } },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Agent status", "readOnlyHint": true }
        },
        {
            "name": "send_to_agent",
            "description": "Send a message to an agent you started. It reads it after its current \
                turn. Only your own agents.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The id start_agent gave" },
                    "text": { "type": "string" }
                },
                "required": ["id", "text"],
                "additionalProperties": false
            },
            "annotations": { "title": "Message an agent", "destructiveHint": false, "openWorldHint": false }
        },
        {
            "name": "stop_agent",
            "description": "Stop an agent you started: its session ends, and so do the agents it \
                started. It stays in the user's list. Only your own agents.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "The id start_agent gave" } },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Stop an agent" }
        },
        {
            "name": "list_agents",
            "description": "List the agents you started, on every machine, with their state.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "title": "List agents", "readOnlyHint": true }
        },
        {
            "name": "remember",
            "description": "Save a lasting instruction or preference the user states, such as \
                \"always use the Windows machine for Blender\", in Beep's memories. Every coding \
                agent on every machine gets it, and the user sees it on the Memories page. Use it \
                when the user tells you how they always want something done, not for notes on \
                this task. One short self-contained sentence. Never store secrets.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "The rule as one short sentence that names its subject" },
                    "scope": scope
                },
                "required": ["content"],
                "additionalProperties": false
            },
            "annotations": { "title": "Remember", "destructiveHint": false, "openWorldHint": false }
        },
        {
            "name": "recall",
            "description": "Search Beep's memories of the user by meaning, best first; an empty \
                query lists the newest. Use it before a decision they may have spoken to, such \
                as which machine, tool or workflow to use. Each result has an id, which \
                update_memory and forget take.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to look for" },
                    "scope": scope
                },
                "additionalProperties": false
            },
            "annotations": { "title": "Recall", "readOnlyHint": true }
        },
        {
            "name": "update_memory",
            "description": "Replace a memory's text, when the user corrects it or it is wrong. \
                Give the id recall gave and the full new sentence.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The id recall gave" },
                    "content": { "type": "string", "description": "The full new sentence" },
                    "scope": scope
                },
                "required": ["id", "content"],
                "additionalProperties": false
            },
            "annotations": { "title": "Update a memory", "destructiveHint": false, "openWorldHint": false }
        },
        {
            "name": "forget",
            "description": "Delete a memory that no longer holds, by the id recall gave, when \
                the user asks or it is plainly wrong.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string", "description": "The id recall gave" } },
                "required": ["id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Forget a memory", "destructiveHint": true, "openWorldHint": false }
        },
        {
            "name": ASK_USER,
            "description": "Ask the user a question and wait for the answer. It shows in \
                Beep's Needs you on their desktop and phone. Give `options` for a choice; they \
                may still answer in their own words. Ask only what you cannot decide yourself.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "options": { "type": "array", "items": { "type": "string" }, "maxItems": 8 }
                },
                "required": ["question"],
                "additionalProperties": false
            },
            "annotations": { "title": "Ask the user", "readOnlyHint": true }
        }
    ])
}
