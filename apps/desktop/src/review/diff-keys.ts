// The diff's keyboard, its current file and its folds: j and k step between
// files, x folds one, / finds; the current file is marked in the list and its
// section as the view scrolls.
import { currentFile, inView, isFolded, estimateHeight, matchesFilter, stepFile, type DiffFile } from "./diff-state.ts";
import { GAP, type Viewer, indexOf, itemAt, keepCurrent, remeasure, remember, shown } from "./diff-viewer.ts";

export function onKey(v: Viewer, e: KeyboardEvent) {
  if (e.metaKey || e.ctrlKey || e.altKey || !(e.target instanceof HTMLElement)) return;
  const t = e.target, find = v.find;

  if (t === find) {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); find.blur(); v.root.focus(); }
    return;
  }

  if (t.closest("input, textarea, [contenteditable='true']")) return;
  const list = shown(v.state);
  if (["j", "k", "x", "/"].includes(e.key)) e.stopPropagation();
  const at = list.findIndex(f => f.path === v.state.cur);

  if (e.key === "j" || e.key === "k") {
    const i = stepFile(at, list.length, e.key === "j" ? 1 : -1);
    e.preventDefault();
    if (i != null) jump(v, list[i].path);
  } else if (e.key === "x") {
    const f = list[at] ?? list[currentFile(tops(v, list), v.files?.scrollTop ?? 0)];
    e.preventDefault();
    if (f) toggle(v, f.path);
  } else if (e.key === "/") {
    e.preventDefault();
    find?.focus();
  }
}

/// Scrolls file `path` to the top and makes it current.
export function jump(v: Viewer, path: string) {
  const index = indexOf(v, path), item = itemAt(v, index), files = v.files;
  if (index < 0 || !item || !files || !v.virtual) return;
  v.store.setState({ cur: path, revealTree: v.state.revealTree + 1, ...revealFolders(v, path) });
  // A long way is not worth animating; it would draw every file between.
  const near = Math.abs(item.start - files.scrollTop) < files.clientHeight * 3;
  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  v.virtual.scrollToIndex(index, { align: "start", behavior: near && !reduced ? "smooth" : "auto" });
}

/// Marks the current file: the one stepped to while it shows, else the one
/// at the top. With `reveal`, the list opens its folders and scrolls to it.
export function markCurrent(v: Viewer, reveal = true) {
  const files = v.files;
  if (!files || v.disposed) return;
  const s = v.state, list = shown(s);
  const held = itemAt(v, indexOf(v, s.cur));
  let cur = s.cur;

  if (!held || !held.size || !inView(held.start, held.end - GAP, files.scrollTop, files.clientHeight)) {
    cur = list[currentFile(tops(v, list), files.scrollTop)]?.path ?? null;
  }

  if (cur === s.cur) return;
  // Not flushed: it runs from effects and scrolling, and nothing measures it.
  v.store.setState(reveal ? { cur, revealTree: s.revealTree + 1, ...revealFolders(v, cur) } : { cur });
}

/// Where each shown file's header sits, less the gap above it.
function tops(v: Viewer, list: readonly DiffFile[]) {
  v.virtual?.getVirtualItems();
  const at = new Map(v.state.list.map((f, i) => [f.path, i]));
  return list.map(f => (v.virtual?.measurementsCache[at.get(f.path) ?? -1]?.start ?? 0) - GAP);
}

/// The folders that hold `path`, opened.
function revealFolders(v: Viewer, path: string | null) {
  const open = (closed: ReadonlySet<string>) => new Set([...closed].filter(folder => !path?.startsWith(folder)));
  return { closedFolders: open(v.state.closedFolders), filteredClosedFolders: open(v.state.filteredClosedFolders) };
}

/// A file's height before it is measured, with the gap after it.
export function sizeOf(f: DiffFile | undefined, filter: string, folded: ReadonlyMap<string, boolean>) {
  return f && matchesFilter(f, filter) ? estimateHeight(f, isFolded(f, folded)) + GAP : 0;
}

export function toggle(v: Viewer, path: string) {
  const f = v.state.view?.files.find(x => x.path === path);
  if (!f) return;
  const folding = !isFolded(f, v.state.folded);
  // Folding a file scrolled past its header keeps its header in view.
  const item = itemAt(v, indexOf(v, path));
  if (folding && item && v.files && item.start < v.files.scrollTop) v.files.scrollTop = item.start - GAP;
  setFolded(v, [path], folding);
  remember(v);
}

function setFolded(v: Viewer, paths: readonly string[], folded: boolean) {
  const next = new Map(v.state.folded);
  for (const path of paths) next.set(path, folded);
  v.set({ folded: next });

  // A file not drawn is laid out again by its estimate; a drawn one is measured as it changes.
  for (const path of paths) {
    const index = indexOf(v, path), f = v.state.list[index];
    if (f && !v.sections.has(path)) v.virtual?.resizeItem(index, sizeOf(f, v.state.filter, next));
  }
}

export function foldAll(v: Viewer, folded: boolean) {
  keepCurrent(v, () => {
    setFolded(v, shown(v.state).map(f => f.path), folded);
    remeasure(v);
  });
  remember(v);
  if (folded && v.files) v.files.scrollTop = 0;
}
