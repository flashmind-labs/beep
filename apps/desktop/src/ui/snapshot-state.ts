// Current rendered views received from their source, bounded to this visible window.

export interface ViewSnapshot {
  command: string;
  args?: Record<string, unknown>;
  value?: unknown;
  error?: string;
}

const VIEW_COMMANDS = [
  "center_home", "list_machines", "mac_control", "projects_on_top", "project_groups",
  "project_sync", "git_items", "usage_get", "agent_accounts", "center_project_lists",
  "center_project_runs", "center_project_run", "center_project_issues", "center_project_pull_requests",
] as const;
export type ViewCommand = typeof VIEW_COMMANDS[number];
const COMMANDS = new Set<string>(VIEW_COMMANDS);
const MAX_VIEWS = 128;
const MAX_BYTES = 8 * 1024 * 1024;
export type CachedView =
  | { kind: "value"; value: unknown }
  | { kind: "error"; error: string };
interface Snapshot { command: string; args: Record<string, unknown>; result: CachedView }
const views = new Map<string, CachedView & { command: string; bytes: number }>();
let bytes = 0;
/// The newest projections received and not read yet. A view can change many times a second, so
/// measuring and copying one waits until it is read, once for all the updates before.
const pending = new Map<string, Snapshot>();

/// Install every projection before notifying any subscriber of the source update.
export function installViews(data: unknown): void {
  if (!record(data) || !Array.isArray(data.views)) return;

  for (const value of data.views) {
    const snapshot = readSnapshot(value);
    if (!snapshot) continue;
    const id = key(snapshot.command, snapshot.args);
    pending.delete(id);
    pending.set(id, snapshot);
  }

  if (pending.size > MAX_VIEWS) settle();
}

/// Moves the pending projections into the bounded cache.
function settle(): void {
  for (const [id, snapshot] of pending) {
    const { result } = snapshot;
    const size = JSON.stringify(result.kind === "error" ? result.error : result.value)?.length ?? MAX_BYTES + 1;
    const previous = views.get(id);
    if (previous) { bytes -= previous.bytes; views.delete(id); }
    if (size > MAX_BYTES) continue;
    views.set(id, { ...clone(result), command: snapshot.command, bytes: size });
    bytes += size;

    while (views.size > MAX_VIEWS || bytes > MAX_BYTES) {
      const oldest = views.entries().next().value;
      if (!oldest) break;
      bytes -= oldest[1].bytes;
      views.delete(oldest[0]);
    }
  }

  pending.clear();
}

/// Explicit refreshes read the source. Callers own their copy of each rendered view.
export function snapshotView(command: string, args?: Record<string, unknown>): CachedView | null {
  if (!COMMANDS.has(command) || args?.refresh === true) return null;
  settle();
  const held = views.get(key(command, args));
  return held ? clone(held) : null;
}

/// A failure is delivered once, then the subscriber's retry can ask the recovered source.
export function takeSnapshotView(command: string, args?: Record<string, unknown>): CachedView | null {
  const snapshot = snapshotView(command, args);

  if (snapshot?.kind === "error") {
    const id = key(command, args), held = views.get(id);
    if (held) { bytes -= held.bytes; views.delete(id); }
  }

  return snapshot;
}

export function clearViews(): void {
  views.clear();
  pending.clear();
  bytes = 0;
}

/// Which rendered views a command can change before their next snapshot arrives.
export type WriteScope =
  | { kind: "none" }
  | { kind: "views"; commands: readonly ViewCommand[] }
  | { kind: "all" };

