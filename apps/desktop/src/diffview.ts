// The diff viewer: a header with the counts, a filter and Expand all /
// Collapse all; a list of the files at wide widths; each file folding
// under its sticky header, its lines in a unified view that scrolls
// sideways on its own. j and k step between files and x folds the one at
// the top. The session's Changes tab and the sheet a branch's or a task's
// diff opens in both draw it.
import {
  type DiffFile, type DiffView, STATUS_LETTER, changedLabel, currentFile, diffRows, isFolded, leftOutNote,
  inView, matchesFilter, numberWidth, splitPath, stepFile, withLoaded,
} from "./diff-state.ts";

/// Where a diff comes from and what its files can open in.
export interface DiffSource {
  /// Reads the diff; with `only`, just those files in full.
  load: (only: string[], lockfiles: boolean) => Promise<DiffView>;
  /// Opens a file of the diff, when something can show it.
  open?: { label: string; run: (path: string) => void };
}

export interface DiffViewHandle {
  el: HTMLElement;
  reload: () => void;
  focus: () => void;
}

/// Wider than this, the file list shows on the left.
const WIDE = 900;

const SVG = {
  chev: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>',
  to: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>',
  copy: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/></svg>',
  open: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>',
  refresh: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>',
};

type Kid = Node | string | null | undefined | false;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", ...kids: Kid[]): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  for (const k of kids) if (k != null && k !== false) e.append(k);
  return e;
}

/// An icon from the fixed markup above, never from a diff.
function icon(svg: string) {
  const s = el("span");
  s.style.display = "grid";
  s.innerHTML = svg;
  return s;
}

function btn(label: Kid, cls: string, title: string, run: () => void) {
  const b = el("button", `dv-btn${cls ? ` ${cls}` : ""}`, label);
  b.type = "button";
  if (title) b.title = title;
  b.addEventListener("click", e => { e.stopPropagation(); run(); });
  return b;
}

const counts = (a: number, d: number) => [a ? el("i", "dv-plus", `+${a.toLocaleString("en-US")}`) : null, d ? el("i", "dv-minus", `-${d.toLocaleString("en-US")}`) : null];

