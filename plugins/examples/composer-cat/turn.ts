// Reads the app's `agent` topic: where a session's turn stands.

/** Where a session's turn stands. `done` is a session that waits for a prompt. */
export type Turn = "running" | "done" | "other";

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Where the session's turn stands, from the `agent` topic. The app's view is
 * `{ Sessions: { Sessions: { sessions: [{ id, state: { status, subagents } }] } } }`.
 * A turn runs while the status is Working or Asking, or while subagents run.
 */
export function readTurn(data: unknown, session: string): Turn | null {
  if (!isRecord(data) || !isRecord(data.Sessions)) {
    return null;
  }
  const reply = data.Sessions.Sessions;
  if (!isRecord(reply) || !Array.isArray(reply.sessions)) {
    return null;
  }

  const info: unknown = reply.sessions.find(
    (item: unknown) => isRecord(item) && item.id === session,
  );
  if (!isRecord(info) || !isRecord(info.state)) {
    return null;
  }

  const { status, subagents } = info.state;
  const waiting = typeof subagents === "number" && subagents > 0;
  if (status === "Working" || status === "Asking") {
    return "running";
  }
  if (status === "Idle") {
    return waiting ? "running" : "done";
  }

  return "other";
}
