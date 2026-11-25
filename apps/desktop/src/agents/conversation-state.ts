import type { AgentRow, OpenAgent } from "./agents.ts";
import { isNativeSession } from "./native-session-state.ts";

type SessionId = Pick<AgentRow, "machine_id" | "agent" | "id">;

/// A session as a conversation: the key of the picker and of a field's draft.
export const conversationKey = (s: SessionId) => JSON.stringify([s.machine_id, s.agent, s.id]);

/// A session of a harness, as the maps of moves, usage and continuing key it.
export const agentKey = (s: SessionId) => `${s.machine_id}\n${s.agent}\n${s.id}`;

/// A session by machine and id alone, for what is kept per session.
export const sessionKey = (s: Pick<AgentRow, "machine_id" | "id">) => `${s.machine_id}\n${s.id}`;

/// Visits change when a conversation opens, never while the picker cycles.
export class ConversationHistory {
  private visits: string[] = [];
  visit(s: AgentRow) {
    const key = conversationKey(s);
    this.visits = [key, ...this.visits.filter(k => k !== key)].slice(0, 100);
  }
  order(rows: readonly AgentRow[], current: AgentRow | null): AgentRow[] {
    const unique = new Map<string, AgentRow>();

    for (const row of rows) {
      unique.set(conversationKey(row), row);
    }

    const rank = (s: AgentRow) => {
      const key = conversationKey(s);
      if (current && key === conversationKey(current)) return -1;
      const i = this.visits.indexOf(key);
      return i < 0 ? Infinity : i;
    };
    return [...unique.values()].filter(row => !row.parent && !isNativeSession(row.id) && row.status !== "ended" && row.status !== "closing").sort((a, b) => rank(a) - rank(b) || (b.last_activity ?? b.started_at ?? 0) - (a.last_activity ?? a.started_at ?? 0));
  }
}
export const conversationHistory = new ConversationHistory();

/// The title a session shows after it changed, here or on another app: null when it has none.
export interface Retitled extends OpenAgent { title: string | null }

/// Session `s` under the title `change` shows for it, or null when `change` is for another
/// session.
export function retitle(s: AgentRow, change: Retitled): AgentRow | null {
  const key = conversationKey({ machine_id: change.machine_id, agent: change.agent, id: change.session });
  if (conversationKey(s) !== key) return null;
  return { ...s, title: change.title };
}

/// `rows` with the title `change` shows on its session, or null when no row is that session or
/// it shows that title already.
export function retitled<T extends SessionId & { title: string | null }>(rows: readonly T[], change: Retitled): T[] | null {
  const key = conversationKey({ machine_id: change.machine_id, agent: change.agent, id: change.session });
  if (!rows.some(row => conversationKey(row) === key && row.title !== change.title)) return null;
  return rows.map(row => (conversationKey(row) === key ? { ...row, title: change.title } : row));
}

/// A row of the switcher, and how deep it sits under the session that started it.
export interface ConversationChoice {
  row: AgentRow;
  depth: number;
}

/// The deepest a row is drawn under its parent.
const MAX_DEPTH = 3;

/// `rows` in their order, each session started by another right under its
/// parent, in the order they started. A parent `parentOf` knows that is not
/// listed (it ended, or runs elsewhere) joins as the head of its group.
export function groupConversations(rows: readonly AgentRow[], parentOf: (row: AgentRow) => AgentRow | null): ConversationChoice[] {
  const known = new Map(rows.map(row => [conversationKey(row), row]));
  const up = new Map<string, string>();
  const queue = [...known.values()];

  for (const row of queue) {
    const key = conversationKey(row), parent = parentOf(row);
    if (!parent || up.has(key)) continue;
    const parentKey = conversationKey(parent);
    if (parentKey === key) continue;
    up.set(key, parentKey);

    if (!known.has(parentKey)) {
      known.set(parentKey, parent);
      queue.push(parent);
    }
  }

  const kids = new Map<string, AgentRow[]>();

  for (const [key, row] of known) {
    const parent = up.get(key);
    if (parent !== undefined) kids.set(parent, [...(kids.get(parent) ?? []), row]);
  }

  // A loop of parents ends at the first row seen twice.
  const root = (key: string) => {
    const seen = new Set([key]);
    let at = key;

    for (let parent = up.get(at); parent !== undefined && !seen.has(parent); parent = up.get(at)) {
      seen.add(parent);
      at = parent;
    }

    return at;
  };

  const out: ConversationChoice[] = [];
  const placed = new Set<string>();
  const place = (row: AgentRow, depth: number) => {
    const key = conversationKey(row);
    if (placed.has(key)) return;
    placed.add(key);
    out.push({ row, depth: Math.min(depth, MAX_DEPTH) });
    const under = [...(kids.get(key) ?? [])].sort((a, b) => (a.started_at ?? 0) - (b.started_at ?? 0));
    for (const kid of under) place(kid, depth + 1);
  };

  for (const row of rows) {
    const top = known.get(root(conversationKey(row)));
    if (top) place(top, 0);
  }

  return out;
}

/// The state a row shows: a subagent the harness runs is done once idle.
export function conversationState(row: AgentRow): string {
  if (isNativeSession(row.id) && (row.status === "idle" || row.status === "ended")) return "Done";

  switch (row.status) {
    case "asking": return "Needs you";
    case "idle": return "Ready";
    case "waiting": return "Waiting";
    case "starting": return "Starting";
    case "ended": return "Ended";
    default: return "Working";
  }
}

/// The first press from the current conversation passes over the rows under
/// it, so a quick Option+Tab still returns to the one used before.
export function firstConversation(choices: readonly ConversationChoice[], index: number, backwards: boolean) {
  const next = nextConversation(index, choices.length, backwards);
  if (backwards || index < 0 || index >= choices.length) return next;
  const depth = choices[index].depth;
  const after = choices.findIndex((c, i) => i > index && c.depth <= depth);
  return after < 0 ? next : after;
}

export function nextConversation(index: number, count: number, backwards: boolean) {
  if (!count) return -1;
  if (index < 0) return backwards ? count - 1 : 0;
  return (index + (backwards ? -1 : 1) + count) % count;
}
export function isConversationKey(e: Pick<KeyboardEvent, "key" | "altKey" | "ctrlKey" | "metaKey" | "isComposing">) {
  return e.key === "Tab" && e.altKey && !e.ctrlKey && !e.metaKey && !e.isComposing;
}

export const openOf = (row: Pick<AgentRow, "machine_id" | "agent" | "id">): OpenAgent => ({ machine_id: row.machine_id, agent: row.agent, session: row.id });
/// The session `o` names, as the node's commands take it.
export const where = (o: OpenAgent) => ({ machineId: o.machine_id, agent: o.agent, session: o.session });
