// What the "Import a project" sheet shows and how it steps, as pure
// functions so they can be tested without Tauri. importproject.ts draws it.
import type { ProjectRow, MachineFolder } from "../project/projects-state.ts";
import { reason } from "../ui/format.ts";

/// "org/repo", a GitHub URL, a Forgejo URL or `git@host:org/repo`: what the
/// Beep takes. It says why when it is none of these.
export function looksLikeRepo(text: string | null | undefined) {
  const t = String(text || "").trim();
  if (!t) return false;
  if (/^git@[^\s:]+:[^\s/]+\/[^\s/]+$/.test(t)) return true;
  if (/^(https?|ssh):\/\/[^\s/]+\/[^\s/]+\/[^\s/]+/.test(t)) return true;
  return /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/?$/.test(t) || /^[a-z0-9.-]+\.[a-z]+\/[^\s/]+\/[^\s/]+/i.test(t);
}

/// A machine chip: its id, or null for Auto.
interface MachineChip {
  id: string | null;
  label: string;
}

/// The chips under the field: Auto, then each machine online.
export function machineChips(machines: readonly { id: string; name: string; online: boolean }[] | null | undefined): MachineChip[] {
  const online = (machines || []).filter(m => m.online);
  return [{ id: null, label: "Auto" }, ...online.map(m => ({ id: m.id, label: m.name }))];
}

/// `org/repo` for a GitHub remote, so the field reads as if typed; any other
/// remote as it is.
export function repoFromRemote(remote: string | null | undefined) {
  const r = String(remote || "").trim();
  const m = r.match(/^(?:https?:\/\/(?:www\.)?github\.com\/|ssh:\/\/git@github\.com\/|git@github\.com:)([^\s/]+)\/([^\s/]+?)(?:\.git)?\/?$/i);
  return m ? `${m[1]}/${m[2]}` : r;
}

/// The org and the name of what the field holds: ["org", "repo"], or null
/// before it names a repository.
function orgAndName(repo: string) {
  if (!looksLikeRepo(repo)) return null;
  const parts = String(repo).trim().replace(/\.git\/?$/, "").split(/[/:]/).filter(Boolean);
  return parts.length >= 2 ? parts.slice(-2) : null;
}

/// Strips whitespace and trailing slashes from a folder path, keeping `/`.
export function cleanFolder(path: string | null | undefined) {
  return String(path || "").trim().replace(/(.)\/+$/, "$1");
}

/// The folder typed in Clone into when it differs from the machine's default
/// projects root; null while untouched, blank, or equal to `defaultRoot`.
export function typedFolder(typed: string | null | undefined, defaultRoot: string | null | undefined): string | null {
  if (typed == null) return null;
  const clean = cleanFolder(typed);
  if (!clean || clean === cleanFolder(defaultRoot)) return null;
  return clean;
}

/// True when a folder was picked or typed away from the machine's default root.
export function hasCustomFolder(
  state: Pick<ImportState, "machine" | "chosen">,
  typed: string | null | undefined,
  defaultRoot: string | null | undefined,
) {
  if (!state.machine) return false;
  if (state.chosen) return true;
  if (typed == null) return false;
  return cleanFolder(typed) !== cleanFolder(defaultRoot);
}

/// The folder `repo` goes in under `root`: `<root>/<org>/<repo>`.
export function folderUnder(root: string, repo: string): string | null {
  const parts = orgAndName(repo);
  return parts ? `${cleanFolder(root)}/${parts[0]}/${parts[1]}` : null;
}

/// The destination preview under Clone into before a folder listing is read.
export function goesInText(repo: string, target: string | null, parent: string | null) {
  if (target) return `Goes in ${target}.`;
  if (!parent) return "";
  return `Goes in ${folderUnder(parent, repo) ?? `${cleanFolder(parent)}/<org>/<repo>`}.`;
}

/// Where the project `project` goes under `root`: `<root>/<org>/<repo>`, or
/// `<root>/<name>` for a local one.
export function projectUnder(root: string, project: string) {
  const base = root.replace(/\/+$/, ""), [org, name] = project.split("/");
  return name && org !== "local" ? `${base}/${org}/${name}` : `${base}/${name || org}`;
}

/// The machine the last import went on, kept across launches so the next
/// one starts there.
export interface LastImport {
  machine: string;
}

const LAST_KEY = "beep.lastImport.v1";

export function lastImport(): LastImport | null {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(LAST_KEY) || "null");
    return v && typeof v === "object" && "machine" in v && typeof v.machine === "string" ? { machine: v.machine } : null;
  } catch {
    return null;
  }
}

/// Notes the machine `project` landed on, for the next import.
export function keepLastImport(project: ProjectRow) {
  const last: LastImport = { machine: project.machine_id };
  try { localStorage.setItem(LAST_KEY, JSON.stringify(last)); } catch { /* kept for this launch only */ }
}

