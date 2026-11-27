use super::AgentKind;
use agent_client_protocol::v1::schema::McpServer;
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio::process::Command;

const PI_COMMAND: &str = "PI_ACP_PI_COMMAND";
/// Match Beep's longest MCP tool waits (e.g. `remote_run` and `ask_user`).
const MCP_TIMEOUT_SECS: u32 = 600;
/// Leave room below Unix `SUN_LEN` (104 bytes on macOS and BSD).
#[cfg(unix)]
const MAX_UNIX_SOCKET_LEN: usize = 92;

/// Configure Beep's ACP and RPC shims around `pi-acp` and standalone `pi`.
///
/// `pi` natively supports in-turn RPC steering (`steer`), while `pi-acp` does
/// not implement `_session/steering` and blocks `session/cancel` on slow
/// extension `agent_settled` hooks after `agent_end`. Each session gets:
/// - `acp-bridge.mjs`, speaking ACP on stdio around `pi-acp` and advertising
///   `_meta.steering.supported = true`,
/// - `rpc-bridge.mjs`, wrapping `pi --mode rpc` so live steers inject into
///   running turns and aborted turns settle as soon as `agent_end` arrives,
/// - `beep.ts`, registering Beep's MCP server and content-filter recovery.
pub(super) fn setup(
    kind: &AgentKind,
    flag: Option<&str>,
    process: &mut Command,
    cwd: &Path,
    instructions: &mut Option<String>,
    mcp: Option<&McpServer>,
) -> Result<Option<TempDir>> {
    if !is_pi_acp(kind) {
        return Ok(None);
    }
    let dir = tempfile::tempdir().context("creating Pi's session settings")?;

    // Passing --append-system-prompt suppresses Pi's discovery of
    // APPEND_SYSTEM.md. Route saved instructions through the extension instead
    // when that file exists so Pi keeps loading it with its trust check.
    let agent_dir = value(process, "PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .map(|path| cwd.join(path))
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent")));
    let has_append_file = cwd.join(".pi/APPEND_SYSTEM.md").exists()
        || agent_dir.is_some_and(|dir| dir.join("APPEND_SYSTEM.md").exists());

    let mut extra_args: Vec<String> = Vec::new();
    let inline_instructions = match instructions.as_deref() {
        Some(text) if has_append_file => Some(text),
        Some(text) => {
            let path = dir.path().join("instructions.md");
            fs::write(&path, text).context("writing Pi's session instructions")?;
            extra_args.push("--append-system-prompt".to_owned());
            extra_args.push(path.to_string_lossy().into_owned());
            None
        }
        None => None,
    };

    let extension = extension_source(inline_instructions, mcp)?;
    let ext_path = dir.path().join("beep.ts");
    write_private(&ext_path, &extension).context("writing Pi's session extension")?;
    extra_args.push("--extension".to_owned());
    extra_args.push(ext_path.to_string_lossy().into_owned());

    let bridge_dir = dir.path().join("pi-acp");
    fs::create_dir_all(&bridge_dir).context("creating Pi's bridge folder")?;
    let acp_bridge = bridge_dir.join("acp-bridge.mjs");
    write_private(&acp_bridge, ACP_BRIDGE_SOURCE).context("writing Pi's ACP bridge")?;
    let rpc_bridge = bridge_dir.join("rpc-bridge.mjs");
    write_private(&rpc_bridge, RPC_BRIDGE_SOURCE).context("writing Pi's RPC bridge")?;

    let launcher = if cfg!(windows) { "pi.cmd" } else { "pi" };
    let pi = value(process, PI_COMMAND).unwrap_or_else(|| OsString::from(launcher));
    let wrapper = dir.path().join(launcher);
    write_launcher(&wrapper).context("preparing Pi's session launcher")?;

    let mut acp_args = kind.args.clone();
    if let Some(flag) = flag {
        acp_args.push(flag.to_owned());
    }
    let acp_args_json = serde_json::to_string(&acp_args)?;
    let extra_args_json = serde_json::to_string(&extra_args)?;

    process
        .arg(&acp_bridge)
        .env("BEEP_PI_ACP_PROGRAM", &kind.program)
        .env("BEEP_PI_ACP_ARGS", acp_args_json)
        .env("BEEP_PI_EXTRA_ARGS", extra_args_json)
        .env("BEEP_PI_COMMAND", pi)
        .env(PI_COMMAND, wrapper);

    #[cfg(unix)]
    {
        let sock = dir.path().join("b.sock");
        if sock.as_os_str().len() <= MAX_UNIX_SOCKET_LEN {
            process.env("BEEP_PI_BRIDGE_SOCK", sock);
        }
    }

    instructions.take();

    Ok(Some(dir))
}

/// The `node` executable to run `acp-bridge.mjs` when `is_pi_acp(kind)`.
pub(super) fn node_program(kind: &AgentKind) -> String {
    let path = Path::new(&kind.program);
    if path.file_stem().and_then(|s| s.to_str()) == Some("node") {
        return kind.program.clone();
    }
    let bin = if cfg!(windows) { "node.exe" } else { "node" };
    if let Some(sibling) = path.parent().map(|dir| dir.join(bin)).filter(|p| p.exists()) {
        return sibling.to_string_lossy().into_owned();
    }
    if let Some(found) = crate::machine::env::locate::find("node", dirs::home_dir().as_deref()) {
        return found.to_string_lossy().into_owned();
    }
    bin.to_owned()
}

pub(super) fn is_pi_acp(kind: &AgentKind) -> bool {
    if kind.name != "pi" || kind.bridge.is_some() {
        return false;
    }

    match Path::new(&kind.program)
        .file_stem()
        .and_then(|stem| stem.to_str())
    {
        Some("pi-acp") => true,
        Some("npx") => kind
            .args
            .iter()
            .any(|arg| arg == "pi-acp" || arg.starts_with("pi-acp@")),
        Some("node") => kind.args.first().is_some_and(|script| {
            Path::new(script)
                .ancestors()
                .any(|path| path.ends_with("node_modules/pi-acp"))
        }),
        _ => false,
    }
}

#[derive(Serialize)]
struct PiMcpServer<'a> {
    name: &'a str,
    config: PiMcpConfig<'a>,
}

