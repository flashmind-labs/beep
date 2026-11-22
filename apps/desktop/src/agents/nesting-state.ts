// Coding agents that other agents started: where a child sits under its parent
// in a list, and how its card in the parent's log finds its row. No DOM here.
import type { AgentSession } from "./agents.ts";

/// A row of the list, and how deep it sits under the session that started it.
export interface Nested {
  row: AgentSession;
  depth: number;
}

/// The deepest a chain of agents starting agents goes, as the node allows.
const MAX_DEPTH = 3;

const same = (a: Pick<AgentSession, "machine_id" | "id">, b: Pick<AgentSession, "machine_id" | "id">) => a.machine_id === b.machine_id && a.id === b.id;

/// The row of the session that started `s`, when the list has it. A parent is
/// listed under the agent's own id, or the id its node gave it before that.
export function parentRow(rows: readonly AgentSession[], s: Pick<AgentSession, "machine_id" | "id">): AgentSession | undefined {
  return startedBy(rows, rows.find(r => same(r, s)) ?? {});
}

/// The row of the session `s` names as the one that started it.
export function startedBy(rows: readonly AgentSession[], s: Partial<Pick<AgentSession, "parent">>): AgentSession | undefined {
  const p = s.parent;
  if (!p) return undefined;
  return rows.find(r => r.machine_id === p.machine_id && (r.id === p.id || (p.key !== "" && r.id === p.key)));
}

/// `rows` with each child right under its parent, in the order the list had
/// them: a child is not a top-level row. A child whose parent is not listed
/// (another machine is off, the parent was deleted) stays top-level.
export function nest(rows: AgentSession[]): Nested[] {
  const kids = new Map<AgentSession, AgentSession[]>();
  const nested = new Set<AgentSession>();

  for (const row of rows) {
    const parent = parentRow(rows, row);
    if (!parent || parent === row) continue;
    kids.set(parent, [...(kids.get(parent) ?? []), row]);
    nested.add(row);
  }

  const out: Nested[] = [];
  const seen = new Set<AgentSession>();
  const add = (row: AgentSession, depth: number) => {
    if (seen.has(row)) return;
    seen.add(row);
    out.push({ row, depth });
    const under = [...(kids.get(row) ?? [])].sort((a, b) => a.started_at - b.started_at);
    for (const kid of under) add(kid, Math.min(depth + 1, MAX_DEPTH));
  };
  for (const row of rows) if (!nested.has(row)) add(row, 0);
  // A loop of parents (none is top-level) still lists every row once.
  for (const row of rows) add(row, 0);
  return out;
}

/// A machine as agents name it: lower case, one word, as `command_name` makes it.
const machineName = (name: string) =>
  name.toLowerCase().replace(/['\u2019]/g, "").split(/[^\p{L}\p{N}]+/u).filter(part => part !== "").join("-");

/// The row a card in a parent's log opens: its `[child]` line gives the id
/// (`<id>@<machine>`) and the id the machine listed it under.
export function childRow(rows: AgentSession[], id: string, key = ""): AgentSession | undefined {
  const at = id.lastIndexOf("@");
  const session = at < 0 ? id : id.slice(0, at), machine = at < 0 ? "" : id.slice(at + 1);
  return rows.find(r => (r.id === session || (key !== "" && r.id === key)) && (!machine || machineName(r.machine_name) === machine));
}

/// `rows` as `nest` made them, split into the trees with a session still
/// going and the trees where every session `ended` says ended. A tree stays
/// whole, so a child never leaves the parent it sits under.
export function splitEnded(rows: Nested[], ended: (row: AgentSession) => boolean): { live: Nested[]; ended: Nested[] } {
  const trees: Nested[][] = [];

  for (const n of rows) {
    if (n.depth === 0 || !trees.length) trees.push([n]);
    else trees[trees.length - 1].push(n);
  }

  const done = (tree: Nested[]) => tree.every(n => ended(n.row));
  return { live: trees.filter(t => !done(t)).flat(), ended: trees.filter(done).flat() };
}
