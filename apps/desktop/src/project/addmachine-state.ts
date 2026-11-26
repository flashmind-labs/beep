// What the "Add a project to a machine" sheet shows, as pure functions.
// addmachine.tsx draws it.
import { fileText, rateText, syncPct, timeLeft, type SyncDetail } from "./filesync.ts";
import { bytes } from "../ui/format.ts";
import { syncedBySyncthing, type MachineFolder } from "./projects-state.ts";
import { projectUnder } from "../addproject/importproject-state.ts";

/// A project's name without its org: `widget` for `acme/widget`.
export function shortName(project: string) {
  return project.slice(project.lastIndexOf("/") + 1);
}

/// One thing the picker offers to do with the folder it shows: the folder the
/// project would be in, and what the row says.
export interface FolderOffer {
  label: string;
  path: string;
}

/// What the picker offers for `listing`. A checkout joins as it is, and so
/// does a folder named for the project, with the files it has; an empty one
/// takes the project; any other gets a folder in it, named for the project.
/// A folder that already has one named for it offers nothing: that one is
/// the place. `root` is the machine's projects folder, which holds projects
/// and is never one: there it goes under its org. `change` points a project
/// the machine has at another folder, and moves nothing.
export function offers(listing: MachineFolder, project: string, change: boolean, root: string | null): FolderOffer[] {
  const here = listing.path, name = shortName(project);
  const [fresh, take] = change ? ["Use a new folder", "Use this folder"] : ["Add in a new folder", "Add here"];
  if (listing.origin != null) return [{ label: "Use this checkout", path: here }];
  if (listing.folders.includes(name)) return [];
  if (here === root) return [{ label: fresh, path: projectUnder(here, project) }];
  if (!listing.exists || listing.empty) return [{ label: take, path: here }];
  if (here.slice(here.lastIndexOf("/") + 1) === name) return [{ label: `${take}, keeping its files`, path: here }];

  return [{ label: fresh, path: `${here.replace(/\/+$/, "")}/${name}` }];
}

/// The folder the picker opens for a project in `path`: the one it is in.
export function browseStart(path: string) {
  const clean = path.trim().replace(/\/+$/, "");
  const at = clean.lastIndexOf("/");
  if (at < 0) return null;
  return at === 0 ? "/" : clean.slice(0, at);
}

/// What the sheet says of the copy.
export interface CopyLine {
  text: string;
  /// How far it is, 0 to 100, while the machine syncs; null otherwise.
  pct: number | null;
  /// The large file it waits on and how much of it came, while it comes.
  file: string | null;
  /// Why it is not moving, or what went wrong.
  note: string | null;
  done: boolean;
}

/// "Copying to laptop: 12 MB of 25 MB · 1.2 MB/s · under a minute" while
/// the machine says how far it is, else "Copying to laptop: 1,204 of 1,880
/// files", with the large file it waits on under it. The total is what the machines that have the project hold; with
/// none of them answering, only the count that is known shows.
export function copyLine(detail: SyncDetail | null | undefined, machineId: string, machineName: string): CopyLine {
  const machines = detail?.machines || [];
  const me = machines.find(m => m.machine_id === machineId);
  const others = machines.filter(m => m.machine_id !== machineId);
  const have = me?.files ?? 0;
  const total = Math.max(have + (me?.pending ?? 0), ...others.map(m => m.files));
  const count = (n: number) => n.toLocaleString("en-US");
  const p = me?.status === "syncing" ? me.progress : null;
  const text = p && p.bytes_total
    ? [`Copying to ${machineName}: ${bytes(p.bytes_done)} of ${bytes(p.bytes_total)}`, rateText(p.rate), timeLeft(p.eta_secs)].filter(Boolean).join(" · ")
    : total ? `Copying to ${machineName}: ${count(have)} of ${count(total)} files` : `Copying to ${machineName}…`;
  const note = detail?.paused ? "Sync is paused."
    : machines.some(syncedBySyncthing) ? "Synced by Syncthing"
    : me?.error ?? null;
  const done = !!me && !!total && have >= total && me.status === "in_sync";
  return { text, pct: me ? syncPct(me) : null, file: fileText(p?.file ?? null) || null, note, done };
}
