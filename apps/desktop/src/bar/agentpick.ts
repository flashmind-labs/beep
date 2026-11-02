// The start form's choices, as plain data: which harnesses the picked
// machine offers, the models and thinking levels each takes, the pill's
// text, the Machine menu's rows and the Folder menu's checkout rule.

import type { AgentModels } from "./agentmodels.ts";

/// The harnesses the picker's rail shows, in this order; others a machine
/// offers (such as OpenCode) follow.
export const RAIL = ["claude", "codex", "gemini", "pi", "flsh"];

/// A rail badge: the harness, and whether the picked machine has it.
export interface RailItem {
  agent: string;
  enabled: boolean;
}

/// The rail for `offered`, the harnesses the picked machine has (any
/// machine's on Auto), with `everywhere` the harnesses any machine has.
export function railOf(offered: readonly string[], everywhere: readonly string[]): RailItem[] {
  const extra = everywhere.filter(a => !RAIL.includes(a));
  return [...RAIL, ...extra].map(agent => ({ agent, enabled: offered.includes(agent) }));
}

/// The harness the form starts on: the one picked when the machine has
/// it, else the first the machine has in `order`, else none.
export function harnessFor(picked: string | null, offered: readonly string[], order: readonly string[]): string | null {
  if (picked && offered.includes(picked)) return picked;
  return [...offered].sort((a, b) => (order.indexOf(a) + 1 || 99) - (order.indexOf(b) + 1 || 99))[0] ?? null;
}

/// A thinking level: what the start sends, and what the picker shows.
export interface Effort {
  value: string;
  label: string;
}

const level = (value: string, label: string): Effort => ({ value, label });
const OFF = level("off", "Off"), MINIMAL = level("minimal", "Minimal"), LOW = level("low", "Low"),
  MEDIUM = level("medium", "Medium"), HIGH = level("high", "High"), XHIGH = level("xhigh", "Extra high"), MAX = level("max", "Max");

/// The levels each harness takes, as the server's `efforts` lists them:
/// Claude Code's `--effort`, Codex's `model_reasoning_effort` and pi's
/// `--thinking`. The rest have none.
const EFFORTS: Record<string, readonly Effort[]> = {
  claude: [LOW, MEDIUM, HIGH, XHIGH, MAX],
  codex: [LOW, MEDIUM, HIGH],
  pi: [OFF, MINIMAL, LOW, MEDIUM, HIGH, XHIGH, MAX],
};

export const effortsOf = (agent: string | null): readonly Effort[] => (agent && EFFORTS[agent]) || [];

/// What the Thinking column says for a harness without levels.
export function effortNote(agent: string, name: string): string {
  if (agent === "flsh") return "flsh sets thinking in its own config.";
  return `${name} has no thinking setting Beep can pass.`;
}

/// The level kept when the harness changes: the same one when the new
/// harness takes it, else its own default.
export function keepEffort(agent: string | null, effort: string | null | undefined): string | null {
  return effort && effortsOf(agent).some(e => e.value === effort) ? effort : null;
}

/// The pill's text after the harness's name: the model, then the thinking
/// level when one is picked.
export function pillDetail(modelLabel: string, agent: string | null, effort: string | null | undefined): string {
  const think = effortsOf(agent).find(e => e.value === effort)?.label;
  return [modelLabel || "Default", think].filter(Boolean).join(" · ");
}

/// A row of the model column.
export interface ModelRow {
  /// What the start sends; empty for the harness's own default.
  value: string;
  label: string;
  note: string;
}

/// The model column for `list` (`undefined` while the machine is asked,
/// `null` when it cannot say), filtered by `query`. A harness that names
/// models freely gets the query itself as a row to pick.
export function modelRows(list: AgentModels | null | undefined, query: string): { rows: ModelRow[]; note: string } {
  const q = query.trim(), lower = q.toLowerCase();
  const name = (id: string) => list?.models.find(m => m.id === id)?.name || id;
  const first: ModelRow = { value: "", label: "Default", note: list?.current ? name(list.current) : "its own" };
  const fits = (r: ModelRow) => !lower || r.label.toLowerCase().includes(lower) || r.value.toLowerCase().includes(lower);
  if (list === undefined) return { rows: [first], note: "Asking the machine which models it has…" };
  if (!list || list.pick === "fixed") return { rows: [first], note: "It runs its own model here." };
  if (list.pick === "named" || !list.models.length) {
    return { rows: q ? [first, { value: q, label: q, note: "use this name" }] : [first], note: q ? "" : "Type a model name to use it." };
  }
  const rows = [first, ...list.models.map(m => ({ value: m.id, label: m.name || m.id, note: m.name && m.name !== m.id ? m.id : "" }))].filter(fits);
  return { rows, note: rows.length ? "" : "No model matches." };
}