const SESSIONS: readonly ViewCommand[] = ["center_home"];
const FORGE: readonly ViewCommand[] = ["center_project_lists", "center_project_issues", "center_project_pull_requests", "git_items", "center_home"];
const ACCOUNTS: readonly ViewCommand[] = ["agent_accounts", "usage_get"];
/// Frequent writes and the views they change. Other writes drop every view.
const WRITES: Readonly<Record<string, readonly ViewCommand[]>> = {
  send_prompt: SESSIONS, agent_steer: SESSIONS, agent_start: SESSIONS, agent_resume: SESSIONS,
  agent_delete: SESSIONS, agent_title_save: SESSIONS, agent_stop_process: SESSIONS, agent_close_external: SESSIONS, agent_review_start: SESSIONS,
  center_answer_agent: SESSIONS, answer_approval: SESSIONS, answer_group_approval: SESSIONS, answer_work: SESSIONS,
  center_dismiss_updates: SESSIONS, center_stop_worker: SESSIONS, center_update_task: SESSIONS,
  cancel_run: SESSIONS, delete_session: SESSIONS, new_chat: SESSIONS, pin_session: SESSIONS,
  rename_session: SESSIONS, rerun_prompt: SESSIONS, stop_research: SESSIONS, send_to_group: SESSIONS,
  session_move: SESSIONS, session_move_cancel: SESSIONS,
  terminal_open: ["center_home", "list_machines"], terminal_start: ["center_home", "list_machines"],
  terminal_close: ["center_home", "list_machines"],
  agent_file_save: ["project_sync"], agent_tree_edit: ["project_sync"], agent_tree_upload: ["project_sync"],
  agent_files_sync: ["project_sync"], project_sync_pause: ["project_sync"], project_sync_resolve: ["project_sync"],
  save_file: [], save_picture: [], save_machine_log: [],
  set_zoom_level: [], set_send_on_pause: [], set_notice_voice: [], set_agent_notices: [], set_autostart: [], choose_save_folder: [], use_downloads: [],
  set_machine_search: [],
  set_mac_control: ["mac_control"], allow_mac_permission: ["mac_control"],
  pin_project_to_top: ["projects_on_top"],
  save_agent_token: ACCOUNTS, use_cli_login: ACCOUNTS,
  review_dismiss: FORGE, review_fix: FORGE, review_merge: FORGE, review_post: FORGE, review_start: FORGE,
  center_reply_thread: FORGE, center_comment_thread: FORGE, center_track_issue: FORGE,
};

export function writeScope(command: string): WriteScope {
  const commands = WRITES[command];
  if (commands) return commands.length ? { kind: "views", commands } : { kind: "none" };
  return writes(command) ? { kind: "all" } : { kind: "none" };
}

/// A successful write can change several projections before their next snapshot arrives.
export function viewWrite(command: string): boolean {
  return writeScope(command).kind !== "none";
}

function writes(command: string): boolean {
  return /^(?:add_|allow_|answer_|cancel_|close_|complete_|connect_|create_|delete_|disconnect_|edit_|import_|invite_|join_|leave_|login_|new_|pin_|remove_|rename_|rerun_|resume_|save_|send_|set_|sign_|start_|stop_|update_|use_)/.test(command)
    || /^(?:agent_(?:delete|file_save|files_sync|harness_update|resume|review_start|start|steer|stop_process|title_save|tree_edit|tree_upload)|center_(?:answer|comment|dismiss|reply|stop|track|update)|project_(?:add|copy|git_move|sync_pause|sync_resolve)|review_(?:dismiss|fix|merge|post|start)|session_move(?:_cancel)?$|terminal_(?:close|open|start))/.test(command);
}

/// Drops the views a write can change, every view when the write is not known.
export function dropViews(scope: WriteScope): void {
  switch (scope.kind) {
    case "none": return;
    case "all": clearViews(); return;
    case "views": {
      const dropped = new Set<string>(scope.commands);
      for (const [id, snapshot] of pending) if (dropped.has(snapshot.command)) pending.delete(id);

      for (const [id, held] of views) {
        if (!dropped.has(held.command)) continue;
        bytes -= held.bytes;
        views.delete(id);
      }
    }
  }
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/// Optional fields are accepted only at the wire boundary; the cache has explicit states.
function readSnapshot(value: unknown): Snapshot | null {
  if (!record(value) || typeof value.command !== "string" || !COMMANDS.has(value.command)) return null;
  if (value.args !== undefined && !record(value.args)) return null;
  const args = record(value.args) ? value.args : {};
  if (typeof value.error === "string") return { command: value.command, args, result: { kind: "error", error: value.error } };
  return "value" in value ? { command: value.command, args, result: { kind: "value", value: value.value } } : null;
}

function clone(result: CachedView): CachedView {
  return result.kind === "value" ? { kind: "value", value: structuredClone(result.value) } : { kind: "error", error: result.error };
}

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value)
    .filter(([key, item]) => item !== undefined && !(key === "refresh" && item === false))
    .sort(([a], [b]) => a.localeCompare(b)).map(([key, item]) => [key, canonical(item)]));
  return value;
}

function key(command: string, args: Record<string, unknown> = {}) {
  return JSON.stringify([command, canonical(args)]);
}
