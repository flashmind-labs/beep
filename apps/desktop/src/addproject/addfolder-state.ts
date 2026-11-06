// What the "Add a folder" sheet decides, as pure functions so they can be
// tested without Tauri: the machine it starts on, what Choose… opens, and
// what the folder chosen is.
import { cleanError, repoFromRemote } from "./importproject-state.ts";
import type { MachineFolder, ProjectRow } from "../project/projects-state.ts";

/// The Add a folder sheet's state.
export interface FolderState {
  step: "form" | "adding" | "done";
  machine: string | null;
  /// The folder the browser shows while it is open.
  browsing: MachineFolder | null;
  /// The folder picked, as `list_machine_folder` read it.
  chosen: MachineFolder | null;
  /// The chosen checkout's branch: undefined while it is read, null for none.
  branch: string | null | undefined;
  error: string | null;
  project: ProjectRow | null;
}

export type FolderEvent =
  | { type: "pick"; machine: string }
  | { type: "browse"; listing: MachineFolder | null }
  | { type: "choose"; listing: MachineFolder }
  | { type: "branch"; path: string; branch: string | null }
  | { type: "oops"; message: string }
  | { type: "submit" }
  | { type: "done"; project: ProjectRow }
  | { type: "failed"; message: string };

/// The sheet on this Mac when it is online, else the first machine online.
export function startOn(machines: readonly { id: string; online: boolean }[], macId: string | null): FolderState {
  const online = machines.filter(m => m.online);
  const machine = online.find(m => m.id === macId)?.id ?? online[0]?.id ?? null;
  return { step: "form", machine, browsing: null, chosen: null, branch: null, error: null, project: null };
}

/// What Choose… opens on the machine picked: the macOS folder picker on this
/// Mac, the folder browser on another machine.
export function chooseWith(machine: string | null, macId: string | null): "picker" | "browser" | null {
  if (!machine) return null;
  return machine === macId ? "picker" : "browser";
}

export function nextFolder(state: FolderState, event: FolderEvent): FolderState {
  if (event.type === "done") return state.step === "adding" ? { ...state, step: "done", project: event.project } : state;
  if (event.type === "failed") return state.step === "adding" ? { ...state, step: "form", error: cleanError(event.message) } : state;
  if (state.step !== "form") return state;
  switch (event.type) {
    case "pick":
      if (event.machine === state.machine) return state;
      return { ...state, machine: event.machine, browsing: null, chosen: null, branch: null, error: null };
    case "browse":
      return { ...state, browsing: event.listing, error: null };
    case "choose":
      return { ...state, browsing: null, chosen: event.listing, branch: event.listing.origin != null ? undefined : null, error: null };
    case "branch":
      return state.chosen?.path === event.path ? { ...state, branch: event.branch } : state;
    case "oops":
      return { ...state, error: cleanError(event.message) };
    case "submit":
      return addable(state) ? { ...state, step: "adding", error: null } : state;
  }
}

/// A checkout with an origin remote can be added in place.
export function addable(state: Pick<FolderState, "chosen" | "machine">) {
  return !!state.machine && !!state.chosen?.origin;
}

/// The repository Add project names: the checkout's origin.
export function repoOf(chosen: MachineFolder) {
  return repoFromRemote(chosen.origin);
}

/// What the folder chosen is, in words: whether it is a checkout, of what,
/// its remote and branch.
export function folderFacts(chosen: MachineFolder | null, branch: string | null | undefined): string[] {
  if (!chosen) return [];
  if (chosen.origin == null) return ["Not a git checkout. Choose the folder that holds .git."];
  if (!chosen.origin) return ["A checkout with no origin remote. Add an origin remote to add it."];
  const on = branch === undefined ? "Reading the branch…" : branch ? `Branch ${branch}` : "No branch";
  return [`A checkout of ${repoOf(chosen)}, added in place.`, `Remote ${chosen.origin} · ${on}`];
}
