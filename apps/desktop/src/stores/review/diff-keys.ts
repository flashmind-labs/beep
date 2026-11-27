// The diff's keyboard, its current file and its folds: j and k step between
// files, x folds one, / finds, n and N or enter step between the places
// found; the current file is marked in the list and its
// section as the view scrolls.
import { glide } from "../../ui/motion.ts";
import { currentFile, diffRows, inView, isFolded, estimateHeight, lineKey, matchesFilter, sameHit, splitRows, stepFile, type DiffFile, type LineMatch } from "./diff-state.ts";
import { CHUNK, GAP, type Viewer, hits, indexOf, itemAt, keepCurrent, remeasure, remember, shown } from "./diff-viewer.ts";

export function onKey(v: Viewer, e: KeyboardEvent) {
  // ⌘F in the diff is its filter, not the chat's find.
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && !e.isComposing && e.key.toLowerCase() === "f" && v.find && !v.find.disabled) {
    e.preventDefault();
    e.stopPropagation();
    v.find.focus();
    v.find.select();
    return;
  }

  if (e.metaKey || e.ctrlKey || e.altKey || !(e.target instanceof HTMLElement)) return;
  const t = e.target, find = v.find;

  if (t === find) {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); find.blur(); v.root.focus(); }
    if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); e.stopPropagation(); stepHit(v, e.shiftKey ? -1 : 1); }
    return;
  }

  if (t.closest("input, textarea, [contenteditable='true']")) return;
  const list = shown(v.state);
  if (["j", "k", "x", "/", "n", "N"].includes(e.key)) e.stopPropagation();
  const at = list.findIndex(f => f.path === v.state.cur);

  if (e.key === "j" || e.key === "k") {
    const i = stepFile(at, list.length, e.key === "j" ? 1 : -1);
    e.preventDefault();
    if (i != null) jump(v, list[i].path);
  } else if (e.key === "x") {
    // The file holding the focus, else the one clicked or stepped to, else the one at the top.
    const focused = t.closest<HTMLElement>(".dv-file")?.dataset.path;
    const path = focused ?? list[at]?.path ?? list[topFile(v, v.files?.scrollTop ?? 0)]?.path;
    e.preventDefault();

    if (path) {
      v.store.setState({ cur: path });
      toggle(v, path);
    }
  } else if (e.key === "/") {
    e.preventDefault();
    find?.focus();
  } else if (e.key === "n" || e.key === "N") {
    e.preventDefault();
    stepHit(v, e.key === "n" ? 1 : -1);
  }
}

/// Goes to the next place the filter finds, or the previous one with a
/// negative `step`, round from the last to the first.
export function stepHit(v: Viewer, step: 1 | -1) {
  const all = hits(v.state);
  if (!all.length) return;
  const at = all.findIndex(hit => sameHit(hit, v.state.hit));
  const next = all[at < 0 ? (step > 0 ? 0 : all.length - 1) : (at + step + all.length) % all.length];
  jumpToLine(v, next.path, next);
}

/// Scrolls file `path` to the top and makes it current.
export function jump(v: Viewer, path: string) {
  const index = indexOf(v, path), item = itemAt(v, index), files = v.files;
  if (index < 0 || !item || !files || !v.virtual) return;
  v.store.setState({ cur: path, revealTree: v.state.revealTree + 1, ...revealFolders(v, path) });
  // The snap cuts a long way short, so it draws few files between; the list then settles on
  // the file's measured place.
  const virtual = v.virtual;
  glide(files, item.start, "snap", { landed: null, done: () => virtual.scrollToIndex(index, { align: "start" }) });
}

