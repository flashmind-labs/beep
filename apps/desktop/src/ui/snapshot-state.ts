// Current rendered views received from their source, bounded to this visible window.

export interface ViewSnapshot {
  command: string;
  args?: Record<string, unknown>;
  value?: unknown;
  error?: string;
}

const COMMANDS = new Set([
  "center_home", "list_machines", "mac_control", "projects_on_top", "project_groups",
  "project_sync", "git_items", "usage_get", "agent_accounts", "center_project_lists",
  "center_project_runs", "center_project_run", "center_project_issues", "center_project_pull_requests",
]);
const MAX_VIEWS = 128;
const MAX_BYTES = 8 * 1024 * 1024;
export type CachedView =
  | { kind: "value"; value: unknown }
  | { kind: "error"; error: string };
interface Snapshot { command: string; args: Record<string, unknown>; result: CachedView }
const views = new Map<string, CachedView & { bytes: number }>();
let bytes = 0;

/// Install every projection before notifying any subscriber of the source update.
export function installViews(data: unknown): void {
  if (!record(data) || !Array.isArray(data.views)) return;
  for (const value of data.views) {
    const snapshot = readSnapshot(value);
    if (!snapshot) continue;
    const { result } = snapshot;
    const id = key(snapshot.command, snapshot.args);
    const size = JSON.stringify(result.kind === "error" ? result.error : result.value)?.length ?? MAX_BYTES + 1;
    const previous = views.get(id);
    if (previous) { bytes -= previous.bytes; views.delete(id); }
    if (size > MAX_BYTES) continue;
    views.set(id, { ...clone(result), bytes: size });
    bytes += size;
    while (views.size > MAX_VIEWS || bytes > MAX_BYTES) {
      const oldest = views.entries().next().value;
      if (!oldest) break;
      bytes -= oldest[1].bytes;
      views.delete(oldest[0]);
    }
  }
}

/// Explicit refreshes read the source. Callers own their copy of each rendered view.
export function snapshotView(command: string, args?: Record<string, unknown>): CachedView | null {
  if (!COMMANDS.has(command) || args?.refresh === true) return null;
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
  bytes = 0;
}

/// A successful write can change several projections before their next snapshot arrives.
export function viewWrite(command: string): boolean {
  return /^(?:add_|allow_|answer_|cancel_|close_|complete_|connect_|create_|delete_|disconnect_|edit_|import_|invite_|join_|leave_|login_|new_|pin_|remove_|rename_|rerun_|resume_|save_|send_|set_|sign_|start_|stop_|update_|use_)/.test(command)
    || /^(?:agent_(?:delete|file_save|files_sync|harness_update|resume|review_start|start|steer|stop_process|title_save|tree_edit|tree_upload)|center_(?:answer|comment|dismiss|reply|stop|track|update)|project_(?:add|copy|git_move|sync_pause|sync_resolve)|review_(?:dismiss|fix|merge|post|start)|session_move|terminal_(?:close|open|start))/.test(command);
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