#[derive(Serialize)]
struct PiMcpConfig<'a> {
    command: String,
    args: &'a [String],
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<&'a str, &'a str>,
    exposure: &'static str,
    timeout: u32,
}

fn mcp_server_spec(mcp: Option<&McpServer>) -> Option<PiMcpServer<'_>> {
    let McpServer::Stdio(stdio) = mcp? else {
        return None;
    };
    let env = stdio
        .env
        .iter()
        .map(|var| (var.name.as_str(), var.value.as_str()))
        .collect();

    Some(PiMcpServer {
        name: &stdio.name,
        config: PiMcpConfig {
            command: stdio.command.to_string_lossy().into_owned(),
            args: &stdio.args,
            env,
            exposure: "direct",
            timeout: MCP_TIMEOUT_SECS,
        },
    })
}

const ACP_BRIDGE_SOURCE: &str = r#"import { spawn, spawnSync } from "node:child_process";
import * as crypto from "node:crypto";
import * as net from "node:net";
import * as readline from "node:readline";

const acpProgram = process.env.BEEP_PI_ACP_PROGRAM || "pi-acp";
const acpArgs = JSON.parse(process.env.BEEP_PI_ACP_ARGS || "[]");
const passArgs = process.argv.slice(2);
const useShell = process.platform === "win32" && /\.(cmd|bat)$/i.test(acpProgram);

if (passArgs.includes("--terminal-login")) {
    const res = spawnSync(acpProgram, [...acpArgs, ...passArgs], {
        stdio: "inherit",
        env: { ...process.env, BEEP_NODE: process.execPath },
        shell: useShell,
    });
    process.exit(typeof res.status === "number" ? res.status : 1);
}

function promptToPiMessage(blocks) {
    let message = "";
    const images = [];
    for (const b of Array.isArray(blocks) ? blocks : []) {
        if (!b || typeof b !== "object") continue;
        switch (b.type) {
            case "text":
                if (typeof b.text === "string") message += b.text;
                break;
            case "resource_link":
                if (typeof b.uri === "string") message += `\n[Context] ${b.uri}`;
                break;
            case "image":
                if (typeof b.data === "string" && typeof b.mimeType === "string") {
                    images.push({ type: "image", mimeType: b.mimeType, data: b.data });
                }
                break;
            case "resource": {
                const r = b.resource;
                const uri = typeof r?.uri === "string" ? r.uri : "(unknown)";
                if (typeof r?.text === "string") {
                    const mime = typeof r?.mimeType === "string" ? r.mimeType : "text/plain";
                    message += `\n[Embedded Context] ${uri} (${mime})\n${r.text}`;
                } else if (typeof r?.blob === "string") {
                    const mime = typeof r?.mimeType === "string" ? r.mimeType : "application/octet-stream";
                    const bytes = Buffer.byteLength(r.blob, "base64");
                    message += `\n[Embedded Context] ${uri} (${mime}, ${bytes} bytes)`;
                } else {
                    message += `\n[Embedded Context] ${uri}`;
                }
                break;
            }
        }
    }
    return { message, images };
}

