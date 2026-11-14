// Every synced project's state for the sidebar and Home: which projects sync
// and their conflicts. A project's Machines section shows the detail.
import { state } from "../center/state.ts";
import { currentLocation } from "../center/views.ts";
import { drawSide } from "../center/sidebar.ts";
import { draw } from "../center/home.ts";
import { fsync } from "./sections.ts";

/// How often a project's sync detail is read while its summary changes.
const SHOWN_MAX_AGE_MS = 10_000;

/// The projects some machine syncs.
export const syncProjects = () => [...new Set(state.sync.map(r => r.project))];

/// Reads the projects' sync that are due; what changed draws itself.
export async function readSync() {
  await Promise.all(syncProjects().map(p => fsync.ensure(p)));
}

/// After a sync change: reads the projects whose summary changed, and the
/// one on screen, at most every ten seconds. Others wait for their own
/// change or for being shown.
export async function followSync(changed: ReadonlySet<string>) {
  await fsync.follow(changed, [], SHOWN_MAX_AGE_MS);
}

/// The sidebar's conflict counts and Home's conflict cards.
export function redrawSync() {
  drawSide();
  if (currentLocation()?.view === "home") draw();
}
