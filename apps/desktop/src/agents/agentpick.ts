// The start form's choices, as plain data: which harnesses the picked
// machine offers, the models and thinking levels each takes, the pill's
// text, the Machine menu's rows, the Project and Folder menus and where
// the picker goes on screen.

import type { AgentModels, ModelInfo } from "./agentmodels.ts";
import { folderOn } from "../project/projects-state.ts";

/// The harnesses the picker's rail shows, in this order; others a machine
/// offers (such as OpenCode) follow.
const RAIL = ["claude", "codex", "agy", "pi", "flsh"];

/// A rail badge: the harness, and whether the picked machine has it.
interface RailItem {
  agent: string;
  enabled: boolean;
}

/// The rail for `offered`, the harnesses the picked machine has (any
/// machine's before one is picked), with `everywhere` the harnesses any machine has.
/// Harnesses the project does not allow are left out.
export function railOf(offered: readonly string[], everywhere: readonly string[], allowed: Allowed): RailItem[] {
  const extra = everywhere.filter(a => !RAIL.includes(a));
  return allowedOf([...RAIL, ...extra], allowed).map(agent => ({ agent, enabled: offered.includes(agent) }));
}

/// The harnesses a project allows; null for every one.
export type Allowed = readonly string[] | null;

/// Those of `agents` that `allowed` lets run.
export const allowedOf = (agents: readonly string[], allowed: Allowed) => allowed ? agents.filter(agent => allowed.includes(agent)) : [...agents];

/// The harnesses machine `machine` offers, in the order the pickers list them;
/// before one is named, those any machine offers. Only those `allowed` lets run.
export function harnessesOn(hosts: readonly { machine_id: string; agents: readonly string[] }[] | null, machine: string | null, allowed: Allowed) {
  const host = machine ? hosts?.find(m => m.machine_id === machine) : null;
  const all = host ? host.agents : [...new Set((hosts ?? []).flatMap(m => m.agents))];
  return allowedOf(all, allowed).sort(byPreference);
}

/// Sorts harnesses by their place in `order`; those it lacks go last.
export function byOrder(order: readonly string[]) {
  const rank = (a: string) => order.indexOf(a) + 1 || 99;
  return (a: string, b: string) => rank(a) - rank(b);
}

/// The harnesses in the order a form prefers them.
export const AGENT_ORDER = ["claude", "codex", "agy", "opencode", "pi", "flsh", "hermes"];
export const byPreference = byOrder(AGENT_ORDER);

/// The harness the form starts on: the one picked when the machine has
/// it, else the first the machine has in `order`, else none.
export function harnessFor(picked: string | null, offered: readonly string[], order: readonly string[]): string | null {
  if (picked && offered.includes(picked)) return picked;
  return [...offered].sort(byOrder(order))[0] ?? null;
}

/// A thinking level: what the start sends, and what the picker shows.
interface Effort {
  value: string;
  label: string;
}

const level = (value: string, label: string): Effort => ({ value, label });
const NONE = level("none", "None"), ULTRA = level("ultra", "Ultra");
const OFF = level("off", "Off"), MINIMAL = level("minimal", "Minimal"), LOW = level("low", "Low"),
  MEDIUM = level("medium", "Medium"), HIGH = level("high", "High"), XHIGH = level("xhigh", "Extra high"), MAX = level("max", "Max");

/// The levels each harness takes, as the server's `efforts` lists them: Claude
/// Code's `--effort`, Codex's `model_reasoning_effort`, Antigravity's
/// `--effort`, pi's `--thinking` and flsh's `thought_level` (0.2.7 and later).
const EFFORTS: Record<string, readonly Effort[]> = {
  claude: [LOW, MEDIUM, HIGH, XHIGH, MAX],
  codex: [NONE, MINIMAL, LOW, MEDIUM, HIGH, XHIGH, MAX, ULTRA],
  agy: [LOW, MEDIUM, HIGH, XHIGH, MAX],
  pi: [OFF, MINIMAL, LOW, MEDIUM, HIGH, XHIGH, MAX],
  flsh: [OFF, LOW, MEDIUM, HIGH],
};

export const effortsOf = (agent: string | null): readonly Effort[] => (agent && EFFORTS[agent]) || [];

/// What the Thinking column says for a harness without levels.
export function effortNote(name: string): string {
  return `${name} has no thinking setting Beep can pass.`;
}

/// The level kept when the harness changes: the same one when the new
/// harness takes it, else its own default.
export function keepEffort(agent: string | null, effort: string | null | undefined): string | null {
  return effort && effortsOf(agent).some(e => e.value === effort) ? effort : null;
}

