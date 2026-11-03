// The start form's choices, as plain data: which harnesses the picked
// machine offers, the models and thinking levels each takes, the pill's
// text, the Machine menu's rows, the Project and Folder menus and where
// the picker goes on screen.

import type { AgentModels, ModelInfo } from "./agentmodels.ts";
import { folderOn } from "../projects-state.ts";

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

/// The levels each harness takes, as the server's `efforts` lists them: Claude
/// Code's `--effort`, Codex's `model_reasoning_effort` and pi's `--thinking`.
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

/// The model column for `list` (`undefined` while the machine is asked, `null`
/// when it cannot say), filtered by `query`.
export function modelRows(list: AgentModels | null | undefined, query: string): { rows: ModelRow[]; note: string } {
  const q = query.trim(), lower = q.toLowerCase();
  // The harness's own "default" entry is our Default row: one row, whose
  // note says what it resolves to.
  const isDefault = (m: ModelInfo) => m.id.toLowerCase() === "default" || /^default\b/i.test(m.name);
  const own = list?.models.find(isDefault), current = list?.models.find(m => m.id === list.current);
  const resolves = own?.description ? own.description.split(/ · | - |\. |, /)[0].trim()
    : current && !isDefault(current) ? current.name || current.id
    : !own && list?.current ? list.current : "its own";
  const first: ModelRow = { value: "", label: "Default", note: resolves };
  const fits = (r: ModelRow) => !lower || r.label.toLowerCase().includes(lower) || r.value.toLowerCase().includes(lower);
  if (list === undefined) return { rows: [first], note: "Asking the machine which models it has…" };
  if (!list || list.pick === "fixed") return { rows: [first], note: "It runs its own model here." };
  if (list.pick === "named" || !list.models.length) {
    return { rows: q ? [first, { value: q, label: q, note: "use this name" }] : [first], note: q ? "" : "Type a model name to use it." };
  }
  const rows = [first, ...list.models.filter(m => !isDefault(m)).map(m => ({ value: m.id, label: m.name || m.id, note: m.name && m.name !== m.id ? m.id : "" }))].filter(fits);
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

/// The Machine menu after Auto: each machine that can run an agent, then online
/// ones without any, then offline ones, each by name.
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

/// Which checkout a session starts in: the checkout as it is, a new branch
/// in the checkout, a new worktree on a new branch (both named from the
/// prompt), or an existing worktree by folder.
export type CheckoutPick = "current" | "branch" | "new" | { worktree: string };

/// What a start picks when the user picks nothing: the checkout as it is.
export const DEFAULT_CHECKOUT: CheckoutPick = "current";

/// The session fields the worktree list reads.
export interface SessionAt {
  machine_id: string;
  cwd: string;
}

const trimmed = (p: string) => p.replace(/\/+$/, "");

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
export function checkoutLabel(pick: CheckoutPick): string {
  if (typeof pick === "object") return `Worktree ${pick.worktree.split("/").pop()}`;
  return pick === "current" ? "Current checkout" : pick === "branch" ? "New branch" : "New worktree";
}

/// What `agent_start` sends as `checkout`: the node makes a `beep/<slug>`
/// branch from `base` (the checkout's HEAD when null), in the checkout or
/// in a new worktree under `.worktrees/`.
export type CheckoutPlan = { new_branch: { base: string | null } } | { new_worktree: { base: string | null } };

/// Whether the pick makes a new branch, so the form asks for its base.
export function makesBranch(pick: CheckoutPick): boolean {
  return pick === "branch" || pick === "new";
}

/// The folder and checkout plan a start sends for `pick` in the checkout
/// `cwd`: an existing worktree is a folder to start in, a new branch or
/// worktree is a plan the node carries out in `cwd`, and the checkout
/// sends no plan, so nothing is made.
export function startPlace(pick: CheckoutPick, cwd: string, base: string | null): { cwd: string; checkout: CheckoutPlan | null } {
  const from = base?.trim() || null;
  if (typeof pick === "object") return { cwd: pick.worktree, checkout: null };
  if (pick === "branch") return { cwd, checkout: { new_branch: { base: from } } };
  if (pick === "new") return { cwd, checkout: { new_worktree: { base: from } } };
  return { cwd, checkout: null };
}

/// A project's branches as `project_branches` reads them, the fields the
/// Base menu needs.
export interface BaseBranches {
  default_branch: string;
  branches: readonly { name: string; local: boolean; committed_at: number }[];
}

/// The most branches the Base menu lists.
export const MAX_BASES = 30;

/// The Base menu's branches: the default branch first, then the others
/// matching `query`, newest commit first, at most `MAX_BASES`.
export function baseRows(list: BaseBranches | null | undefined, query = ""): string[] {
  const q = query.trim().toLowerCase();
  const rest = [...(list?.branches ?? [])]
    .filter(b => b.name !== list?.default_branch)
    .sort((a, b) => b.committed_at - a.committed_at)
    .map(b => b.name);
  const all = [...new Set([list?.default_branch || "", ...rest])].filter(Boolean);
  return all.filter(name => !q || name.toLowerCase().includes(q)).slice(0, MAX_BASES);
}

/// The base a start sends: the one picked, else the project's default
/// branch, else none (the checkout's HEAD).
export function baseFor(picked: string | null | undefined, list: BaseBranches | null | undefined): string | null {
  return picked?.trim() || list?.default_branch || null;
}

// ----- Project and folder -----

/// `path` as the form shows it: under `home` (the machine's home folder,
/// when known) written `~/...`, else any `/home/<user>` or `/Users/<user>`
/// folder the same way.
export function tildePath(path: string, home: string | null | undefined): string {
  const p = trimmed(path.trim());
  if (!p) return "";
  const h = home ? trimmed(home.trim()) : "";
  if (h && p === h) return "~";
  if (h && p.startsWith(`${h}/`)) return `~/${p.slice(h.length + 1)}`;
  return folderOn(p, null, null);
}

/// A project as the form offers it: its name and its checkout on each
/// machine.
export interface FormProject {
  name: string;
  places: { machine: string; cwd: string }[];
}

/// The projects in `rows` (one row per project and machine, as Home's
/// imported projects come), each with its checkouts, by name.
export function formProjects(rows: readonly { name: string; machine_id: string; folder: string }[] | null | undefined): FormProject[] {
  const by = new Map<string, FormProject>();
  for (const r of rows ?? []) {
    if (!r.folder) continue;
    const p = by.get(r.name) ?? { name: r.name, places: [] };
    if (!p.places.some(x => x.machine === r.machine_id)) p.places.push({ machine: r.machine_id, cwd: r.folder });
    by.set(r.name, p);
  }
  return [...by.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/// A project's folder in the Folder menu: the folder a pick sets, and how
/// the row shows it.
export interface ProjectFolder {
  project: string;
  cwd: string;
  shown: string;
}

/// The projects checked out on `machine`, each at its folder there; on
/// Auto (`machine` null) each at its first folder written under `~`, which
/// each machine reads as its own home.
export function projectFolders(projects: readonly FormProject[], machine: string | null, home: string | null | undefined): ProjectFolder[] {
  return projects.flatMap(p => {
    const at = machine ? p.places.find(x => x.machine === machine) : p.places[0];
    if (!at) return [];
    const cwd = machine ? at.cwd : folderOn(at.cwd, null, null);
    return [{ project: p.name, cwd, shown: tildePath(cwd, machine ? home : null) }];
  });
}

/// The project whose checkout `cwd` is on `machine` (any machine on
/// Auto), if any.
export function projectAt(projects: readonly FormProject[], machine: string | null, cwd: string): FormProject | null {
  const at = trimmed(cwd.trim());
  if (!at) return null;
  const tilde = folderOn(at, null, null);
  return projects.find(p => p.places.some(x => (!machine || x.machine === machine) && (trimmed(x.cwd) === at || folderOn(x.cwd, null, null) === tilde))) ?? null;
}

/// The heading: the project's short name in bold, when there is one.
export function formTitle(project: string | null | undefined): { lead: string; name: string | null } {
  const name = project?.split("/").filter(Boolean).pop() ?? null;
  return name ? { lead: "What should we build in", name } : { lead: "What should we build?", name: null };
}

/// A row of the Folder menu outside a project.
export type FolderRow =
  | { kind: "workspace"; shown: string; on: boolean }
  | { kind: "project"; project: string; cwd: string; shown: string; on: boolean }
  | { kind: "type" }
  | { kind: "rule" };

/// The Folder menu outside a project: Workspace, the projects checked out
/// on the machine, then a row to type any folder.
export function folderRows(cwd: string, workspace: string | null | undefined, home: string | null | undefined, folders: readonly ProjectFolder[]): FolderRow[] {
  const at = trimmed(cwd.trim());
  const rule: FolderRow = { kind: "rule" };
  return [
    { kind: "workspace", shown: workspace?.trim() ? tildePath(workspace, home) : "~", on: !at },
    ...(folders.length ? [rule] : []),
    ...folders.map(f => ({ kind: "project" as const, project: f.project, cwd: f.cwd, shown: f.shown, on: trimmed(f.cwd) === at })),
    rule,
    { kind: "type" },
  ];
}

/// What the Folder pill says outside a project: "Workspace" and its path,
/// or the folder picked.
export function folderPill(cwd: string, workspace: string | null | undefined, home: string | null | undefined): { label: string; path: string } {
  if (cwd.trim()) return { label: "", path: tildePath(cwd, home) };
  return { label: "Workspace", path: workspace?.trim() ? tildePath(workspace, home) : "~" };
}

// ----- Picker placement -----

/// A box on screen, in CSS pixels.
export interface Rect {
  top: number;
  left: number;
  width: number;
  height: number;
}

/// Where a popover goes, in viewport pixels; `maxHeight` is set when it
/// does not fit whole on either side.
export interface Placement {
  top: number;
  left: number;
  width: number;
  maxHeight: number | null;
}

/// Places a popover of `size` by `anchor` inside a `view` of that size: above
/// it with right edges lined up when it fits there, else below, else on the
/// roomier side cut to fit.
export function placeMenuAt(anchor: Rect, size: { width: number; height: number }, area: { top: number; bottom: number; width: number }, margin = 8, gap = 4) {
  const below = area.bottom - margin - (anchor.top + anchor.height + gap);
  const above = anchor.top - gap - (area.top + margin);
  const up = size.height > below && above > below;
  const room = Math.max(0, up ? above : below);
  const height = Math.min(size.height, room);
  const top = up ? anchor.top - gap - height : anchor.top + anchor.height + gap;
  const left = Math.max(margin, Math.min(anchor.left, area.width - margin - size.width));
  return { top, left, maxHeight: room, up };
}

export function placePopover(anchor: Rect, size: { width: number; height: number }, view: { width: number; height: number }, margin = 8, gap = 6): Placement {
  const width = Math.min(size.width, Math.max(0, view.width - 2 * margin));
  const left = Math.max(margin, Math.min(anchor.left + anchor.width - width, view.width - margin - width));
  const above = anchor.top - gap - margin;
  const below = view.height - (anchor.top + anchor.height) - gap - margin;
  const under = Math.min(view.height - margin, anchor.top + anchor.height + gap);
  const over = (h: number) => Math.max(margin, anchor.top - gap - h);
  if (size.height <= above) return { top: over(size.height), left, width, maxHeight: null };
  if (size.height <= below) return { top: under, left, width, maxHeight: null };
  if (above >= below) {
    const h = Math.max(0, above);
    return { top: over(h), left, width, maxHeight: h };
  }
  return { top: under, left, width, maxHeight: Math.max(0, below) };
}
