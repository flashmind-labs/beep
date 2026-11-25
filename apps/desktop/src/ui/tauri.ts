// Tauri's global API (`withGlobalTauri`), typed for the pages.
import { create } from "zustand";

/// A channel Rust streams events into, passed as an `invoke` argument.
interface TauriChannel<T> {
  /// The callback Rust's messages go to.
  id: number;
  onmessage: (message: T) => void;
}

/// An event Rust emits to the webviews.
interface TauriEvent<T> {
  payload: T;
}

interface TauriGlobal {
  core: {
    invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
    Channel: new <T = unknown>() => TauriChannel<T>;
  };
  event: {
    listen<T = unknown>(event: string, handler: (event: TauriEvent<T>) => void): Promise<() => void>;
  };
  /// Absent from the browser tests' stand-in.
  app?: {
    /// Android's back button calls `handler` instead of closing the app.
    onBackButtonPress(handler: () => void): Promise<unknown>;
  };
  webviewWindow: {
    /// Its `listen` hears only events sent to this window; `event.listen` hears them for every window.
    getCurrentWebviewWindow(): {
      readonly label: string;
      listen<T = unknown>(event: string, handler: (event: TauriEvent<T>) => void): Promise<() => void>;
      /// The title bar's theme on Windows and Linux. Absent from the tests' stand-ins.
      setTheme?(theme: "light" | "dark"): Promise<void>;
    };
  };
}

/// What Rust's channel messages call: one with its index, then the end
/// with the count sent.
type ChannelCall = { message: unknown; index: number } | { end: true; index: number };

interface TauriInternals {
  transformCallback(callback: (call: ChannelCall) => void, once?: boolean): number;
  unregisterCallback(id: number): void;
}

declare global {
  interface Window {
    __TAURI__: TauriGlobal;
    __BEEP_RESTORING_WINDOW__?: boolean;
    __TAURI_INTERNALS__: TauriInternals;
    /// When the app put the kept Home frame on the page, in milliseconds since the Unix epoch.
    __BEEP_KEPT_FRAME_AT__?: number;
  }
}

const core = window.__TAURI__.core;
export const { Channel } = core;

export interface BuildCapabilities {
  local_host: boolean;
  metrics_recording: boolean;
  autostart: boolean;
  image_clipboard: boolean;
  desktop_control: boolean;
}

const NO_CAPABILITIES: BuildCapabilities = {
  local_host: false, metrics_recording: false, autostart: false,
  image_clipboard: false, desktop_control: false,
};

function capabilitiesFrom(value: unknown): BuildCapabilities {
  if (typeof value !== "object" || value === null
    || !("local_host" in value) || typeof value.local_host !== "boolean"
    || !("metrics_recording" in value) || typeof value.metrics_recording !== "boolean"
    || !("autostart" in value) || typeof value.autostart !== "boolean"
    || !("image_clipboard" in value) || typeof value.image_clipboard !== "boolean"
    || !("desktop_control" in value) || typeof value.desktop_control !== "boolean") {
    throw new Error("The app did not report its available features.");
  }

  return {
    local_host: value.local_host, metrics_recording: value.metrics_recording,
    autostart: value.autostart, image_clipboard: value.image_clipboard,
    desktop_control: value.desktop_control,
  };
}

/// One native read per window; pending or invalid replies expose no local capabilities.
export const useBuildCapabilities = create<BuildCapabilities>(() => NO_CAPABILITIES);
const capabilitiesLoaded = core.invoke<unknown>("build_capabilities").then(capabilitiesFrom).then(value => {
  useBuildCapabilities.setState(value, true);
  return value;
}).catch((error: unknown) => {
  logError(String(error));
  return NO_CAPABILITIES;
});

export function buildCapabilities(): Promise<BuildCapabilities> {
  return capabilitiesLoaded;
}

/// A channel that hands over each message with its index as it arrives.
/// Tauri's own holds every later message back until the one before comes,
/// and loses one for good when its fetch fails, as when its IPC falls back to
/// postMessage partway: the stream would stop for good.
export function indexedChannel(deliver: (message: unknown, index: number) => void): TauriChannel<unknown> {
  const internals = window.__TAURI_INTERNALS__;
  const channel = new Channel();
  let received = 0;
  let total: number | null = null;
  internals.unregisterCallback(channel.id);

  const id = internals.transformCallback(call => {
    if ("end" in call) total = call.index;
    else {
      received++;
      deliver(call.message, call.index);
    }

    if (received === total) internals.unregisterCallback(id);
  });
  channel.id = id;
  return channel;
}

/// Sends a page's error to the app's log.
export function logError(message: string) {
  core.invoke("log_webview", { message }).catch(() => {});
}

/// Sends a diagnostic to the same local log, without marking it as an error.
export function logInfo(message: string) {
  core.invoke("log_webview", { message, info: true }).catch(() => {});
}

type Timing = { kind: "page" | "data" | "command"; name: string; ms: number; outcome: "ok" | "error" | "timeout" };
const TIMED_COMMANDS = new Set([
  "machine_clocks", "project_sync_detail", "agent_worktrees", "session_moves", "session_move_progress",
  "agent_view", "agent_modes", "agent_tasks", "agent_sessions", "agent_hosts", "center_home",
  "project_sync", "project_git_state", "center_project_lists", "center_project_runs",
]);
let timings: Timing[] = [];
let timingTimer: ReturnType<typeof setTimeout> | null = null;
function flushTimings() {
  if (timingTimer !== null) clearTimeout(timingTimer);
  timingTimer = null;
  if (!timings.length) return;
  const samples = timings;
  timings = [];
  core.invoke("desktop_timings", { samples }).catch(() => {});
}
/// Queues bounded timing samples, without recording page arguments or command inputs.
export function recordTiming(kind: Timing["kind"], name: string, ms: number, outcome: Timing["outcome"] = "ok") {
  if (!useBuildCapabilities.getState().metrics_recording) return;
  if (!Number.isFinite(ms) || ms < 0 || ms > 600_000) return;
  timings.push({ kind, name, ms, outcome });
  if (timings.length >= 200) flushTimings();
  else timingTimer ??= setTimeout(flushTimings, 5_000);
}
window.addEventListener("pagehide", flushTimings);
document.addEventListener("visibilitychange", () => { if (document.hidden) flushTimings(); });

