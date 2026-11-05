// The command center's Sync page: a panel for each project that syncs, its
// machines and its conflicts, those with conflicts first. The sidebar's count
// comes from the same reads.
import { el as h } from "../ui/dom.ts";
import { emptyPanel } from "../ui/parts.ts";
import { state, views } from "../center/state.ts";
import { shown, registerView, drawTop } from "../center/views.ts";
import { drawSide } from "../center/sidebar.ts";
import { refresh } from "../center/refresh.ts";
import { draw } from "../center/home.ts";
import { conflictCount, conflictList, conflictTotal } from "./filesync.ts";
import { fsync } from "./sections.ts";

/// How old a project's sync may get while the page shows.
const PAGE_MAX_AGE_MS = 5000;

/// The projects some machine syncs.
export const syncProjects = () => [...new Set(state.sync.map(r => r.project))];

/// Conflicts across the projects read so far.
export const syncConflicts = () => conflictTotal(syncProjects().map(fsync.detail));

/// Every conflict row across the projects, for Home, projects with the most first.
export function conflictRows() {
  return syncProjects().flatMap(p => {
    const d = fsync.detail(p);
    return conflictList(d).map(c => fsync.conflictRow(p, c, d?.machines ?? [], true));
  });
}

/// Reads the projects' sync that are due; what changed draws itself.
export function readSync() {
  const age = shown === "sync" ? PAGE_MAX_AGE_MS : undefined;
  for (const p of syncProjects()) fsync.ensure(p, age);
}

/// The sidebar's count, and the page while it shows.
export function redrawSync() {
  drawSide();
  if (shown === "home") draw();
  if (shown !== "sync") return;
  drawSync();
  drawTop();
}

function drawSync() {
  const el = views.get("sync")?.el;
  if (!el) return;
  const count = (p: string) => conflictList(fsync.detail(p)).length;
  const projects = syncProjects().sort((a, b) => count(b) - count(a) || a.localeCompare(b));
  const view = h("div", "cc-view", ...(projects.length
    ? projects.flatMap(p => fsync.panel(p, p))
    : [emptyPanel(h("b", "", "No project syncs yet"))]));
  const old = el.querySelector<HTMLElement>(":scope > .cc-view");
  const top = old?.scrollTop ?? 0;
  if (old) old.replaceWith(view); else el.append(view);
  view.scrollTop = top;
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
