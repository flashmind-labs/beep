// What the "Add a project to a machine" sheet shows, as pure functions.
// addmachine.ts draws it.
import { syncedBySyncthing, type SyncDetail } from "./filesync.ts";
import type { MachineFolder } from "./projects-state.ts";

/// A project's name without its org: `widget` for `acme/widget`.
export function shortName(project: string) {
  return project.slice(project.lastIndexOf("/") + 1);
}

/// The folder a project goes in when `listing` is picked: that folder when it
/// is there and empty, else one in it, named for the project.
export function destination(listing: MachineFolder, project: string) {
  if (listing.exists && listing.empty) return listing.path;
  return `${listing.path.replace(/\/+$/, "")}/${shortName(project)}`;
}

/// The folder the browser opens for the field's `path`: the one it would go under.
export function browseStart(path: string) {
  const clean = path.trim().replace(/\/+$/, "");
  const at = clean.lastIndexOf("/");
  if (at < 0) return null;
  return at === 0 ? "/" : clean.slice(0, at);
}

/// What the sheet says of the copy.
export interface CopyLine {
  text: string;
  /// Why it is not moving, or what went wrong.
  note: string | null;
  done: boolean;
}

/// "Copying to laptop: 1,204 of 1,880 files". The total is what the machines
/// that have the project hold; with none of them answering, only the count
/// that is known shows.
export function copyLine(detail: SyncDetail | null | undefined, machineId: string, machineName: string): CopyLine {
  const machines = detail?.machines || [];
  const me = machines.find(m => m.machine_id === machineId);
  const others = machines.filter(m => m.machine_id !== machineId);
  const have = me?.files ?? 0;
  const total = Math.max(have + (me?.pending ?? 0), ...others.map(m => m.files));
  const count = (n: number) => n.toLocaleString("en-US");
  const text = total ? `Copying to ${machineName}: ${count(have)} of ${count(total)} files` : `Copying to ${machineName}…`;
  const note = detail?.paused ? "Sync is paused."
    : machines.some(syncedBySyncthing) ? "Synced by Syncthing"
    : me?.error ?? null;
  const done = !!me && !!total && have >= total && me.status === "in_sync";
  return { text, note, done };
}