/// How long a command may take unless its call says otherwise (#428).
const COMMAND_WAIT_MS = 120_000;
/// Commands that wait on the user (sign-ins, dialogs) or do long work
/// (installs, moves, imports, reports, runs): no limit unless their call sets one.
const UNTIMED = new Set([
  "apply_machine_mcp_server",
  "agent_adapters_install", "agent_file", "agent_file_open", "agent_file_save", "agent_review", "agent_review_start", "agent_tree_upload",
  "allow_mac_permission", "claude_sign_in", "create_project", "follow_project", "found_machines", "found_projects",
  "gh_sign_in", "harness_compact", "import_project", "machine_sign_in", "mesh_join",
  "plugins_stage_folder", "plugins_stage_git", "plugins_stage_update",
  "project_add_to_machine", "project_git_move", "report_collect", "report_issue", "report_send", "rescan_found",
  "save_machine_log", "save_picture", "session_move", "chat_move", "terminal_attach", "terminal_open", "terminal_output",
  "update_machine", "update_machine_tool", "use_cli_login",
]);

/// Calls a Rust command, logging it with its machine and session when it
/// fails. A command that has not answered within `within` ms (by default
/// `COMMAND_WAIT_MS`, none for `UNTIMED` ones or one that streams into a
/// channel) fails with a plain error, so a call that never returns cannot
/// hold the page.
let nextInvocation = 0;
let traceCommands = false;
void core.invoke<boolean>("desktop_trace_enabled").then(enabled => { traceCommands = enabled === true; }).catch(() => {});

/// Whether a command was refused as a conflict, such as a question another device answered first.
export const conflicted = (error: unknown) => /^conflict/i.test(String(error));

export function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>, within?: number): Promise<T> {
  const request = ++nextInvocation;
  const started = performance.now();
  const trace = (stage: string, outcome: string) => {
    if (traceCommands) logInfo(`command trace: ${JSON.stringify({ request, command: cmd, stage, outcome, ms: Math.round(performance.now() - started) })}`);
  };
  trace("requested", "pending");
  const lifecycle = cmd === "agent_start" || cmd === "agent_begin" || cmd === "agent_resume";
  const at = Date.now();
  const measure = TIMED_COMMANDS.has(cmd) ? performance.now() : null;
  let timedOut = false;
  if (lifecycle) logInfo(`agent command: ${JSON.stringify({ command: cmd, stage: "requested", machine: args?.machineId })}`);
  const streams = Object.values(args ?? {}).some(v => v instanceof Channel);
  const limit = within ?? (UNTIMED.has(cmd) || streams ? null : COMMAND_WAIT_MS);
  let timer: ReturnType<typeof setTimeout> | null = null;
  const call = core.invoke<T>(cmd, args);
  const limited = limit == null ? call : Promise.race([call, new Promise<never>((_, reject) => {
    timer = setTimeout(() => { timedOut = true; reject(`no answer in ${Math.round(limit / 1000)} s. Try again.`); }, limit);
  })]);
  return limited.then(value => {
    trace("returned", "ok");
    if (measure !== null) recordTiming("command", cmd, performance.now() - measure);

    if (lifecycle) {
      const started = value as { session?: { id?: string; machine_id?: string; status?: string } } | null;
      logInfo(`agent command: ${JSON.stringify({ command: cmd, stage: "returned", ms: Date.now() - at, session: started?.session?.id, machine: started?.session?.machine_id, status: started?.session?.status })}`);
    }

    return value;
  }).finally(() => {
    if (timer !== null) clearTimeout(timer);
  }).catch((e: unknown) => {
    trace("returned", timedOut ? "timeout" : "error");
    if (measure !== null) recordTiming("command", cmd, performance.now() - measure, timedOut ? "timeout" : "error");
    // A lost link the caller reads again after is not an error; Rust logged why.
    if (linkLost(e)) throw e;
    const about = [["machine", args?.machineId], ["session", args?.session]].filter(([, v]) => typeof v === "string").map(([k, v]) => `${k} ${v}`).join(", ");
    logError(`${cmd} failed${about ? ` (${about})` : ""}: ${typeof e === "object" && e && !(e instanceof Error) ? JSON.stringify(e) : e}`);
    throw e;
  });
}

/// A command's `{ kind: "lost" }` failure: the machine's link dropped or was not up yet.
const linkLost = (e: unknown) => typeof e === "object" && e !== null && "kind" in e && e.kind === "lost";

window.addEventListener("error", e => logError(e.error instanceof Error ? `${e.error.name}: ${e.error.message}\n${e.error.stack ?? ""}` : e.message));
window.addEventListener("unhandledrejection", e => logError(`unhandled rejection: ${e.reason instanceof Error ? e.reason.stack ?? e.reason : e.reason}`));
export const { listen } = window.__TAURI__.event;

/// Opens `url` in the user's browser.
export function openUrl(url: string) {
  invoke("open_url", { url }).catch(e => console.warn("opening a link:", e));
}
