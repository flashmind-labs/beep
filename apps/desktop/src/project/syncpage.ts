// The command center's Sync page: a panel for each project that syncs, its
// machines and its conflicts, those with conflicts first. The sidebar's count
// comes from the same reads.
import { svgEl as icon } from "../ui/parts.ts";
import { state, views } from "../center/state.ts";
import { currentLocation, registerView, drawTop, go } from "../center/views.ts";
import { drawSide } from "../center/sidebar.ts";
import { refresh } from "../center/refresh.ts";
import { draw } from "../center/home.ts";
import { conflictCount, conflictList, conflictSub, conflictTotal } from "./filesync.ts";
import { line, rowButton } from "../ui/pieces.ts";
import { IC } from "../ui/icons.ts";
import { fsync } from "./sections.ts";
import { createElement } from "react";
import { showWindowParts } from "../center/window-root.ts";
import { SyncProject } from "./syncpage-view.tsx";

/// How old a project's sync may get while the page shows.
const PAGE_MAX_AGE_MS = 5000;

/// The projects some machine syncs.
export const syncProjects = () => [...new Set(state.sync.map(r => r.project))];

/// Conflicts across the projects read so far.
export const syncConflicts = () => conflictTotal(syncProjects().map(fsync.detail));

/// Every conflict as one Needs you line on Home, Resolve opening the Sync
/// page where it is compared and settled.
export function conflictLines() {
  return syncProjects().flatMap(p => {
    const d = fsync.detail(p);
    return conflictList(d).map(c => {
      const row = line(icon(IC.sync), c.path, conflictSub(p, c, d?.machines ?? []), [rowButton("Resolve", "small", () => go("sync"))], () => go("sync"));
      row.title = c.path;
      return row;
    });
  });
}

/// Reads the projects' sync that are due; what changed draws itself.
export async function readSync(changed = false) {
  const age = currentLocation()?.view === "sync" ? PAGE_MAX_AGE_MS : undefined;
  await Promise.all(syncProjects().map(p => changed ? fsync.reload(p) : fsync.ensure(p, age)));
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
  const el = views.get("sync")?.el;
  if (!el) return;
  const count = (p: string) => conflictList(fsync.detail(p)).length;
  const projects = syncProjects().sort((a, b) => count(b) - count(a) || a.localeCompare(b));
  const models = projects.map(project => ({ project, model: fsync.page(project) }));
  showWindowParts([{ part: "sync-page", host: el, content: createElement("div", { className: "cc-view" }, projects.length
    ? models.map(({ project, model }) => createElement(SyncProject, { key: project, sync: fsync, project, model }))
    : createElement("div", { className: "cc-panel cc-empty" }, createElement("b", null, "No project syncs yet"))) }]);
}

registerView("sync", {
  title() {
    const n = syncConflicts();
    return { title: "Sync", sub: n ? conflictCount(n) : syncProjects().length ? "All in sync" : "" };
  },
  mount() {
    drawSync();
    readSync();
    refresh();
  },
});