function reducedMotion() {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function diffView(src: DiffSource): DiffViewHandle {
  const root = el("div", "dv");
  root.tabIndex = -1;
  const st = {
    view: null as DiffView | null,
    error: "",
    loading: false,
    /// Folded or not, as the user left each file while the view is open.
    folded: new Map<string, boolean>(),
    filter: "",
    /// Files being read in full.
    pending: new Set<string>(),
    /// The file j, k and x act on: the one stepped to while it shows,
    /// else the one at the top.
    cur: null as string | null,
    seq: 0,
  };
  /// Each file's section, by path, as last drawn.
  let sections = new Map<string, HTMLElement>();
  const find = el("input", "dv-find");
  find.placeholder = "Filter files";
  find.setAttribute("aria-label", "Filter files");
  find.spellcheck = false;
  find.addEventListener("input", () => { st.filter = find.value; drawFiles(); });
  const sum = el("span", "dv-sum");
  const expand = btn("Expand all", "", "Unfold every file", () => foldAll(false));
  const collapse = btn("Collapse all", "", "Fold every file", () => foldAll(true));
  const refresh = btn(icon(SVG.refresh), "icon", "Read the changes again", () => load());
  const flash = el("span", "dv-flash");
  flash.setAttribute("role", "status");
  let flashTimer = 0;
  const say = (text: string) => {
    flash.textContent = text;
    clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => { flash.textContent = ""; }, 2000);
  };
  const head = el("div", "dv-head", sum, el("span", "dv-sp"), flash, find, expand, collapse, refresh);
  const tree = el("nav", "dv-tree");
  tree.setAttribute("aria-label", "Changed files");
  const files = el("div", "dv-files");
  files.style.position = "relative";
  const body = el("div", "dv-body", tree, files);
  const keys = el("div", "dv-keys");
  keys.innerHTML = "<span><kbd>j</kbd><kbd>k</kbd>next, previous file</span><span><kbd>x</kbd>fold the file</span><span><kbd>/</kbd>filter</span>";
  root.append(head, body, keys);

  new ResizeObserver(() => root.classList.toggle("wide", root.clientWidth >= WIDE)).observe(root);
  files.addEventListener("scroll", () => markCurrent(), { passive: true });
  root.addEventListener("keydown", onKey);

  function shown(): DiffFile[] {
    return st.view ? st.view.files.filter(f => matchesFilter(f, st.filter)) : [];
  }

  function drawHead() {
    sum.replaceChildren();
    const v = st.view;
    if (!v) {
      sum.append(el("b", "", st.loading ? "Reading the changes…" : "Changes"));
    } else {
      const base = el("span", "base", "against ", el("code", "", v.base));
      if (v.merge_base) base.append(" at ", el("code", "", v.merge_base.slice(0, 7)));
      if (v.working_tree) base.append(", uncommitted included");
      sum.append(el("b", "", changedLabel(v)), ...counts(v.additions, v.deletions).filter(x => x != null), base);
    }
    const none = !v?.files.length;
    expand.disabled = collapse.disabled = none;
    find.disabled = none;
    refresh.disabled = st.loading;
    refresh.title = st.loading ? "Reading…" : "Read the changes again";
  }

  function drawFiles() {
    const v = st.view;
    tree.replaceChildren();
    files.replaceChildren();
    sections = new Map();
    if (st.error) files.append(el("div", "dv-err", st.error));
    if (!v) {
      if (st.loading) files.append(el("div", "dv-empty", el("span", "dspin"), "Reading the changes…"));
      return;
    }
    if (!v.files.length) {
      files.append(el("div", "dv-empty", el("b", "", "Nothing changed"),
        el("span", "", v.working_tree ? `The folder matches ${v.base}, with nothing uncommitted.` : `The branch has no commits past ${v.base}.`)));
      return;
    }
    const list = shown();
    if (st.filter.trim()) files.append(el("div", "dv-more", `${list.length} of ${v.files.length} files match`));
    for (const f of list) {
      const { dir, name } = splitPath(f.path);
      const row = el("button", "dv-tr", el("span", `dv-st ${STATUS_LETTER[f.status]}`, STATUS_LETTER[f.status]),
        el("span", "nm", dir ? el("i", "", dir) : null, el("b", "", name)), el("span", "ct", ...counts(f.additions, f.deletions)));
      row.type = "button";
      row.title = f.old_path ? `${f.old_path} -> ${f.path}` : f.path;
      row.dataset.path = f.path;
      row.addEventListener("click", () => jump(f.path));
      tree.append(row);
      const section = fileSection(f);
      sections.set(f.path, section);
      files.append(section);
    }
    if (v.more) files.append(el("div", "dv-more", `And ${v.more.toLocaleString("en-US")} more files not shown.`));
    markCurrent();
  }

  function fileSection(f: DiffFile) {
    const folded = isFolded(f, st.folded);
    const section = el("section", `dv-file${folded ? " closed" : ""}`);
    const path = el("span", "dv-path");
    if (f.old_path) {
      const old = el("span", "old", f.old_path);
      old.title = f.old_path;
      const to = icon(SVG.to);
      to.className = "to";
      to.title = "renamed to";
      path.append(old, to);
    }
    const now = el("span", "", f.path);
    now.title = f.path;
    path.append(now);
    const acts = el("span", "dv-facts", btn(icon(SVG.copy), "icon", "Copy path", () => copy(f.path)));
    if (src.open && f.status !== "deleted") {
      const open = src.open;
      acts.append(btn(icon(SVG.open), "icon", open.label, () => open.run(f.path)));
    }
    const fh = el("div", "dv-fh", el("span", "dv-chev", icon(SVG.chev)), path, el("span", `dv-chip ${f.status}`, f.status),
      el("span", "dv-fct", ...counts(f.additions, f.deletions)), acts);
    fh.tabIndex = 0;
    fh.setAttribute("role", "button");
    fh.setAttribute("aria-expanded", String(!folded));
    fh.title = "Click or press x to fold";
    fh.addEventListener("click", () => { st.cur = f.path; toggle(f.path); markCurrent(); });
    fh.addEventListener("keydown", e => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      toggle(f.path);
    });
    const inner = el("div");
    // The lines are drawn the first time the file opens.
    if (!folded) inner.append(fileBody(f));
    section.append(fh, el("div", "dv-fold", inner));
    return section;
  }

  function fileBody(f: DiffFile): HTMLElement {
    const note = leftOutNote(f);
    if (note) {
      const box = el("div", "dv-note", note.text);
      if (note.load) {
        const busy = st.pending.has(f.path);
        const b = btn(busy ? "Loading…" : note.load, "", "", () => loadFull(f));
        b.disabled = busy;
        box.append(b);
      }
      return box;
    }
    const lines = el("div", "dv-lines");
    lines.style.setProperty("--nw", String(Math.max(2, numberWidth(f))));
    const frag = document.createDocumentFragment();
    for (const r of diffRows(f)) {
      if (r.kind === "hunk") {
        frag.append(el("div", "dv-l hunk", el("span", "dv-g", el("span"), el("span")), el("span", "dv-c", r.text)));
      } else if (r.kind === "gap") {
        frag.append(el("div", "dv-l gap", el("span", "dv-g", el("span"), el("span")),
          el("span", "dv-c", `${r.lines.toLocaleString("en-US")} unchanged ${r.lines === 1 ? "line" : "lines"}`)));
      } else {
        const cls = r.k === "+" ? " add" : r.k === "-" ? " del" : "";
        const eol = r.eol ? el("span", "dv-eol", "no newline at end") : null;
        if (eol) eol.title = "No newline at end of file";
        frag.append(el("div", `dv-l${cls}`, el("span", "dv-g", el("span", "", r.old == null ? "" : String(r.old)), el("span", "", r.new == null ? "" : String(r.new))),
          el("span", "dv-c", r.text, eol)));
      }
    }
    lines.append(frag);
    return el("div", "dv-scroll", lines);
  }

  function setFolded(path: string, folded: boolean) {
    st.folded.set(path, folded);
    const section = sections.get(path);
    const f = st.view?.files.find(x => x.path === path);
    if (!section || !f) return;
    const inner = section.querySelector(".dv-fold > div");
    if (!folded && inner && !inner.childNodes.length) inner.append(fileBody(f));
    section.classList.toggle("closed", folded);
    section.querySelector(".dv-fh")?.setAttribute("aria-expanded", String(!folded));
  }

  function toggle(path: string) {
    const f = st.view?.files.find(x => x.path === path);
    if (!f) return;
    const folding = !isFolded(f, st.folded);
    // Folding a file scrolled past its header keeps its header in view.
    const section = sections.get(path);
    if (folding && section && section.offsetTop < files.scrollTop) files.scrollTop = section.offsetTop - 10;
    setFolded(path, folding);
  }

  function foldAll(folded: boolean) {
    for (const f of shown()) setFolded(f.path, folded);
    if (folded) files.scrollTop = 0;
  }

  function tops() {
    return shown().map(f => (sections.get(f.path)?.offsetTop ?? 0) - 10);
  }

  function markCurrent() {
    const list = shown();
    const held = st.cur ? sections.get(st.cur) : null;
    if (!held || !inView(held.offsetTop, held.offsetTop + held.offsetHeight, files.scrollTop, files.clientHeight)) {
      st.cur = list[currentFile(tops(), files.scrollTop)]?.path ?? null;
    }
    const path = st.cur;
    for (const [p, s] of sections) s.classList.toggle("cur", p === path);
    for (const row of tree.querySelectorAll<HTMLElement>(".dv-tr")) row.classList.toggle("on", row.dataset.path === path);
  }

  function jump(path: string) {
    const section = sections.get(path);
    if (!section) return;
    st.cur = path;
    markCurrent();
    files.scrollTo({ top: section.offsetTop - 10, behavior: reducedMotion() ? "auto" : "smooth" });
  }

  function onKey(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target as HTMLElement;
    if (t === find) {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); find.blur(); root.focus(); }
      return;
    }
    if (t.closest("input, textarea, [contenteditable='true']")) return;
    const list = shown();
    if (["j", "k", "x", "/"].includes(e.key)) e.stopPropagation();
    const at = list.findIndex(f => f.path === st.cur);
    if (e.key === "j" || e.key === "k") {
      const i = stepFile(at, list.length, e.key === "j" ? 1 : -1);
      e.preventDefault();
      if (i != null) jump(list[i].path);
    } else if (e.key === "x") {
      const f = list[at] ?? list[currentFile(tops(), files.scrollTop)];
      e.preventDefault();
      if (f) toggle(f.path);
    } else if (e.key === "/") {
      e.preventDefault();
      find.focus();
    }
  }

  async function copy(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      say("Copied the path");
    } catch (e) {
      say(`Could not copy: ${reasonOf(e)}`);
    }
  }

  async function loadFull(f: DiffFile) {
    if (!st.view || st.pending.has(f.path)) return;
    st.pending.add(f.path);
    redrawFile(f.path);
    try {
      const got = await src.load([f.path], true);
      if (st.view) st.view = withLoaded(st.view, got.files);
    } catch (e) {
      st.error = `Could not read ${f.path}: ${reasonOf(e)}`;
    }
    st.pending.delete(f.path);
    st.folded.set(f.path, false);
    drawFiles();
  }

  /// Draws one file's section again, in place.
  function redrawFile(path: string) {
    const f = st.view?.files.find(x => x.path === path), old = sections.get(path);
    if (!f || !old) return;
    const next = fileSection(f);
    old.replaceWith(next);
    sections.set(path, next);
  }

  async function load() {
    const seq = ++st.seq;
    st.loading = true;
    st.error = "";
    drawHead();
    if (!st.view) drawFiles();
    try {
      const view = await src.load([], false);
      if (seq !== st.seq) return;
      st.view = view;
    } catch (e) {
      if (seq !== st.seq) return;
      st.error = reasonOf(e);
    }
    st.loading = false;
    const keep = files.scrollTop;
    drawHead();
    drawFiles();
    files.scrollTop = keep;
  }

  load();
  return {
    el: root,
    reload: load,
    focus: () => root.focus({ preventScroll: true }),
  };
}

function reasonOf(e: unknown) {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
