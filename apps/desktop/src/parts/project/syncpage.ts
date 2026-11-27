// Every synced project's state for the sidebar and Home: which projects sync
// and their conflicts. A project's Machines section shows the detail.
import { state } from "../../stores/center/state.ts";
import { fsync } from "./sections.ts";

/// The projects some machine syncs.
export const syncProjects = () => [...new Set(state.sync.map(r => r.project))];

/// Reads the sync of the projects not read yet; what changed draws itself.
export async function readSync() {
  await Promise.all(syncProjects().map(p => fsync.ensure(p)));
}
