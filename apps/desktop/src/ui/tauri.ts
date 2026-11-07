// Tauri's global API (`withGlobalTauri`), typed for the pages.

/// A channel Rust streams events into, passed as an `invoke` argument.
export interface TauriChannel<T> {
  onmessage: (message: T) => void;
}

/// An event Rust emits to the webviews.
export interface TauriEvent<T> {
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
}

declare global {
  interface Window {
    __TAURI__: TauriGlobal;
  }
}

const core = window.__TAURI__.core;
export const { Channel } = core;

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
  "agent_view", "agent_modes", "agent_tasks", "agent_sessions", "agent_hosts", "center_home",
  "project_sync", "project_overview", "center_project", "project_git_state", "center_project_lists",
  "center_project_issues", "center_project_pull_requests", "center_project_runs",
]);
let timings: Timing[] = [];
let timingTimer: ReturnType<typeof setTimeout> | undefined;
function flushTimings() {
  clearTimeout(timingTimer);
  timingTimer = undefined;
  if (!timings.length) return;
  const samples = timings;
  timings = [];
  core.invoke("desktop_timings", { samples }).catch(() => {});
}
/// Queues bounded timing samples, without recording page arguments or command inputs.
export function recordTiming(kind: Timing["kind"], name: string, ms: number, outcome: Timing["outcome"] = "ok") {
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
  "agent_adapters_install", "agent_file_save", "agent_files_sync", "agent_review", "agent_review_start", "agent_tree_upload",
  "allow_mac_permission", "claude_sign_in", "complete_login", "connect_app", "connect_mcp_server", "create_project",
  "export_research", "follow_project", "follow_run", "found_machines", "found_projects", "gh_sign_in", "harness_compact",
  "import_project", "join_account", "link_offer", "login_hosted", "machine_sign_in", "mesh_join", "pick_folder",
  "project_add_to_machine", "project_copy", "project_git_move", "report_collect", "report_issue", "report_send",
  "rerun_prompt", "rescan_found", "research_begin", "research_send", "review_fix", "review_merge", "review_start",
  "save_file", "save_machine_log", "save_picture", "send_prompt", "send_to_group", "session_move", "sign_in",
  "start_login", "start_over", "terminal_attach", "terminal_open", "terminal_output", "terminal_start", "update_machine",
  "update_machine_tool", "use_cli_login", "use_own_server",
]);

/// Calls a Rust command, logging it with its machine and session when it
/// fails. A command that has not answered within `within` ms (by default
/// `COMMAND_WAIT_MS`, none for `UNTIMED` ones or one that streams into a
/// channel) fails with a plain error, so a call that never returns cannot
/// hold the page.
export function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>, within?: number): Promise<T> {
  const lifecycle = cmd === "agent_start" || cmd === "agent_resume";
  const at = Date.now();
  const measure = TIMED_COMMANDS.has(cmd) ? performance.now() : null;
  let timedOut = false;
  if (lifecycle) logInfo(`agent command: ${JSON.stringify({ command: cmd, stage: "requested", machine: args?.machineId })}`);
  const streams = Object.values(args ?? {}).some(v => v instanceof Channel);
  const limit = within ?? (UNTIMED.has(cmd) || streams ? null : COMMAND_WAIT_MS);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const call = core.invoke<T>(cmd, args);
  const limited = limit == null ? call : Promise.race([call, new Promise<never>((_, reject) => {
    timer = setTimeout(() => { timedOut = true; reject(`no answer in ${Math.round(limit / 1000)} s. Try again.`); }, limit);
  })]);
  return limited.then(value => {
    if (measure !== null) recordTiming("command", cmd, performance.now() - measure);
    if (lifecycle) {
      const started = value as { session?: { id?: string; machine_id?: string; status?: string } } | null;
      logInfo(`agent command: ${JSON.stringify({ command: cmd, stage: "returned", ms: Date.now() - at, session: started?.session?.id, machine: started?.session?.machine_id, status: started?.session?.status })}`);
    }
    return value;
  }).finally(() => clearTimeout(timer)).catch((e: unknown) => {
    if (measure !== null) recordTiming("command", cmd, performance.now() - measure, timedOut ? "timeout" : "error");
    const about = [["machine", args?.machineId], ["session", args?.session]].filter(([, v]) => typeof v === "string").map(([k, v]) => `${k} ${v}`).join(", ");
    logError(`${cmd} failed${about ? ` (${about})` : ""}: ${typeof e === "object" && e && !(e instanceof Error) ? JSON.stringify(e) : e}`);
    throw e;
  });
}

window.addEventListener("error", e => logError(e.error instanceof Error ? e.error.stack ?? String(e.error) : e.message));
window.addEventListener("unhandledrejection", e => logError(`unhandled rejection: ${e.reason instanceof Error ? e.reason.stack ?? e.reason : e.reason}`));
export const { listen } = window.__TAURI__.event;

