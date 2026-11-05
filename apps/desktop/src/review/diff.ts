// The diff viewer: a header with the counts, a filter and Expand all /
// Collapse all; a list of the files at wide widths; each file folding under
// its sticky header, its lines in a unified view that scrolls sideways on its
// own.
import {
  type DiffFile, type DiffRow, type DiffView, type ReviewComment, type SplitCell, type SplitRow, STATUS_LETTER, changedLabel, currentFile, diffRows, isFolded, leftOutNote,
  inView, lineKey, lineLabel, matchesFilter, numberWidth, parseDraft, placeComments, reviewText, splitHeight, splitPath, splitRows, stepFile, withLoaded,
} from "./diff-state.ts";
import { button, el, type Kid } from "../ui/dom.ts";

/// Where a diff comes from and what its files can open in.
export interface DiffSource {
  /// Reads the diff; with `only`, just those files in full.
  load: (only: string[], lockfiles: boolean, uncommitted: boolean) => Promise<DiffView>;
  /// Offers the Uncommitted only toggle, off to begin with.
  uncommitted?: boolean;
  /// Opens a file of the diff, when something can show it.
  open?: { label: string; run: (path: string) => void };
  /// Where review comments are kept and go: `key` names the draft in
  /// storage, `send` delivers the review text and says why it did not ("" when it went).
  review: { key: string; send: (text: string) => Promise<string> };
}

export interface DiffViewHandle {
  el: HTMLElement;
  reload: () => void;
  focus: () => void;
}

/// Wider than this, the file list shows on the left.
const WIDE = 900;
/// At this width or less the lines stay unified, as the CSS hides the button.
const NARROW = 640;

/// Rows per chunk, how far past the viewport chunks stay drawn, and the width
/// of one character of the code font.
const CHUNK = 25;
const MARGIN = 500;
const CH = 7.2;

/// A run of rows, drawn only while it is near the viewport.
interface Chunk {
  box: HTMLElement;
  draw: () => HTMLElement[];
  live: boolean;
}

const SPLIT_KEY = "beep.diffSplit";

function loadSplit() {
  try { return localStorage.getItem(SPLIT_KEY) === "1"; } catch { return false; }
}

function saveSplit(on: boolean) {
  try { localStorage.setItem(SPLIT_KEY, on ? "1" : "0"); } catch { /* kept for this launch only */ }
}

function loadDraft(id: string) {
  try { return parseDraft(localStorage.getItem(`beep.diffReview.${id}`)); } catch { return []; }
}

function saveDraft(id: string, draft: readonly ReviewComment[]) {
  try {
    if (draft.length) localStorage.setItem(`beep.diffReview.${id}`, JSON.stringify(draft));
    else localStorage.removeItem(`beep.diffReview.${id}`);
  } catch { /* kept for this launch only */ }
}

const SVG = {
  chev: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>',
  to: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>',
  copy: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/></svg>',
  open: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>',
  refresh: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>',
};

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

function hunkRow(text: string) {
  return el("div", "dv-l hunk", el("span", "dv-g", el("span"), el("span")), el("span", "dv-c", text));
}

function gapRow(n: number) {
  return el("div", "dv-l gap", el("span", "dv-g", el("span"), el("span")),
    el("span", "dv-c", `${n.toLocaleString("en-US")} unchanged ${n === 1 ? "line" : "lines"}`));
}

function textCell(text: string, eol: boolean) {
  const mark = eol ? el("span", "dv-eol", "no newline at end") : null;
  if (mark) mark.title = "No newline at end of file";
  return el("span", "dv-c", text, mark);
}

function lineRow(k: "+" | "-" | " ", old: number | null, now: number | null, text: string, eol: boolean) {
  const cls = k === "+" ? " add" : k === "-" ? " del" : "";
  return el("div", `dv-l${cls}`, el("span", "dv-g", el("span", "", old == null ? "" : String(old)), el("span", "", now == null ? "" : String(now))),
    textCell(text, eol));
}