function writeOut(line) {
    if (!process.stdout.destroyed && process.stdout.writable) {
        process.stdout.write(`${line}\n`);
    }
}

function writeJson(obj) {
    writeOut(JSON.stringify(obj));
}

const token = crypto.randomBytes(24).toString("hex");
const sockPath = process.env.BEEP_PI_BRIDGE_SOCK || "";
const sessions = new Map();
const sockets = new Set();
let latestSocket = null;
let steerSeq = 0;
const pendingSteers = new Map();

function resolveSteer(reqId, outcome) {
    const entry = pendingSteers.get(reqId);
    if (!entry) return;
    pendingSteers.delete(reqId);
    const result = outcome === "promptRequired"
        ? { outcome: "promptRequired", reason: "noRunningTurn" }
        : { outcome };
    writeJson({ jsonrpc: "2.0", id: entry.id, result });
}

const server = net.createServer((sock) => {
    let authed = Boolean(sockPath);
    if (authed) {
        sockets.add(sock);
        latestSocket = sock;
    }
    const rl = readline.createInterface({ input: sock });
    rl.on("line", (line) => {
        if (!line.trim()) return;
        let msg;
        try {
            msg = JSON.parse(line);
        } catch {
            return;
        }
        if (!authed) {
            if (msg?.type === "hello" && msg.token === token) {
                authed = true;
                sockets.add(sock);
                latestSocket = sock;
            } else {
                sock.destroy();
            }
            return;
        }
        if (msg?.type === "session" && typeof msg.sessionId === "string") {
            sessions.set(msg.sessionId, sock);
            latestSocket = sock;
            return;
        }
        if (msg?.type === "steerResult" && typeof msg.reqId === "string") {
            resolveSteer(msg.reqId, String(msg.outcome || "failed"));
        }
    });
    const cleanup = () => {
        sockets.delete(sock);
        if (latestSocket === sock) {
            latestSocket = sockets.size > 0 ? [...sockets][sockets.size - 1] : null;
        }
        for (const [sid, s] of sessions) {
            if (s === sock) sessions.delete(sid);
        }
        for (const [reqId, entry] of pendingSteers) {
            if (entry.sock === sock) resolveSteer(reqId, "promptRequired");
        }
    };
    sock.on("close", cleanup);
    sock.on("error", cleanup);
});

const childEnv = { ...process.env, BEEP_NODE: process.execPath };
await new Promise((resolve, reject) => {
    server.once("error", reject);
    if (sockPath) {
        server.listen(sockPath, resolve);
    } else {
        server.listen(0, "127.0.0.1", () => {
            const addr = server.address();
            if (addr && typeof addr === "object") {
                childEnv.BEEP_PI_BRIDGE_PORT = String(addr.port);
                childEnv.BEEP_PI_BRIDGE_TOKEN = token;
            }
            resolve();
        });
    }
});
server.unref();

const child = spawn(acpProgram, [...acpArgs, ...passArgs], {
    stdio: ["pipe", "pipe", "inherit"],
    env: childEnv,
    shell: useShell,
});

let nativeSteering = false;
const initIds = new Set();

