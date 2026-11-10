// The command center's Sync page: a panel for each project that syncs, its
// machines and its conflicts, those with conflicts first. The sidebar's count
// comes from the same reads.
import { type CenterView, state } from "../center/state.ts";
import { currentLocation, drawPage, drawTop } from "../center/views.ts";
import { drawSide } from "../center/sidebar.ts";
import { ensureHome } from "../center/refresh.ts";
import { UNWATCHED_MS } from "../ui/pagecache.ts";
import { draw } from "../center/home.ts";
import { conflictCount, conflictList, conflictTotal } from "./filesync.ts";
import { fsync } from "./sections.ts";
import { createElement } from "react";
import { SyncPage } from "./syncpage-view.tsx";

/// How old a project's sync may get while the page shows and its summary
/// changes.
const PAGE_MAX_AGE_MS = 5000;

/// The projects some machine syncs.
export const syncProjects = () => [...new Set(state.sync.map(r => r.project))];

/// Conflicts across the projects read so far.
export const syncConflicts = () => conflictTotal(syncProjects().map(fsync.detail));

/// Reads the projects' sync that are due, `age` old by default; what
/// changed draws itself.
export async function readSync(age = currentLocation()?.view === "sync" ? PAGE_MAX_AGE_MS : undefined) {
  await Promise.all(syncProjects().map(p => fsync.ensure(p, age)));
}

/// After a sync change: reads the projects whose summary changed, and the
/// ones on screen once their detail is a few seconds old. Others wait for
/// their own change or for being shown.
export async function followSync(changed: ReadonlySet<string>) {
  await fsync.follow(changed, currentLocation()?.view === "sync" ? syncProjects() : [], PAGE_MAX_AGE_MS);
}

/// The sidebar's count, and the page while it shows.
export function redrawSync() {
  drawSide();
  if (currentLocation()?.view === "home") draw();
  if (currentLocation()?.view !== "sync") return;
  drawSync();
  drawTop();
}

function drawSync() {
  drawPage("sync");
}

/// The Sync page, projects with conflicts first.
export function SyncRoute() {
  const count = (p: string) => conflictList(fsync.detail(p)).length;
  const projects = syncProjects().sort((a, b) => count(b) - count(a) || a.localeCompare(b));
  const models = projects.map(project => ({ project, model: fsync.page(project) }));
  return createElement(SyncPage, { sync: fsync, models });
}

export const syncView: CenterView<"sync"> = {
  title() {
    const n = syncConflicts();
    return { title: "Sync", sub: n ? conflictCount(n) : syncProjects().length ? "All in sync" : "" };
  },
  mount() {
    // Sync changes read the projects they touch on any page, so opening it
    // reads only a project not read in a long while.
    drawSync();
    void readSync(UNWATCHED_MS);
    ensureHome();
  },
};