/// One side of a side by side row, empty where that side has no line.
function cell(c: SplitCell | null) {
  if (!c) return el("div", "dv-l none", el("span", "dv-g", el("span")), el("span", "dv-c"));
  const cls = c.k === "+" ? " add" : c.k === "-" ? " del" : "";
  return el("div", `dv-l${cls}`, el("span", "dv-g", el("span", "", String(c.num))), textCell(c.text, c.eol));
}

const counts = (a: number, d: number) => [a ? el("i", "dv-plus", `+${a.toLocaleString("en-US")}`) : null, d ? el("i", "dv-minus", `-${d.toLocaleString("en-US")}`) : null];

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
    /// Side by side is chosen; it shows only while the viewer is not narrow.
    split: loadSplit(),
    narrow: false,
    /// Only the uncommitted changes, against HEAD.
    uncommitted: false,
    /// Files being read in full.
    pending: new Set<string>(),
    /// The file j, k and x act on: the one stepped to while it shows,
    /// else the one at the top.
    cur: null as string | null,
    seq: 0,
    /// The review comments waiting to be sent.
    draft: loadDraft(src.review.key),
    sending: false,
  };
  let placedCache: ReturnType<typeof placeComments> | null = null;
  const placed = () => placedCache ??= placeComments(st.view?.files ?? [], st.draft);
  /// Each file's section, by path, as last drawn.
  let sections = new Map<string, HTMLElement>();
  /// The chunks of rows of every open file.
  const chunks = new Set<Chunk>();
  const find = el("input", "dv-find");
  find.placeholder = "Filter files";
  find.setAttribute("aria-label", "Filter files");
  find.spellcheck = false;
  const matchNote = el("div", "dv-more");
  find.addEventListener("input", () => { st.filter = find.value; applyFilter(); files.scrollTop = 0; });
  const sum = el("span", "dv-sum");
  const splitBtn = btn("Side by side", "dv-splitbtn", "Show the lines side by side", () => {
    st.split = !st.split;
    saveSplit(st.split);
    splitBtn.setAttribute("aria-pressed", String(st.split));
    redrawBodies();
  });
  splitBtn.setAttribute("aria-pressed", String(st.split));
  const uncommittedBtn = btn("Uncommitted only", "", "Show uncommitted changes only, without the branch's commits", () => {
    st.uncommitted = !st.uncommitted;
    uncommittedBtn.setAttribute("aria-pressed", String(st.uncommitted));
    load();
  });
  uncommittedBtn.setAttribute("aria-pressed", "false");
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
  const cmts = el("span", "dv-cmts");
  const send = button("Send review", "dbtn pri small", () => sendReview());
  const lostBox = el("div", "dv-lost");
  const head = el("div", "dv-head", sum, el("span", "dv-sp"), flash, cmts, send, find, ...(src.uncommitted ? [uncommittedBtn] : []), splitBtn, expand, collapse, refresh);
  const tree = el("nav", "dv-tree");
  tree.setAttribute("aria-label", "Changed files");
  const files = el("div", "dv-files");
  files.style.position = "relative";
  const body = el("div", "dv-body", tree, files);
  const keys = el("div", "dv-keys");
  keys.innerHTML = "<span><kbd>j</kbd><kbd>k</kbd>next, previous file</span><span><kbd>x</kbd>fold the file</span><span><kbd>/</kbd>filter</span>";
  root.append(head, body, keys);

  new ResizeObserver(() => {
    root.classList.toggle("wide", root.clientWidth >= WIDE);
    const narrow = root.clientWidth <= NARROW;
    if (narrow === st.narrow) return;
    st.narrow = narrow;
    if (st.split) redrawBodies();
  }).observe(root);
  new ResizeObserver(() => windowChunks()).observe(files);
  files.addEventListener("scroll", () => { windowChunks(); markCurrent(); }, { passive: true });
  files.addEventListener("click", onGutter);
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
      const base = el("span", "base", "against ", el("code", "", st.uncommitted ? "HEAD" : v.base));
      if (v.merge_base && !st.uncommitted) base.append(" at ", el("code", "", v.merge_base.slice(0, 7)));
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
    placedCache = null;
    if (st.error) files.append(el("div", "dv-err", st.error));
    files.append(lostBox);
    if (!v) {
      if (st.loading) files.append(el("div", "dv-empty", el("span", "dspin"), "Reading the changes…"));
      return;
    }
    if (!v.files.length) {
      files.append(el("div", "dv-empty", el("b", "", "Nothing changed"),
        el("span", "", v.working_tree ? `The folder matches ${v.base}, with nothing uncommitted.` : `The branch has no commits past ${v.base}.`)));
      drawReview();
      return;
    }
    files.append(matchNote);
    for (const f of v.files) {
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
    applyFilter();
    drawReview();
  }

  /// Hides the files that do not match, without drawing any again.
  function applyFilter() {
    const v = st.view;
    if (!v) return;
    v.files.forEach((f, i) => {
      const hide = !matchesFilter(f, st.filter);
      sections.get(f.path)!.hidden = hide;
      tree.children[i].toggleAttribute("hidden", hide);
    });
    matchNote.hidden = !st.filter.trim();
    matchNote.textContent = `${shown().length} of ${v.files.length} files match`;
    windowChunks();
    markCurrent();
  }

  function fileSection(f: DiffFile) {
    const folded = isFolded(f, st.folded);
    const section = el("section", `dv-file${folded ? " closed" : ""}`);
    section.dataset.path = f.path;
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
    // Comments on the file itself sit outside the fold, so a folded, deleted or binary file shows them.
    const fcs = el("div", "dv-fcs");
    const mine = placed().files.get(f.path);
    if (mine) fcs.append(...[...mine.file, ...mine.top].map(c => commentEl(c, lineLabel(c))));
    const acts = el("span", "dv-facts", btn("Comment", "", "Comment on this file", () => {
      addBox({ path: f.path, side: "new", line: null, text: "" }, "", "Comment on this file", box => fcs.append(box));
    }), btn(icon(SVG.copy), "icon", "Copy path", () => copy(f.path)));
    if (src.open && f.status !== "deleted") {
      const open = src.open;
      acts.append(btn(icon(SVG.open), "icon", open.label, () => open.run(f.path)));
    }
    const fh = el("div", "dv-fh", el("span", "dv-chev", icon(SVG.chev)), path, el("span", `dv-chip ${f.status}`, f.status),
      el("span", "dv-fct", ...counts(f.additions, f.deletions)), el("span", "dv-fcm", countLabel(f.path)), acts);
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
    section.append(fh, fcs, el("div", "dv-fold", inner));
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
    const split = st.split && !st.narrow;
    const lines = el("div", split ? "dv-lines split" : "dv-lines");
    lines.style.setProperty("--nw", String(Math.max(2, numberWidth(f))));
    const rows = diffRows(f);
    // The widest line is known up front, so the sideways scroll does not
    // change as rows come and go.
    lines.style.setProperty("--len", String(rows.reduce((m, r) => (r.kind === "line" ? Math.max(m, r.text.length) : m), 0)));
    // Line comments are looked up as a chunk draws, so they follow the draft.
    const nodes = (r: DiffRow | SplitRow): HTMLElement[] => {
      if (r.kind === "hunk") return [hunkRow(r.text)];
      if (r.kind === "gap") return [gapRow(r.lines)];
      const lc = placed().files.get(f.path)?.lines;
      const saved = (...keys: (string | null)[]) => (lc?.size ? keys.flatMap(k => (k && lc.get(k)) || []).map(c => commentEl(c, "")) : []);
      if (r.kind === "pair") {
        return [el("div", "dv-pair", cell(r.left), cell(r.right)),
          ...saved(r.left?.k === "-" ? lineKey("old", r.left.num) : null, r.right ? lineKey("new", r.right.num) : null)];
      }
      return [lineRow(r.k, r.old, r.new, r.text, r.eol), ...saved(r.k === "-" ? lineKey("old", r.old!) : lineKey("new", r.new!))];
    };
    // Rows are drawn in chunks, only while they are near the viewport; a
    // chunk off screen is an empty box as tall as it last was.
    const cols = Math.floor(((files.clientWidth || 900) / 2 - Math.max(2, numberWidth(f)) * CH - 50) / CH);
    const items: (DiffRow | SplitRow)[] = split ? splitRows(rows) : rows;
    for (let i = 0; i < items.length; i += CHUNK) {
      const slice = items.slice(i, i + CHUNK);
      const box = el("div", "dv-chunk");
      box.style.height = `${split ? splitHeight(slice as SplitRow[], cols) : slice.reduce((h, r) => h + (r.kind === "gap" ? 22 : 19), 0)}px`;
      chunks.add({ box, draw: () => slice.flatMap(nodes), live: false });
      lines.append(box);
    }
    return el("div", "dv-scroll", lines);
  }

  /// Draws the chunks near the viewport and empties the far ones. A chunk
  /// that is drawn above the viewport and differs from its estimate would
  /// push the lines down (WebKit has no scroll anchoring), so the scroll
  /// moves by the difference.
  function windowChunks() {
    // A drawn chunk can move the next ones, so go again until nothing changes.
    for (let pass = 0; pass < 3 && windowStep(); pass++);
  }

  /// Whether the user's selection reaches into `box`: emptying it would lose the text.
  function selected(box: HTMLElement) {
    const sel = document.getSelection();
    return !!sel && !sel.isCollapsed && sel.containsNode(box, true);
  }

  function windowStep() {
    let changed = false;
    const view = files.getBoundingClientRect();
    const lo = view.top - MARGIN, hi = view.bottom + MARGIN;
    for (const c of chunks) {
      if (!c.box.isConnected) { chunks.delete(c); continue; }
      if (!c.box.getClientRects().length) continue;
      const r = c.box.getBoundingClientRect();
      const want = r.bottom > lo && r.top < hi;
      if (want === c.live) continue;
      if (want) {
        const was = c.box.offsetHeight;
        c.box.replaceChildren(...c.draw());
        c.box.style.height = "";
        if (r.bottom <= view.top) files.scrollTop += c.box.offsetHeight - was;
      } else if (!c.box.querySelector(".dv-cmt.open") && !selected(c.box)) {
        const h = c.box.offsetHeight;
        c.box.replaceChildren();
        c.box.style.height = `${h}px`;
      } else continue;
      c.live = want;
      changed = true;
    }
    return changed;
  }

  function countLabel(path: string) {
    const n = st.draft.filter(c => c.path === path).length;
    return n ? `${n} ${n === 1 ? "comment" : "comments"}` : "";
  }

  /// The header's count and Send button, each folded file's count and the
  /// comments on files the diff no longer has.
  function drawReview() {
    const n = st.draft.length;
    cmts.textContent = `${n} ${n === 1 ? "comment" : "comments"}`;
    cmts.hidden = send.hidden = !n;
    send.disabled = st.sending;
    for (const [path, s] of sections) {
      const t = s.querySelector(".dv-fcm");
      if (t) t.textContent = countLabel(path);
    }
    const lost = st.view ? placed().lost : [];
    lostBox.replaceChildren(...(lost.length ? [el("div", "dv-lh", "On files no longer in the changes"), ...lost.map(c => commentEl(c, `${c.path} ${lineLabel(c)}`.trim()))] : []));
  }

  /// The draft changed: keep it and show it.
  function changed() {
    placedCache = null;
    saveDraft(src.review.key, st.draft);
    drawReview();
  }

  /// A comment as saved, with Edit and remove.
  function commentEl(c: ReviewComment, label: string) {
    const place = c.line == null ? "Comment on this file" : "Comment on this line";
    const row = el("div", "dv-cmt", el("div", "b", label ? el("code", "", label) : null, label ? " " : null, c.text),
      el("div", "a", el("button", "", "Edit"), el("button", "", "×")));
    const [edit, drop] = row.querySelectorAll("button");
    edit.type = drop.type = "button";
    drop.setAttribute("aria-label", "Remove the comment");
    edit.addEventListener("click", () => {
      const box = writeBox(c.text, place, text => { c.text = text; changed(); box.replaceWith(commentEl(c, label)); }, () => box.replaceWith(row));
      row.replaceWith(box);
      focusBox(box);
    });
    drop.addEventListener("click", () => {
      st.draft.splice(st.draft.indexOf(c), 1);
      row.remove();
      changed();
    });
    return row;
  }

  /// The box to write in: Add (or Cmd+Enter) gives the text, Cancel and Esc drop it.
  function writeBox(text: string, place: string, add: (text: string) => void, drop: () => void) {
    const ta = el("textarea");
    ta.rows = 2;
    ta.value = text;
    ta.placeholder = place;
    ta.setAttribute("aria-label", place);
    const done = () => { const t = ta.value.trim(); if (t) add(t); };
    ta.addEventListener("keydown", e => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); drop(); root.focus({ preventScroll: true }); }
      else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); done(); }
    });
    return el("div", "dv-cmt open", ta, el("div", "a", button("Add", "dbtn small pri", done), button("Cancel", "dbtn small quiet", drop)));
  }

  function focusBox(box: HTMLElement) {
    box.querySelector("textarea")?.focus({ preventScroll: true });
  }

  /// A new comment `c`: its box goes where `put` says, and Add keeps it.
  function addBox(c: ReviewComment, label: string, place: string, put: (box: HTMLElement) => void) {
    const box = writeBox("", place, text => {
      c.text = text;
      st.draft.push(c);
      changed();
      box.replaceWith(commentEl(c, label));
    }, () => box.remove());
    put(box);
    focusBox(box);
  }

  /// A click on a line's number opens a box under its row (under the pair, side by side).
  function onGutter(e: MouseEvent) {
    const g = (e.target as Element).closest<HTMLElement>(".dv-g");
    const row = g?.parentElement;
    const path = g?.closest<HTMLElement>(".dv-file")?.dataset.path;
    if (!row || !path || row.matches(".hunk, .gap, .none")) return;
    const pair = row.parentElement?.classList.contains("dv-pair") ? row.parentElement : null;
    const del = row.classList.contains("del");
    // On an unchanged line the right cell has the new number.
    const nums = (!del && pair ? pair.lastElementChild! : row).firstElementChild!.children;
    const line = Number((del ? nums[0] : nums[nums.length - 1]).textContent);
    if (!line) return;
    let at: Element = pair ?? row;
    while (at.nextElementSibling?.classList.contains("dv-cmt")) at = at.nextElementSibling;
    addBox({ path, side: del ? "old" : "new", line, text: "" }, "", "Comment on this line", box => at.after(box));
  }

  async function sendReview() {
    if (st.sending || !st.draft.length) return;
    const sent = st.draft.slice();
    st.sending = true;
    st.error = "";
    drawReview();
    const why = await src.review.send(reviewText(st.view?.files ?? [], sent));
    st.sending = false;
    if (why) {
      st.error = why;
    } else {
      st.draft = st.draft.filter(c => !sent.includes(c));
      saveDraft(src.review.key, st.draft);
    }
    const keep = files.scrollTop;
    drawHead();
    drawFiles();
    files.scrollTop = keep;
    root.focus({ preventScroll: true });
  }

  /// Draws the lines of every open file again for the current mode; folded
  /// files are emptied and draw when they open.
  function redrawBodies() {
    const keep = files.scrollTop;
    for (const [path, section] of sections) {
      const f = st.view?.files.find(x => x.path === path);
      const inner = section.querySelector(".dv-fold > div");
      if (!f || !inner || !inner.childNodes.length) continue;
      inner.replaceChildren(...(isFolded(f, st.folded) ? [] : [fileBody(f)]));
    }
    files.scrollTop = keep;
    windowChunks();
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
    windowChunks();
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
    for (const [p, s] of sections) s.classList.toggle("cur", p === st.cur);
    for (const row of tree.querySelectorAll<HTMLElement>(".dv-tr")) row.classList.toggle("on", row.dataset.path === st.cur);
  }

  function jump(path: string) {
    const section = sections.get(path);
    if (!section) return;
    st.cur = path;
    markCurrent();
    files.scrollTo({ top: section.offsetTop - 10, behavior: typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
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
      const got = await src.load([f.path], true, st.uncommitted);
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
    windowChunks();
  }

  async function load() {
    const seq = ++st.seq;
    st.loading = true;
    st.error = "";
    drawHead();
    if (!st.view) drawFiles();
    try {
      const view = await src.load([], false, st.uncommitted);
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

  drawReview();
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