const childOut = readline.createInterface({ input: child.stdout });
childOut.on("line", (line) => {
    if (!line.trim()) {
        writeOut(line);
        return;
    }
    let msg;
    try {
        msg = JSON.parse(line);
    } catch {
        writeOut(line);
        return;
    }
    if (msg && initIds.has(msg.id)) {
        initIds.delete(msg.id);
        if (msg.result && typeof msg.result === "object") {
            if (msg.result?._meta?.steering?.supported === true) {
                nativeSteering = true;
            } else {
                msg.result._meta = {
                    ...(msg.result._meta ?? {}),
                    steering: { supported: true },
                };
            }
            writeJson(msg);
            return;
        }
    }
    const level = msg?.method === "session/update"
        ? msg.params?.update?._meta?.piAcp?.notify?.level
        : undefined;
    if (typeof level === "string" && level.startsWith("__beep_steer:")) {
        const reqId = level.slice("__beep_steer:".length);
        const entry = pendingSteers.get(reqId);
        pendingSteers.delete(reqId);
        if (entry) {
            if (entry.text) {
                writeJson({
                    jsonrpc: "2.0",
                    method: "session/update",
                    params: {
                        sessionId: entry.sessionId ?? msg.params?.sessionId,
                        update: {
                            sessionUpdate: "user_message_chunk",
                            content: { type: "text", text: entry.text },
                        },
                    },
                });
            }
            writeJson({
                jsonrpc: "2.0",
                id: entry.id,
                result: { outcome: "injected" },
            });
        }
        return;
    }
    writeOut(line);
});

const hostIn = readline.createInterface({ input: process.stdin });
hostIn.on("line", (line) => {
    if (!line.trim()) return;
    let msg;
    try {
        msg = JSON.parse(line);
    } catch {
        child.stdin.write(`${line}\n`);
        return;
    }
    if (msg?.method === "initialize" && msg.id !== undefined) {
        initIds.add(msg.id);
        child.stdin.write(`${line}\n`);
        return;
    }
    if (msg?.method === "_session/steering" && !nativeSteering && msg.id !== undefined) {
        const sessionId = typeof msg.params?.sessionId === "string" ? msg.params.sessionId : "";
        const sock = (sessionId && sessions.get(sessionId)) || latestSocket;
        if (!sock || sock.destroyed) {
            writeJson({
                jsonrpc: "2.0",
                id: msg.id,
                result: { outcome: "promptRequired", reason: "noRunningTurn" },
            });
            return;
        }
        const { message, images } = promptToPiMessage(msg.params?.prompt);
        const reqId = String(++steerSeq);
        pendingSteers.set(reqId, { id: msg.id, sessionId, text: message, sock });
        sock.write(`${JSON.stringify({ type: "steer", reqId, message, images })}\n`);
        return;
    }
    child.stdin.write(`${line}\n`);
});

let exiting = false;
function shutdown(code = 0) {
    if (exiting) return;
    exiting = true;
    try { server.close(); } catch {}
    try { if (!child.killed) child.kill("SIGTERM"); } catch {}
    process.exit(code);
}

child.on("exit", (code) => shutdown(typeof code === "number" ? code : 0));
child.on("error", () => shutdown(1));
process.stdin.on("end", () => { try { child.stdin.end(); } catch {} });
process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));
"#;

const RPC_BRIDGE_SOURCE: &str = r#"import { spawn, spawnSync } from "node:child_process";
import * as net from "node:net";
import * as readline from "node:readline";

const piCmd = process.env.BEEP_PI_COMMAND || (process.platform === "win32" ? "pi.cmd" : "pi");
const extraArgs = JSON.parse(process.env.BEEP_PI_EXTRA_ARGS || "[]");
const args = process.argv.slice(2);
const useShell = process.platform === "win32" && /\.(cmd|bat)$/i.test(piCmd);

const modeIdx = args.indexOf("--mode");
const isRpc = modeIdx !== -1 && args[modeIdx + 1] === "rpc";

if (!isRpc) {
    const res = spawnSync(piCmd, args, {
        stdio: "inherit",
        env: process.env,
        shell: useShell,
    });
    process.exit(typeof res.status === "number" ? res.status : 1);
}

const child = spawn(piCmd, [...extraArgs, ...args], {
    stdio: ["pipe", "pipe", "pipe"],
    env: process.env,
    shell: useShell,
});
child.stderr.on("data", (chunk) => {
    if (!process.stderr.destroyed && process.stderr.writable) {
        process.stderr.write(chunk);
    }
});

function writeAcp(line) {
    if (!process.stdout.destroyed && process.stdout.writable) {
        process.stdout.write(`${line}\n`);
    }
}

function writePi(obj) {
    if (!child.stdin.destroyed && child.stdin.writable) {
        child.stdin.write(`${JSON.stringify(obj)}\n`);
    }
}