/// Scrolls line `match` of file `path` to the middle, marks it a moment and
/// makes the place found in it current. Its chunk draws its rows only once
/// near, so it goes there first.
export function jumpToLine(v: Viewer, path: string, match: LineMatch) {
  const f = v.state.view?.files.find(x => x.path === path);
  if (!f) return;
  v.set({ hit: { ...match, path } });
  if (isFolded(f, v.state.folded)) setFolded(v, [path], false);
  const key = lineKey(match.side, match.line), chunk = Math.floor(rowIndex(v, f, match) / CHUNK);
  // A row drawn already is near the viewport: going to its file first would jump away and back.
  if (!v.sections.get(path)?.querySelector(`[data-at="${key}"]`)) jump(v, path);
  else v.store.setState({ cur: path });
  let frames = 60, near = false;

  const step = () => {
    if (v.disposed || frames-- <= 0) return;
    const section = v.sections.get(path);
    const row = section?.querySelector<HTMLElement>(`[data-at="${key}"]`);

    if (row) {
      // In side by side, an unchanged line's two cells share its key; the mark is in one.
      (section?.querySelector("mark.cur") ?? row).scrollIntoView({ block: "center", inline: "nearest" });
      row.classList.add("dv-hit");
      window.setTimeout(() => row.classList.remove("dv-hit"), 1500);
      return;
    }

    const box = section?.querySelectorAll<HTMLElement>(".dv-chunk")[chunk];

    if (box && !near) {
      box.scrollIntoView({ block: "center" });
      near = true;
    }

    requestAnimationFrame(step);
  };

  requestAnimationFrame(step);
}

/// Where line `match` sits among the rows of `f` as they are drawn.
function rowIndex(v: Viewer, f: DiffFile, match: LineMatch) {
  const rows = diffRows(f, v.context.get(f.path)?.extents ?? null), old = match.side === "old";

  if (v.state.split && !v.state.narrow) {
    return Math.max(0, splitRows(rows).findIndex(r => r.kind === "pair" && (old ? r.left?.k === "-" && r.left.num === match.line : r.right?.num === match.line)));
  }

  return Math.max(0, rows.findIndex(r => r.kind === "line" && (old ? r.k === "-" && r.old === match.line : r.k !== "-" && r.new === match.line)));
}

/// Marks the current file: the one stepped to while it shows, else the one
/// at the top. With `reveal`, the list opens its folders and scrolls to it.
export function markCurrent(v: Viewer, reveal = true) {
  const files = v.files;
  if (!files || v.disposed) return;
  const s = v.state, list = shown(s);
  // The virtualizer keeps the scroll and height from its own events; reading
  // the element's would lay the page out again each time a chunk draws.
  const scroll = v.virtual?.scrollOffset ?? files.scrollTop, height = v.virtual?.scrollRect?.height ?? files.clientHeight;
  const held = itemAt(v, indexOf(v, s.cur));
  let cur = s.cur;

  if (!held || !held.size || !inView(held.start, held.end - GAP, scroll, height)) {
    cur = list[topFile(v, scroll)]?.path ?? null;
  }

  if (cur === s.cur) return;
  // Not flushed: it runs from effects and scrolling, and nothing measures it.
  v.store.setState(reveal ? { cur, revealTree: s.revealTree + 1, ...revealFolders(v, cur) } : { cur });
}

/// The index among the shown files of the one at the top. Scrolling asks on
/// every frame, so it reads only the laid out boxes it needs.
function topFile(v: Viewer, scroll: number) {
  const virtual = v.virtual, list = shown(v.state), at = listIndexes(v, list);
  virtual?.getVirtualItems();
  return currentFile(list.length, index => (virtual?.measurementsCache[at[index]]?.start ?? 0) - GAP, scroll);
}

/// Where each shown file sits in the laid out list.
function listIndexes(v: Viewer, list: readonly DiffFile[]) {
  const kept = v.layout;
  if (kept && kept.shown === list && kept.files === v.state.list) return kept.at;
  const at = list.map(f => indexOf(v, f.path));
  v.layout = { shown: list, files: v.state.list, at };
  return at;
}

/// The folders that hold `path`, opened. A set with none of them stays the
/// same set, so the list keeps its rows.
function revealFolders(v: Viewer, path: string | null) {
  const open = (closed: ReadonlySet<string>) => {
    const kept = [...closed].filter(folder => !path?.startsWith(folder));
    return kept.length === closed.size ? closed : new Set(kept);
  };
  return { closedFolders: open(v.state.closedFolders), filteredClosedFolders: open(v.state.filteredClosedFolders) };
}

/// A file's height before it is measured, with the gap after it.
export function sizeOf(f: DiffFile | undefined, filter: string, folded: ReadonlyMap<string, boolean>) {
  if (!f || !matchesFilter(f, filter)) return 0;
  if (isFolded(f, folded)) return estimateHeight(f, true) + GAP;
  let open = openHeights.get(f);
  if (open === undefined) openHeights.set(f, open = estimateHeight(f, false));
  return open + GAP;
}

/// Each open file's estimated height: every size change lays out the files
/// below it again from these.
const openHeights = new WeakMap<DiffFile, number>();

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