/// What importing into the folder `listing` does, as the line under it says.
export function folderNote(listing: MachineFolder | null | undefined, repo: string) {
  if (!listing) return null;

  if (listing.origin != null) {
    const what = listing.origin ? repoFromRemote(listing.origin) : "a repository with no origin remote";
    return `A checkout of ${what}, imported in place.`;
  }

  if (!listing.exists) return "Not there yet: the clone makes it.";
  if (listing.empty) return "Empty: the clone goes here.";
  const name = (orgAndName(repo) || [])[1] || "repo";
  return `Has other files: it goes in ${listing.path.replace(/\/+$/, "")}/${name}.`;
}

/// A machine by id and name, as the sheets look one up.
type Named = { id: string; name: string };

/// The help line under the folder.
export function folderHelp(state: Pick<ImportState, "machine" | "chosen">, machines: readonly Named[] | null | undefined, custom = false) {
  const tokens = "Private repositories use your tokens from Settings > Credentials.";
  if (!state.machine) return `Beep picks a machine and its default folder. ${tokens}`;
  if (state.chosen || custom) return tokens;
  const m = (machines || []).find(x => x.id === state.machine);
  return `The default on ${m ? m.name : "that machine"}. ${tokens}`;
}

/// The Import a project sheet's state.
export interface ImportState {
  step: "form" | "cloning" | "done";
  repo: string;
  /// The machine picked, null for Auto.
  machine: string | null;
  error: string | null;
  /// The folder picked, as `list_machine_folder` read it (null for the
  /// machine's default).
  chosen: MachineFolder | null;
  /// What Beep made, once `step` is "done".
  project?: ProjectRow;
}

/// What changes the Import a project sheet's state; see `next`.
export type ImportEvent =
  | { type: "edit"; repo: string }
  | { type: "pick"; machine: string | null }
  | { type: "choose"; listing: MachineFolder | null }
  | { type: "unchoose" }
  | { type: "oops"; message: string }
  | { type: "submit" }
  | { type: "done"; project: ProjectRow }
  | { type: "failed"; message: string };

/// The sheet before anything is typed.
export const START: ImportState = { step: "form", repo: "", machine: null, error: null, chosen: null };

/// The next state after `event`:
/// - `edit` as the field changes
/// - `pick` for a chip, null for Auto
/// - `choose` picks that folder; a checkout fills the field from its origin
/// - `unchoose` goes back to the default folder
/// - `oops` when reading a folder failed
/// - `submit` from Import or Enter
/// - `done` when Beep answered
/// - `failed` when it refused
export function next(state: ImportState, event: ImportEvent): ImportState {
  switch (event.type) {
    case "edit":
      if (state.step !== "form") return state;
      return { ...state, repo: event.repo, error: null };
    case "pick":
      if (state.step !== "form") return state;
      if ((event.machine ?? null) === state.machine) return state;
      return { ...state, machine: event.machine ?? null, chosen: null, error: null };
    case "choose": {
      if (state.step !== "form" || !state.machine || !event.listing) return state;
      const origin = event.listing.origin;
      const repo = origin ? repoFromRemote(origin) : state.repo;
      return { ...state, chosen: event.listing, repo, error: null };
    }
    case "unchoose":
      if (state.step !== "form") return state;
      return { ...state, chosen: null };
    case "oops":
      if (state.step !== "form") return state;
      return { ...state, error: cleanError(event.message) };
    case "submit":
      if (state.step !== "form" || !looksLikeRepo(state.repo)) return state;
      return { ...state, step: "cloning", repo: state.repo.trim(), error: null };
    case "done":
      if (state.step !== "cloning") return state;
      return { ...state, step: "done", project: event.project };
    case "failed":
      if (state.step !== "cloning") return state;
      return { ...state, step: "form", error: cleanError(event.message) };
    default:
      return state;
  }
}

/// The refusal, without what the bridge puts before it.
export function cleanError(message: string | null | undefined) {
  return reason(message || "").trim() || "The import failed.";
}

/// What shows while it clones: "Cloning org/repo onto build-box…", or
/// "Importing" for a checkout already there.
export function progressText(
  state: Pick<ImportState, "machine" | "chosen" | "repo">,
  machines: readonly Named[] | null | undefined,
  listing: MachineFolder | null = state.chosen ?? null,
) {
  const m = (machines || []).find(x => x.id === state.machine);
  const how = listing?.origin != null ? "Importing" : "Cloning";
  return `${how} ${state.repo} onto ${m ? m.name : "your machine"}…`;
}

/// The line under the title once it is in: where it went.
export function doneText(project: Pick<ProjectRow, "cloned" | "machine_name" | "folder">) {
  const how = project.cloned ? "Cloned" : "Already there, now listed";
  return `${how} on ${project.machine_name} in ${project.folder}`;
}
