// Views a topic update pushes, read from the update as it arrives.

export interface ViewSnapshot {
  command: string;
  args?: Record<string, unknown>;
  value?: unknown;
  error?: string;
}

type Result =
  | { kind: "value"; value: unknown }
  | { kind: "error"; error: string };
interface Snapshot { command: string; args: Record<string, unknown>; result: Result }

/// What a topic update pushed for `command`: its value, or why that view failed. Null when the
/// update does not carry the view. With `args`, only the view read with those arguments.
export type Pushed<T> = { value: T } | { error: string };

export function pushedView<T>(data: unknown, command: string, args?: Record<string, unknown>): Pushed<T> | null {
  if (!record(data) || !Array.isArray(data.views)) return null;

  for (const value of data.views) {
    const snapshot = readSnapshot(value);
    if (snapshot?.command !== command || (args && key(command, snapshot.args) !== key(command, args))) continue;

    return snapshot.result.kind === "error" ? { error: snapshot.result.error } : { value: snapshot.result.value as T };
  }

  return null;
}

/// Every view of `command` an update pushed, each with the arguments it was read with: for a
/// command read per project, machine or session, where the update carries those it follows.
export function pushedEach<T>(data: unknown, command: string): { args: Record<string, unknown>; pushed: Pushed<T> }[] {
  if (!record(data) || !Array.isArray(data.views)) return [];

  return data.views.flatMap(value => {
    const snapshot = readSnapshot(value);
    if (snapshot?.command !== command) return [];

    return [{ args: snapshot.args, pushed: snapshot.result.kind === "error" ? { error: snapshot.result.error } : { value: snapshot.result.value as T } }];
  });
}

/// The list a topic update pushed for `command`: the
/// error text when that view failed, null when the update does not carry the view.
export function pushedList<T>(data: unknown, command: string): { list: T[] } | { error: string } | null {
  const pushed = pushedView<unknown>(data, command);
  if (!pushed) return null;
  if ("error" in pushed) return pushed;

  return Array.isArray(pushed.value) ? { list: pushed.value as T[] } : null;
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/// Optional fields are accepted only at the wire boundary; a view is a value or an error.
function readSnapshot(value: unknown): Snapshot | null {
  if (!record(value) || typeof value.command !== "string") return null;
  if (value.args !== undefined && !record(value.args)) return null;
  const args = record(value.args) ? value.args : {};
  if (typeof value.error === "string") return { command: value.command, args, result: { kind: "error", error: value.error } };
  return "value" in value ? { command: value.command, args, result: { kind: "value", value: value.value } } : null;
}

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value)
    .filter(([, item]) => item !== undefined)
    .sort(([a], [b]) => a.localeCompare(b)).map(([key, item]) => [key, canonical(item)]));
  return value;
}

function key(command: string, args: Record<string, unknown> = {}) {
  return JSON.stringify([command, canonical(args)]);
}