/// The pill's thinking level, when picked, followed by the model.
export function pillDetail(modelLabel: string, agent: string | null, effort: string | null | undefined): string {
  const think = effortsOf(agent).find(e => e.value === effort)?.label;
  return [think, modelLabel || "Default"].filter(Boolean).join(" ");
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
  const q = query.trim(), words = searchText(q).split(/\s+/).filter(Boolean);
  // The harness's own "default" entry is our Default row: one row, whose
  // note says what it resolves to.
  const isDefault = (m: ModelInfo) => m.id.toLowerCase() === "default" || /^default\b/i.test(m.name);
  const own = list?.models.find(isDefault), current = list?.models.find(m => m.id === list.current);
  const resolves = own?.description ? own.description.split(/ · | - |\. |, /)[0].trim()
    : current && !isDefault(current) ? current.name || current.id
    : !own && list?.current ? list.current : "its own";
  const first: ModelRow = { value: "", label: "Default", note: resolves };
  // Every word of the query, in any order, as flashmind-cli matches models: in the
  // name, the id or the provider.
  const fits = (r: ModelRow) => { const text = searchText(`${r.label} ${r.value} ${r.note}`); return words.every(w => text.includes(w)); };
  if (list === undefined) return { rows: [first], note: "Asking the machine which models it has…" };
  if (list === null) return { rows: [first], note: "Couldn't load models." };
  if (list.pick === "fixed") return { rows: [first], note: "It runs its own model here." };

  if (list.pick === "named" || !list.models.length) {
    return { rows: q ? [first, { value: q, label: q, note: "use this name" }] : [first], note: q ? "" : "Type a model name to use it." };
  }

  // The provider is the note: the same name can show under several. The id
  // joins it only to tell apart two models that share a name and provider.
  const twins = (m: ModelInfo) => list.models.some(o => o !== m && o.name === m.name && o.provider === m.provider);
  const note = (m: ModelInfo) => [m.provider ?? "", m.name && m.name !== m.id && twins(m) ? m.id : ""].filter(Boolean).join(" · ");
  const rows = [first, ...list.models.filter(m => !isDefault(m)).map(m => ({ value: m.id, label: m.name || m.id, note: note(m) }))].filter(fits);
  return { rows, note: rows.length ? "" : "No model matches." };
}

/// Lowercase text with `-` and `_` read as `.`, so `5.5` finds `claude-opus-5-5`.
const searchText = (text: string) => text.toLowerCase().replace(/[-_]/g, ".");

/// A picker search split into what it names, as in `opus 5.5 high`,
/// `deepseek v4 h` or `pi sonnet low`: a harness among those offered, and
/// the words a model and its thinking level must match.
export interface ModelSearch {
  agent: string | null;
  words: string;
}

export function modelSearch(query: string, offered: readonly string[]): ModelSearch {
  let agent: string | null = null;
  const words: string[] = [];

  for (const word of query.trim().split(/\s+/).filter(Boolean)) {
    const w = word.toLowerCase();
    if (!agent && offered.includes(w)) agent = w;
    else words.push(word);
  }

  return { agent, words: words.join(" ") };
}

/// A model row, the harness it runs under and the thinking level the search
/// named, if any.
export interface PickRow extends ModelRow {
  agent: string;
  effort: string | null;
}

/// The harnesses a search looks in: the one it names, else every harness
/// offered once it names a model, in rail order so subscription harnesses
/// come first, else the picked one.
export function searchedHarnesses(search: ModelSearch, picked: string, offered: readonly string[]): string[] {
  if (search.agent) return [search.agent];
  const levels = Object.values(EFFORTS).flat();
  const namesModel = wordsOf(search.words).some(w => !levels.some(level => startsLevel(w, level)));
  return namesModel && offered.length > 1 ? [...offered].sort(byOrder(RAIL)) : [picked];
}

/// The rows of several harnesses for a search. Each word matches the model,
/// or the start of a thinking level, so `opus 5.5 h` shows `Opus 5.5 · High`.
/// Models named outright come before a Default that happens to match, so
/// Enter takes the model typed. A harness that lists no models only joins
/// when the search names it.
export function searchRows(lists: readonly { agent: string; list: AgentModels | null | undefined }[], search: ModelSearch): { rows: PickRow[]; note: string } {
  const named = lists.length === 1 || !!search.agent;
  const rows = lists
    .filter(({ list }) => named || list?.pick === "listed")
    .flatMap(({ agent, list }) => readings(search.words, effortsOf(agent)).flatMap(({ effort, rest }) =>
      modelRows(list, rest).rows.map(row => ({
        ...row,
        label: effort ? `${row.label} · ${effort.label}` : row.label,
        agent,
        effort: effort?.value ?? null,
        typed: !!rest,
      }))));
  const sorted = [...rows.filter(r => r.value || !r.typed), ...rows.filter(r => !r.value && r.typed)]
    .map(({ typed: _, ...row }) => row);
  const note = named ? modelRows(lists[0]?.list, sorted.some(r => r.value) ? "" : search.words).note
    : sorted.length ? "" : "No model matches.";
  return { rows: sorted, note };
}