/// The model's name for the pill: Default, or its name in `list`.
export function modelLabel(list: AgentModels | null | undefined, model: string | null | undefined): string {
  if (!model) return "Default";
  return list?.models.find(m => m.id === model)?.name || model;
}

/// A machine as the Machine menu lists it.
export interface MachineRow {
  id: string;
  name: string;
  online: boolean;
  /// What it offers, or why it cannot be picked.
  note: string;
  enabled: boolean;
}

/// The Machine menu after Auto: each machine that can run an agent, then
/// online ones without any, then offline ones, each by name. `known` are
/// all the user's machines, when read.
export function machineRows(
  hosts: readonly { machine_id: string; machine_name: string; agents: string[] }[],
  known: readonly { id: string; name: string; online: boolean }[] | null,
  title: (agent: string) => string,
  order: readonly string[],
): MachineRow[] {
  const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name);
  const rank = (a: string) => order.indexOf(a) + 1 || 99;
  const able = hosts.map(m => ({
    id: m.machine_id,
    name: m.machine_name,
    online: true,
    note: [...m.agents].sort((a, b) => rank(a) - rank(b)).map(title).join(", "),
    enabled: true,
  })).sort(byName);
  const ids = new Set(able.map(m => m.id));
  const rest = (known ?? []).filter(m => !ids.has(m.id)).map(m => ({
    id: m.id,
    name: m.name,
    online: m.online,
    note: m.online ? "no coding agent" : "offline",
    enabled: false,
  })).sort((a, b) => Number(b.online) - Number(a.online) || byName(a, b));
  return [...able, ...rest];
}

/// Which checkout a session starts in: Auto, the checkout itself, a new
/// worktree branched from the prompt, or an existing worktree by folder.
export type CheckoutPick = "auto" | "current" | "new" | { worktree: string };

/// What Auto resolves to: the checkout, unless a session of ours already
/// runs in it on that machine, then a new worktree. A pick other than
/// Auto stands.
export function checkoutFor(pick: CheckoutPick, busy: boolean): Exclude<CheckoutPick, "auto"> {
  if (pick !== "auto") return pick;
  return busy ? "new" : "current";
}

/// The session fields the checkout rule reads.
export interface SessionAt {
  machine_id: string;
  cwd: string;
  status: string;
  external: boolean;
}

const RUNNING = new Set(["asking", "working", "running", "starting", "waiting", "idle"]);
const trimmed = (p: string) => p.replace(/\/+$/, "");

/// Whether one of our sessions (not one started outside Beep) is open in
/// `cwd` on `machine`.
export function folderBusy(sessions: readonly SessionAt[], machine: string | null, cwd: string): boolean {
  if (!machine || !cwd.trim()) return false;
  const at = trimmed(cwd.trim());
  return sessions.some(s => s.machine_id === machine && !s.external && RUNNING.has(s.status) && trimmed(s.cwd) === at);
}

/// The checkout's worktrees on `machine` that sessions ran in, newest
/// first: folders under `<checkout>/.worktrees/`.
export function knownWorktrees(sessions: readonly (SessionAt & { last_activity?: number })[], machine: string | null, checkout: string): string[] {
  if (!machine || !checkout.trim()) return [];
  const root = `${trimmed(checkout.trim())}/.worktrees/`;
  const found = sessions
    .filter(s => s.machine_id === machine && s.cwd.startsWith(root))
    .sort((a, b) => (b.last_activity ?? 0) - (a.last_activity ?? 0))
    .map(s => root + s.cwd.slice(root.length).split("/")[0])
    .filter(p => p.length > root.length);
  return [...new Set(found)];
}

/// What the checkout row of the Folder button says.
export function checkoutLabel(pick: CheckoutPick, busy: boolean): string {
  const on = checkoutFor(pick, busy);
  const what = on === "current" ? "Current checkout" : on === "new" ? "New worktree" : `Worktree ${on.worktree.split("/").pop()}`;
  return pick === "auto" ? `Auto: ${what}` : what;
}