const sockPath = process.env.BEEP_PI_BRIDGE_SOCK || "";
const port = Number(process.env.BEEP_PI_BRIDGE_PORT || "0");
const token = process.env.BEEP_PI_BRIDGE_TOKEN || "";

let ipc = null;
const ipcQueue = [];
function sendIpc(obj) {
    const line = `${JSON.stringify(obj)}\n`;
    if (ipc && !ipc.destroyed && ipc.writable) {
        ipc.write(line);
    } else {
        ipcQueue.push(line);
    }
}

if (sockPath || port > 0) {
    ipc = sockPath ? net.createConnection(sockPath) : net.createConnection(port, "127.0.0.1");
    ipc.unref();
    ipc.on("connect", () => {
        if (!sockPath && token) {
            ipc.write(`${JSON.stringify({ type: "hello", token })}\n`);
        }
        while (ipcQueue.length > 0) {
            ipc.write(ipcQueue.shift());
        }
    });
    ipc.on("error", () => {});
    const ipcLines = readline.createInterface({ input: ipc });
    ipcLines.on("line", (line) => {
        if (!line.trim()) return;
        let msg;
        try {
            msg = JSON.parse(line);
        } catch {
            return;
        }
        if (msg?.type === "steer" && typeof msg.reqId === "string") {
            handleSteer(msg.reqId, String(msg.message ?? ""), Array.isArray(msg.images) ? msg.images : []);
        }
    });
}

let seq = 0;
let promptInFlight = false;
let inAgentRun = false;
let agentEnded = false;
let abortRequested = false;
let hasSeenAssistantInRun = false;
let prevSteeringLen = 0;
let steersPopped = 0;
let suppressSettledCount = 0;

const promptIds = new Set();
const pendingAbortIds = [];
const suppressedAbortIds = new Set();
const pendingSteers = [];

function flushPendingSteersAsPromptRequired() {
    if (pendingSteers.length === 0) return;
    const hadQueued = pendingSteers.some((s) => s.sent);
    while (pendingSteers.length > 0) {
        const entry = pendingSteers.shift();
        sendIpc({ type: "steerResult", reqId: entry.reqId, outcome: "promptRequired" });
    }
    prevSteeringLen = 0;
    steersPopped = 0;
    if (hadQueued) {
        writePi({ id: `__beep_clear_${++seq}`, type: "clear_queue" });
    }
}

function fastSettleIfReady() {
    if (!abortRequested || !agentEnded || !promptInFlight) return;
    flushPendingSteersAsPromptRequired();
    for (const id of pendingAbortIds.splice(0)) {
        suppressedAbortIds.add(id);
        writeAcp(JSON.stringify({ id, type: "response", command: "abort", success: true }));
    }
    promptInFlight = false;
    inAgentRun = false;
    agentEnded = false;
    abortRequested = false;
    hasSeenAssistantInRun = false;
    suppressSettledCount += 1;
    writeAcp(JSON.stringify({ type: "agent_settled", aborted: true }));
}

function emitSteerFence(reqId) {
    writeAcp(JSON.stringify({
        type: "extension_ui_request",
        id: `__beep_sync_${reqId}`,
        method: "notify",
        message: "",
        notifyType: `__beep_steer:${reqId}`,
    }));
}

function handleSteer(reqId, message, images) {
    if (!inAgentRun || agentEnded || abortRequested) {
        sendIpc({ type: "steerResult", reqId, outcome: "promptRequired" });
        return;
    }
    const rpcId = `__beep_steer_${++seq}`;
    pendingSteers.push({ reqId, rpcId, sent: true });
    writePi({ id: rpcId, type: "steer", message, images });
}