/// The ways to read a search: every word for the model, and for each level
/// that some words start, that level with the other words for the model.
function readings(search: string, levels: readonly Effort[]): { effort: Effort | null; rest: string }[] {
  const all = wordsOf(search);
  const leveled = levels.flatMap(effort => {
    const rest = all.filter(w => !startsLevel(w, effort));
    return rest.length < all.length ? [{ effort, rest: rest.join(" ") }] : [];
  });
  return [{ effort: null, rest: search }, ...leveled];
}

const wordsOf = (text: string) => text.split(/\s+/).filter(Boolean);

/// Whether `word` starts the level's value or label: `h` and `hi` are High,
/// `xhi` and `extra` are Extra high.
const startsLevel = (word: string, level: Effort) => {
  const w = word.toLowerCase();
  return level.value.startsWith(w) || level.label.toLowerCase().startsWith(w);
};

/// Resolve a readable title at the boundary; IDs take precedence over titles.
export function modelId(list: AgentModels | null | undefined, model: string): string {
  const value = model.trim();
  if (!value) return "";

  const exact = list?.models.find(m => m.id === value);
  if (exact) return exact.id;

  // A name under several providers resolves when the text names its provider too,
  // as in "openrouter · DeepSeek V4.1" or "openrouter/DeepSeek V4.1".
  const typed = value.toLowerCase();
  const served = list?.models.find(m => m.provider && m.name && [" · ", "/", ":", " "].some(sep => `${m.provider}${sep}${m.name}`.toLowerCase() === typed));
  if (served) return served.id;

  const matches = list?.models.filter(m => m.name.toLowerCase() === typed) ?? [];
  return matches.length === 1 ? matches[0].id : value;
}

/// The model's name for the pill, resolving the default when known.
export function modelLabel(list: AgentModels | null | undefined, model: string | null | undefined): string {
  if (model && model.toLowerCase() !== "default") return list?.models.find(m => m.id === model)?.name || model;
  const current = list?.current;
  if (current && current.toLowerCase() !== "default") return list?.models.find(m => m.id === current)?.name || current;
  const own = list?.models.find(m => m.id.toLowerCase() === "default" || /^default\b/i.test(m.name));
  return own?.description?.split(/ · | - |\. |, /)[0].trim() || "Default";
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

/// The Machine menu: each machine that can run an agent, then online
/// ones without any, then offline ones, each by name.
export function machineRows(
  hosts: readonly { machine_id: string; machine_name: string; agents: string[] }[],
  known: readonly { id: string; name: string; online: boolean }[] | null,
  title: (agent: string) => string,
  order: readonly string[],
): MachineRow[] {
  const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name);
  const able = hosts.map(m => ({
    id: m.machine_id,
    name: m.machine_name,
    online: true,
    note: [...m.agents].sort(byOrder(order)).map(title).join(", "),
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
interface SessionAt {
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
const MAX_BASES = 30;

/// The Base menu's branches: the default branch first, then the others,
/// newest commit first, at most `MAX_BASES`.
export function baseRows(list: BaseBranches | null | undefined): string[] {
  const rest = [...(list?.branches ?? [])]
    .filter(b => b.name !== list?.default_branch)
    .sort((a, b) => b.committed_at - a.committed_at)
    .map(b => b.name);
  return [...new Set([list?.default_branch || "", ...rest])].filter(Boolean).slice(0, MAX_BASES);
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

/// What the Folder pill says outside a project: "Workspace" and its path,
/// or the folder picked.
export function folderPill(cwd: string, workspace: string | null | undefined, home: string | null | undefined): { label: string; path: string } {
  if (cwd.trim()) return { label: "", path: tildePath(cwd, home) };
  return { label: "Workspace", path: workspace?.trim() ? tildePath(workspace, home) : "~" };
}

// ----- Picker placement -----

/// A box on screen, in CSS pixels.
interface Rect {
  top: number;
  left: number;
  width: number;
  height: number;
}

/// Where a popover goes, in viewport pixels; `maxHeight` is set when it
/// does not fit whole on either side.
interface Placement {
  top: number;
  left: number;
  width: number;
  maxHeight: number | null;
}

/// Places a menu of `size` by `anchor` inside `area`: below when it fits,
/// else above when there is more room there, cut to fit.
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

/// Places a popover of `size` by `anchor` inside a `view` of that size: above
/// it with right edges lined up when it fits there, else below, else on the
/// roomier side cut to fit.
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
