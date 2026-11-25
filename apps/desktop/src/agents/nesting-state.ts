// Coding agents that other agents started: where a child sits under its parent
// in a list, and how its card in the parent's log finds its row. No DOM here.
import type { AgentSession } from "./agents-state.ts";

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

/// What names who started a session: its parent link, else its folder.
type Started = Partial<Pick<AgentSession, "parent" | "machine_id" | "cwd">>;

/// The row of the session that started `s`: the one it names, else the assistant chat whose
/// folder holds its own. A thread found in its agent's files, such as one resumed outside
/// Beep, names no parent, and its folder still says whose it is.
export const startedBy = (rows: readonly AgentSession[], s: Started): AgentSession | undefined => starters(rows)(s);

/// `startedBy` for many sessions of the same `rows`, which are read once.
export function starters(rows: readonly AgentSession[]): (s: Started) => AgentSession | undefined {
  const byId = new Map<string, AgentSession>(), chats = new Map<string, AgentSession>();
  const keep = (map: Map<string, AgentSession>, machine: string, name: string, row: AgentSession) => { if (!map.has(`${machine}\n${name}`)) map.set(`${machine}\n${name}`, row); };

  for (const row of rows) {
    keep(byId, row.machine_id, row.id, row);
    if (row.assistant) keep(chats, row.machine_id, row.cwd, row);
  }

  return s => {
    const p = s.parent;
    if (p) return byId.get(`${p.machine_id}\n${p.id}`) ?? (p.key !== "" ? byId.get(`${p.machine_id}\n${p.key}`) : undefined);
    const chat = THREAD_FOLDER.exec(s.cwd ?? "")?.[1];

    return chat ? chats.get(`${s.machine_id}\n${chat}`) : undefined;
  };
}

/// A thread's folder, right under its conversation's (`.beep/assistant/<id>` in the home
/// folder, as the node makes them), which it captures.
const THREAD_FOLDER = /^(.*[\\/]\.beep[\\/]assistant[\\/][^\\/]+)[\\/][^\\/]+$/;

/// The name of the folder of its own that an assistant chat working in `cwd` has, which its skills
/// are picked by; null for a chat elsewhere.
export function chatFolder(cwd: string): string | null {
  return /[\\/]\.beep[\\/]assistant[\\/]([^\\/]+)[\\/]*$/.exec(cwd)?.[1] ?? null;
}

/// `rows` with each child right under its parent, in the order the list had
/// them: a child is not a top-level row. A child whose parent is not listed
/// (another machine is off, the parent was deleted) stays top-level.
export function nest(rows: AgentSession[]): Nested[] {
  const kids = new Map<AgentSession, AgentSession[]>();
  const nested = new Set<AgentSession>();

  const from = starters(rows);

  for (const row of rows) {
    const parent = from(row);
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
