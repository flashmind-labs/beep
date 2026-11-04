//! The tools `tools/list` gives, with their arguments.

use serde_json::{Value, json};

/// The tool the node answers itself rather than the server.
pub(super) const ASK_USER: &str = "ask_user";

/// Every tool, in the order agents see them.
pub(super) fn tools() -> Value {
    let machine = |what: &str| json!({ "type": "string", "description": what });
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
                at once. Or give `prompt` instead of `command` to start a coding agent there \
                with that prompt; it returns the agent session's job id at once and runs on its \
                own. Follow either with job_status and cancel. Commands start in the machine's \
                home folder, or in `cwd`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "machine": machine("The machine, by its name in list_machines"),
                    "command": { "type": "string", "description": "The shell command to run" },
                    "prompt": { "type": "string", "description": "Starts a coding agent with this prompt instead of running a command" },
                    "agent": { "type": "string", "description": "The coding agent for `prompt` when the machine has several, such as claude or codex" },
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
            "description": "How a job run_on started stands: a background command's state and \
                latest output, or a coding agent's status, plan and last message.",
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
            "description": "Stop a job run_on started: a background command stops with \
                everything it started; a coding agent stops its turn and its session stays.",
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