const piOut = readline.createInterface({ input: child.stdout });
piOut.on("line", (line) => {
    if (!line.trim()) {
        writeAcp(line);
        return;
    }
    let msg;
    try {
        msg = JSON.parse(line);
    } catch {
        writeAcp(line);
        return;
    }

    if (msg?.type === "response") {
        if (typeof msg.id === "string" && msg.id.startsWith("__beep_")) {
            const idx = pendingSteers.findIndex((s) => s.rpcId === msg.id);
            if (idx !== -1) {
                const entry = pendingSteers[idx];
                if (!msg.success) {
                    pendingSteers.splice(idx, 1);
                    sendIpc({ type: "steerResult", reqId: entry.reqId, outcome: "failed" });
                } else if (msg.data?.disposition === "handled") {
                    pendingSteers.splice(idx, 1);
                    emitSteerFence(entry.reqId);
                } else if (!inAgentRun || agentEnded || abortRequested) {
                    flushPendingSteersAsPromptRequired();
                }
            }
            return;
        }
        if (msg.id !== undefined && suppressedAbortIds.has(msg.id)) {
            suppressedAbortIds.delete(msg.id);
            return;
        }
        if (msg.id !== undefined && promptIds.has(msg.id)) {
            promptIds.delete(msg.id);
            if (!msg.success) {
                promptInFlight = false;
                inAgentRun = false;
                agentEnded = false;
                abortRequested = false;
                flushPendingSteersAsPromptRequired();
            }
        }
        if (msg.command === "get_state" && msg.success && typeof msg.data?.sessionId === "string") {
            sendIpc({ type: "session", sessionId: msg.data.sessionId });
        }
        writeAcp(line);
        return;
    }

    switch (msg?.type) {
        case "agent_start":
            promptInFlight = true;
            inAgentRun = true;
            agentEnded = false;
            hasSeenAssistantInRun = false;
            prevSteeringLen = 0;
            steersPopped = 0;
            writeAcp(line);
            return;
        case "queue_update": {
            const nextLen = Array.isArray(msg.steering) ? msg.steering.length : 0;
            if (nextLen < prevSteeringLen) {
                steersPopped += prevSteeringLen - nextLen;
            }
            prevSteeringLen = nextLen;
            writeAcp(line);
            return;
        }
        case "message_start":
            writeAcp(line);
            if (msg.message?.role === "assistant") {
                hasSeenAssistantInRun = true;
            } else if (
                msg.message?.role === "user" &&
                pendingSteers.length > 0 &&
                (steersPopped > 0 || hasSeenAssistantInRun)
            ) {
                if (steersPopped > 0) steersPopped -= 1;
                const entry = pendingSteers.shift();
                emitSteerFence(entry.reqId);
            }
            return;
        case "agent_end":
            writeAcp(line);
            if (msg.willRetry !== true) {
                inAgentRun = false;
                agentEnded = true;
                hasSeenAssistantInRun = false;
                flushPendingSteersAsPromptRequired();
                fastSettleIfReady();
            }
            return;
        case "agent_settled":
            if (suppressSettledCount > 0) {
                suppressSettledCount -= 1;
                return;
            }
            promptInFlight = false;
            inAgentRun = false;
            agentEnded = false;
            abortRequested = false;
            hasSeenAssistantInRun = false;
            pendingAbortIds.length = 0;
            flushPendingSteersAsPromptRequired();
            writeAcp(line);
            return;
        default:
            writeAcp(line);
    }
});

const acpIn = readline.createInterface({ input: process.stdin });
acpIn.on("line", (line) => {
    if (!line.trim()) return;
    let msg;
    try {
        msg = JSON.parse(line);
    } catch {
        child.stdin.write(`${line}\n`);
        return;
    }
    if (
        msg?.type === "extension_ui_response" &&
        typeof msg.id === "string" &&
        msg.id.startsWith("__beep_sync_")
    ) {
        return;
    }
    if (msg?.type === "prompt") {
        if (msg.id !== undefined) promptIds.add(msg.id);
        promptInFlight = true;
        agentEnded = false;
        abortRequested = false;
    } else if (msg?.type === "abort") {
        abortRequested = true;
        if (msg.id !== undefined) pendingAbortIds.push(msg.id);
        child.stdin.write(`${line}\n`);
        fastSettleIfReady();
        return;
    }
    child.stdin.write(`${line}\n`);
});

child.on("exit", (code) => {
    flushPendingSteersAsPromptRequired();
    process.exit(typeof code === "number" ? code : 0);
});
child.on("error", () => {
    flushPendingSteersAsPromptRequired();
    process.exit(1);
});
process.stdin.on("end", () => { try { child.stdin.end(); } catch {} });
process.on("SIGINT", () => { try { child.kill("SIGINT"); } catch {} });
process.on("SIGTERM", () => { try { child.kill("SIGTERM"); } catch {} });
"#;

