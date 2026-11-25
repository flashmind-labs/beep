// Every synced project's state for the sidebar and Home: which projects sync
// and their conflicts. A project's Machines section shows the detail.
import { state } from "../center/state.ts";
import { currentLocation } from "../center/views.tsx";
import { drawSide } from "../center/sidebar.tsx";
import { draw } from "../center/home.tsx";
import { fsync } from "./sections.ts";

/// The projects some machine syncs.
export const syncProjects = () => [...new Set(state.sync.map(r => r.project))];

/// Reads the sync of the projects not read yet; what changed draws itself.
export async function readSync() {
  await Promise.all(syncProjects().map(p => fsync.ensure(p)));
}

/// The sidebar's conflict counts and Home's conflict cards.
export function redrawSync() {
  drawSide();
  if (currentLocation()?.view === "home") draw();
}
