// The diff's keyboard and its current file: j and k step between files, x
// folds one, / finds; the current file is marked in the list and its
// section as the view scrolls.
import { currentFile, inView, stepFile, type DiffFile } from "./diff-state.ts";
import { GAP, type Viewer, shown } from "./diff-viewer.ts";
import { drawTree, itemAt, toggle } from "./diff-render.ts";

export function onKey(v: Viewer, e: KeyboardEvent) {
  if (e.metaKey || e.ctrlKey || e.altKey || !(e.target instanceof HTMLElement)) return;
  const t = e.target;
  const find = v.head.querySelector<HTMLInputElement>(".dv-find");

  if (t === find) {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); find.blur(); v.root.focus(); }
    return;
  }

  if (t.closest("input, textarea, [contenteditable='true']")) return;
  const list = shown(v);
  if (["j", "k", "x", "/"].includes(e.key)) e.stopPropagation();
  const at = list.findIndex(f => f.path === v.cur);

  if (e.key === "j" || e.key === "k") {
    const i = stepFile(at, list.length, e.key === "j" ? 1 : -1);
    e.preventDefault();
    if (i != null) jump(v, list[i].path);
  } else if (e.key === "x") {
    const f = list[at] ?? list[currentFile(tops(v, list), v.files.scrollTop)];
    e.preventDefault();
    if (f) toggle(v, f.path);
  } else if (e.key === "/") {
    e.preventDefault();
    find?.focus();
  }
}

/// Scrolls file `path` to the top and makes it current.
export function jump(v: Viewer, path: string) {
  const index = v.indexOf.get(path), item = itemAt(v, index);
  if (index === undefined || !item) return;

  revealFolders(v, path);
  v.cur = path;
  markCurrent(v);
  v.revealTree++;
  drawTree(v);
  // A long way is not worth animating; it would draw every file between.
  const near = Math.abs(item.start - v.files.scrollTop) < v.files.clientHeight * 3;
  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  v.virtual.scrollToIndex(index, { align: "start", behavior: near && !reduced ? "smooth" : "auto" });
}

/// Marks the current file: the one stepped to while it shows, else the one
/// at the top. With `reveal`, the list opens its folders and scrolls to it.
export function markCurrent(v: Viewer, reveal = true) {
  const list = shown(v);
  const held = itemAt(v, v.cur ? v.indexOf.get(v.cur) : undefined);

  if (!held || !held.size || !inView(held.start, held.end - GAP, v.files.scrollTop, v.files.clientHeight)) {
    v.cur = list[currentFile(tops(v, list), v.files.scrollTop)]?.path ?? null;
  }

  if (v.markedPath === v.cur) return;
  v.markedPath = v.cur;
  for (const [p, s] of v.sections) s.classList.toggle("cur", p === v.cur);
  if (!reveal) return;
  revealFolders(v, v.cur);
  v.revealTree++;
  drawTree(v);
}

/// Where each shown file's header sits, less the gap above it.
function tops(v: Viewer, list: readonly DiffFile[]) {
  v.virtual.getVirtualItems();
  return list.map(f => (v.virtual.measurementsCache[v.indexOf.get(f.path) ?? -1]?.start ?? 0) - GAP);
}

function revealFolders(v: Viewer, path: string | null) {
  if (!path) return;
  for (const closed of [v.closedFolders, v.filteredClosedFolders]) {
    for (const folder of closed) if (path.startsWith(folder)) closed.delete(folder);
  }
}
