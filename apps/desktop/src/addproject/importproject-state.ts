// What the "Import a project" sheet shows and how it steps, as pure
// functions so they can be tested without Tauri. importproject.ts draws it.
import type { ProjectRow, MachineFolder } from "../project/projects-state.ts";
import { reason } from "../ui/format.ts";

/// "org/repo", a GitHub URL, a Forgejo URL or `git@host:org/repo`: what the
/// server takes. It says why when it is none of these.
export function looksLikeRepo(text: string | null | undefined) {
  const t = String(text || "").trim();
  if (!t) return false;
  if (/^git@[^\s:]+:[^\s/]+\/[^\s/]+$/.test(t)) return true;
  if (/^(https?|ssh):\/\/[^\s/]+\/[^\s/]+\/[^\s/]+/.test(t)) return true;
  return /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/?$/.test(t) || /^[a-z0-9.-]+\.[a-z]+\/[^\s/]+\/[^\s/]+/i.test(t);
}

/// A machine chip: its id, or null for Auto.
export interface MachineChip {
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

/// The folder projects go under, from where one landed: `folder` less its
/// `org/repo` ending when it has one, else the folder it sits in.
export function importRoot(folder: string, name: string): string | null {
  const clean = folder.replace(/\/+$/, "");
  const tail = `/${name.replace(/^\/+|\/+$/g, "")}`;
  const root = clean.endsWith(tail) ? clean.slice(0, -tail.length) : clean.slice(0, clean.lastIndexOf("/"));
  return root || null;
}

/// The folder `repo` goes in under `root`, as the last import put its own.
export function folderUnder(root: string, repo: string): string | null {
  const parts = orgAndName(repo);
  return parts ? `${root.replace(/\/+$/, "")}/${parts[0]}/${parts[1]}` : null;
}

/// The machine and the folder projects went under at the last import, kept
/// across launches so the next import starts there.
export interface LastImport {
  machine: string;
  root: string | null;
}

/// The parent a project goes under on `machine` before anything is typed: the
/// last one used there, else the projects folder of `base`, where it starts.
export function defaultParent(machine: string, base: string | null | undefined, os: string | null | undefined, last: LastImport | null) {
  if (last?.root && last.machine === machine) return last.root;
  if (!base) return null;
  return `${base.replace(/\/+$/, "")}/${os === "linux" ? "projects" : "Projects"}`;
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
export function folderHelp(state: Pick<ImportState, "machine" | "chosen">, machines: readonly Named[] | null | undefined) {
  const tokens = "Private repositories use your tokens from Settings > Credentials.";
  if (!state.machine) return `Beep picks a machine and its default folder. ${tokens}`;
  if (state.chosen) return tokens;
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
  /// The folder the browser shows while it is open.
  browsing: MachineFolder | null;
  /// What the server made, once `step` is "done".
  project?: ProjectRow;
}

/// What changes the Import a project sheet's state; see `next`.
export type ImportEvent =
  | { type: "edit"; repo: string }
  | { type: "pick"; machine: string | null }
  | { type: "browse"; listing: MachineFolder | null }
  | { type: "choose"; listing: MachineFolder | null }
  | { type: "unchoose" }
  | { type: "oops"; message: string }
  | { type: "submit" }
  | { type: "done"; project: ProjectRow }
  | { type: "failed"; message: string };

/// The sheet before anything is typed.
export const START: ImportState = { step: "form", repo: "", machine: null, error: null, chosen: null, browsing: null };

/// The next state after `event`:
/// - `edit` as the field changes
/// - `pick` for a chip, null for Auto
/// - `browse` opens or moves the folder browser
/// - `choose` picks that folder; a checkout fills the field from its origin
/// - `unchoose` goes back to the default folder, and closes the browser
/// - `oops` when reading a folder failed
/// - `submit` from Import or Enter
/// - `done` when the server answered
/// - `failed` when it refused
export function next(state: ImportState, event: ImportEvent): ImportState {
  switch (event.type) {
    case "edit":
      if (state.step !== "form") return state;
      return { ...state, repo: event.repo, error: null };
    case "pick":
      if (state.step !== "form") return state;
      if ((event.machine ?? null) === state.machine) return state;
      return { ...state, machine: event.machine ?? null, chosen: null, browsing: null, error: null };
    case "browse":
      if (state.step !== "form" || !state.machine) return state;
      return { ...state, browsing: event.listing, error: null };
    case "choose": {
      if (state.step !== "form" || !state.machine || !event.listing) return state;
      const origin = event.listing.origin;
      const repo = origin ? repoFromRemote(origin) : state.repo;
      return { ...state, chosen: event.listing, browsing: null, repo, error: null };
    }
    case "unchoose":
      if (state.step !== "form") return state;
      return { ...state, chosen: null, browsing: null };
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

/// The server's sentence, without what the bridge puts before it.
export function cleanError(message: string | null | undefined) {
  return reason(message || "").trim() || "The import failed.";
}

/// What shows while it clones: "Cloning org/repo onto build-box…", or
/// "Importing" for a checkout already there.
export function progressText(state: Pick<ImportState, "machine" | "chosen" | "repo">, machines: readonly Named[] | null | undefined) {
  const m = (machines || []).find(x => x.id === state.machine);
  const how = state.chosen && state.chosen.origin != null ? "Importing" : "Cloning";
  return `${how} ${state.repo} onto ${m ? m.name : "your machine"}…`;
}

/// The line under the title once it is in: where it went.
export function doneText(project: Pick<ProjectRow, "cloned" | "machine_name" | "folder">) {
  const how = project.cloned ? "Cloned" : "Already there, now listed";
  return `${how} on ${project.machine_name} in ${project.folder}`;
}