fn extension_source(instructions: Option<&str>, mcp: Option<&McpServer>) -> Result<String> {
    let instructions_json = match instructions {
        Some(text) => serde_json::to_string(text)?,
        None => "null".to_owned(),
    };
    let mcp_json = match mcp_server_spec(mcp) {
        Some(spec) => serde_json::to_string(&spec)?,
        None => "null".to_owned(),
    };

    Ok(format!(
        r#"const instructions = {instructions_json};
const mcpServer = {mcp_json};
const MAX_CONTENT_FILTER_RETRIES = 2;

export default function (pi) {{
    if (mcpServer !== null && typeof pi.registerMcpServer === 'function') {{
        const registered =
            typeof pi.getMcpServers === 'function'
                ? pi.getMcpServers().some((server) => server?.name === mcpServer.name)
                : false;
        if (!registered) {{
            try {{
                pi.registerMcpServer(mcpServer.name, mcpServer.config);
            }} catch {{
                // Leave Pi running if another extension or config already registered it.
            }}
        }}
    }}

    let contentFilterRetries = 0;
    let unrecoveredError = null;

    pi.on('before_agent_start', (event) => {{
        contentFilterRetries = 0;
        unrecoveredError = null;
        if (instructions !== null) {{
            return {{
                systemPrompt: `${{event.systemPrompt}}\n\n${{instructions}}`,
            }};
        }}
    }});

    pi.on('agent_start', () => {{
        unrecoveredError = null;
    }});

    pi.on('agent_end', (event) => {{
        const messages = Array.isArray(event?.messages) ? event.messages : [];
        let last = null;
        for (let i = messages.length - 1; i >= 0; i -= 1) {{
            if (messages[i]?.role === 'assistant') {{
                last = messages[i];
                break;
            }}
        }}
        if (!last || last.stopReason !== 'error') {{
            unrecoveredError = null;
            return;
        }}

        const rawStopReason = typeof last.rawStopReason === 'string' ? last.rawStopReason : '';
        const errorMessage = typeof last.errorMessage === 'string' ? last.errorMessage.trim() : '';
        const isContentFilter =
            rawStopReason === 'content_filter' || /\bcontent_filter\b/i.test(errorMessage);

        if (isContentFilter && contentFilterRetries < MAX_CONTENT_FILTER_RETRIES) {{
            contentFilterRetries += 1;
            const tools = Array.isArray(last.content)
                ? [
                      ...new Set(
                          last.content
                              .filter((block) => block?.type === 'toolCall' && typeof block.name === 'string')
                              .map((block) => block.name),
                      ),
                  ]
                : [];
            const target =
                tools.length > 0
                    ? `Your previous response and its pending tool call(s) (${{tools.join(', ')}}) were blocked`
                    : 'Your previous response was blocked';
            pi.sendMessage(
                {{
                    customType: 'beep-error-recovery',
                    content: `${{target}} by the provider's content filter (content_filter) and did not run. Do not repeat that command or wording; either try a different, safe approach or explain to the user why you cannot perform it.`,
                    display: false,
                }},
                {{ triggerTurn: true, deliverAs: 'followUp' }},
            );
            return;
        }}

        unrecoveredError = errorMessage || 'The model request failed.';
    }});

    pi.on('agent_settled', (event, ctx) => {{
        if (event?.aborted || !unrecoveredError) {{
            unrecoveredError = null;
            return;
        }}
        const message = /^error\b/i.test(unrecoveredError)
            ? unrecoveredError
            : `Error: ${{unrecoveredError}}`;
        unrecoveredError = null;
        ctx?.ui?.notify?.(message, 'error');
    }});
}}
"#
    ))
}

fn value(process: &Command, name: &str) -> Option<OsString> {
    process
        .as_std()
        .get_envs()
        .find(|(key, _)| *key == name)
        .map_or_else(|| env::var_os(name), |(_, value)| value.map(OsString::from))
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    fs::write(path, contents)
}

#[cfg(unix)]
fn write_launcher(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::write(
        path,
        "#!/bin/sh\nexec \"${BEEP_NODE:-node}\" \"${0%/*}/pi-acp/rpc-bridge.mjs\" \"$@\"\n",
    )?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn write_launcher(path: &Path) -> std::io::Result<()> {
    fs::write(
        path,
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\nif defined BEEP_NODE (\r\n  \"%BEEP_NODE%\" \"%~dp0pi-acp\\rpc-bridge.mjs\" %*\r\n) else (\r\n  node \"%~dp0pi-acp\\rpc-bridge.mjs\" %*\r\n)\r\n",
    )
}
