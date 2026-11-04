import { mark } from "../chat/chats.ts";
import { $, $$, bar, esc, ICON, invoke, S } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
import { clock, dateValue } from "./routines.ts";
import { cap } from "../chat/tools.ts";

export const MEM_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 4a3 3 0 00-3 3 3 3 0 00-2 5 3 3 0 002 5 3 3 0 006 0V4a3 3 0 00-3 0zM15 4a3 3 0 013 3 3 3 0 012 5 3 3 0 01-2 5 3 3 0 01-6 0"/></svg>';
export const CLOCK_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>';
export const PLUS_ICON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';
export const PLAY_ICON = '<svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>';
export const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// ---------- Lists ----------

// Memories, Routines, their grants and Skills are each a list with one row
// selected, a mode, and the error of the last thing that failed.

/// What the list helpers below need of a view's state.
export interface ListState<T> {
  list: T[];
  sel: number;
  error: string;
  loaded: boolean;
}

/// Keeps the selection on a row of the list, `by` rows on.
export function clampSel(s: { list: unknown[]; sel: number }, by = 0) {
  s.sel = Math.max(0, Math.min(s.list.length - 1, s.sel + by));
}

/// Moves the selection for ArrowDown or ArrowUp; false for any other key.
export function arrowKey(e: KeyboardEvent, s: { list: unknown[]; sel: number }) {
  const by = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
  if (!by) return false;
  e.preventDefault();
  clampSel(s, by);
  return true;
}

/// While a question is on screen, Return answers yes and other keys wait.
export function confirmKey(e: KeyboardEvent, yes: () => unknown) {
  if (e.key === "Enter") { e.preventDefault(); yes(); }
}

/// Fills `s.list` from `cmd`, or keeps why it could not.
export async function loadList<T>(s: ListState<T>, cmd: string) {
  try {
    s.list = await invoke<T[]>(cmd);
    s.loaded = true;
    s.error = "";
  } catch (e) {
    s.error = String(e);
  }
}

/// The error under a list's head.
export const errLine = (error: string) => error ? `<div class="derr" style="padding:8px 18px 0">${esc(error)}</div>` : "";

/// What a list shows when it is empty: its icon, a line, what it is for and
/// `extra`, such as a button.
export const emptyPage = (icon: string, title: string, about: string, extra = "", aboutStyle = "") =>
  `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${icon}</span>
      <b style="font-size:14px">${title}</b><span class="dmuted" style="display:block${aboutStyle}">${about}</span>${extra}</div>`;

/// Row `i` asking `question`, with `verb` (Delete, Revoke) and Cancel. The
/// buttons carry `attr` as `<verb>-yes` and `list`.
export const confirmRow = (i: number, question: string, attr: string, verb: string) =>
  `<div class="drt" aria-selected="true" data-i="${i}"><span class="tt"><span style="font-size:14px">${question}</span></span>
          <span class="acts"><button class="dbtn warn" ${attr}="${verb.toLowerCase()}-yes">${verb}</button><button class="dbtn" ${attr}="list">Cancel</button></span></div>`;

/// How many days from `from` to `to`, by the calendar.
export function dayDiff(from: Date, to: Date) {
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  return Math.round((day(to) - day(from)) / 86400000);
}
/// "28 Oct".
export const dayMonth = (at: Date) => `${at.getDate()} ${MONTHS[at.getMonth()]}`;

// ---------- Memories ----------

/// A memory as `list_memories` sends it.
export interface Memory {
  id: string;
  content: string;
  category: string;
  source: "user" | "learned" | "email" | "slack";
  /// `YYYY-MM-DD`, as are the dates it holds from and until.
  updated: string;
  valid_from: string | null;
  valid_until: string | null;
  quote: string | null;
}

/// What a memory is about when it is not the user.
export type MemoryScope =
  | { kind: "machine"; id: string; name: string | null }
  | { kind: "project"; name: string }
  | { kind: "account"; app: string };

/// A read of mail or a calendar that added or confirmed memories.
export interface MemoryRead {
  id: string;
  source: string;
  /// RFC 3339.
  finished_at: string;
  memories: string[];
}

/// A chip over the list. `ids` are the memories it shows; Everything has none.
interface MemFilter {
  key: string;
  label: string;
  about?: string;
  ids?: Set<string>;
  read?: boolean;
}

type MemMode = "list" | "edit" | "clear";
type MemAction = MemMode | "save" | "forget" | "clear-yes";

export const MEM_SOURCE: Record<string, string> = { user: "You told Beep", learned: "Learned in a chat", email: "From your email", slack: "From Slack" };

interface MemState extends ListState<Memory> {
  all: Memory[];
  query: string;
  mode: MemMode;
  /// Each scoped memory's subject, by memory id.
  scopes: Map<string, MemoryScope>;
  reads: MemoryRead[];
  /// all | you | scope:<key> | read:<id>
  filter: string;
}
export const M: MemState = { all: [], list: [], sel: 0, query: "", mode: "list", error: "", loaded: false, scopes: new Map(), reads: [], filter: "all" };
export const SCOPE_ICON: Record<string, string> = {
  machine: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>',
  project: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H9l2 2.5h8.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z"/></svg>',
  account: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="10" r="3"/><path d="M6.5 18.5a6.5 6.5 0 0111 0"/></svg>',
};
/// A subject as the phone writes it: the machine's name, "Project atlas", "Github account".
export function scopeLabel(sc: MemoryScope) {
  if (sc.kind === "machine") return sc.name || "A machine";
  return sc.kind === "project" ? `Project ${sc.name}` : `${cap(sc.app)} account`;
}
export function scopeSubject(sc: MemoryScope) {
  if (sc.kind === "machine") return sc.name ? `the machine ${sc.name}` : "a machine";
  return sc.kind === "project" ? `the project ${sc.name}` : `your ${cap(sc.app)} account`;
}
export const scopeKey = (sc: MemoryScope) => `scope:${sc.kind}:${sc.kind === "machine" ? sc.id : sc.kind === "project" ? sc.name : sc.app}`;
export const scopeTag = (sc: MemoryScope) => `<span class="dscope">${SCOPE_ICON[sc.kind] ?? ""}${esc(scopeLabel(sc))}</span>`;
/// "today", "yesterday" or "28 Oct", for a read's chip.
export function readDay(at: Date) {
  const ago = dayDiff(at, new Date());
  return ago === 0 ? "today" : ago === 1 ? "yesterday" : dayMonth(at);
}
/// The chips over the list: everything, the user, each subject by name, then
/// the latest reads that added or confirmed memories.
export function memFilters(): MemFilter[] {
  const subjects = new Map<string, MemFilter & { ids: Set<string> }>();
  for (const sc of M.scopes.values()) {
    const key = scopeKey(sc);
    subjects.set(key, subjects.get(key) ?? { key, label: scopeLabel(sc), about: `What Beep knows about ${scopeSubject(sc)}.`, ids: new Set() });
  }
  for (const m of M.all) { const sc = M.scopes.get(m.id); if (sc) subjects.get(scopeKey(sc))!.ids.add(m.id); }
  const have = new Set(M.all.map(m => m.id));
  const reads = M.reads.filter(r => r.memories.some(id => have.has(id))).slice(0, 3).map(r => {
    const at = new Date(r.finished_at), day = readDay(at);
    const when = day === "today" || day === "yesterday" ? day : `on ${day}`;
    return { key: `read:${r.id}`, label: `${r.source}, ${day}`, read: true, ids: new Set(r.memories.filter(id => have.has(id))),
      about: `What Beep learned or confirmed reading ${r.source} ${when} at ${clock(at.getHours(), at.getMinutes())}.` };
  });
  const scoped = [...subjects.values()].filter(f => f.ids.size).sort((a, b) => a.label.toLowerCase() < b.label.toLowerCase() ? -1 : 1);
  if (!scoped.length && !reads.length) return [];
  const you = new Set(M.all.filter(m => !M.scopes.has(m.id)).map(m => m.id));
  return [{ key: "all", label: "Everything" }, { key: "you", label: "You", about: "What Beep knows about you.", ids: you }, ...scoped, ...reads];
}

/// "2 Sep" from `YYYY-MM-DD`.
export function shortDate(day: string | null | undefined) {
  const [, m, d] = String(day ?? "").split("-").map(Number);
  return m && d ? `${d} ${MONTHS[m - 1]}` : "";
}

export function renderMemories() {
  S.view = "memories";
  invoke("close_group").catch(() => {});
  Object.assign(M, { sel: 0, query: "", mode: "list", error: "", filter: "all" });
  bar.innerHTML = `<div class="dinput"><input id="mfind" placeholder="Search memories" autocomplete="off" spellcheck="false"><span class="dmuted" id="mcount"></span><kbd>⌘M</kbd></div>
    <div id="mchips" style="border-top:1px solid var(--border)"></div>
    <div class="dlist" id="mlist" role="listbox" aria-label="Memories" style="padding-top:2px"></div><div id="mhints"></div>`;
  const find = $$<HTMLInputElement>("#mfind");
  find.focus();
  find.addEventListener("input", () => { M.query = find.value.trim(); M.sel = 0; M.mode = "list"; drawMemories(); });
  $$("#mlist").addEventListener("click", memClick);
  $$("#mchips").addEventListener("click", e => {
    const chip = (e.target as HTMLElement).closest<HTMLElement>("[data-mf]");
    if (chip) { M.filter = chip.dataset.mf!; M.sel = 0; M.mode = "list"; drawMemories(); $("#mfind")?.focus(); }
  });
  drawMemories();
  loadMemories();
}

export async function loadMemories() {
  try {
    // Subjects and reads only add the chips; a server without them still lists memories.
    const [all, scopes, reads] = await Promise.all([
      invoke<Memory[]>("list_memories"),
      invoke<{ id: string; scope: MemoryScope }[]>("list_memory_scopes").catch(() => []),
      invoke<MemoryRead[]>("list_memory_reads").catch(() => []),
    ]);
    M.all = all;
    M.scopes = new Map(scopes.map(x => [x.id, x.scope]));
    M.reads = reads;
    M.loaded = true;
    M.error = "";
  } catch (e) {
    M.error = String(e);
  }
  if (S.view === "memories") drawMemories();
}

/// One memory's row; the selected one shows its actions, or the editor.
function memRow(m: Memory, i: number, today: string) {
  const on = i === M.sel, ended = m.valid_until && m.valid_until < today;
  const meta = [MEM_SOURCE[m.source] ?? "", shortDate(m.updated)].filter(Boolean).join(" · ")
    + (m.valid_from || m.valid_until ? ` · ${m.valid_from ? shortDate(m.valid_from) : ""} to ${m.valid_until ? shortDate(m.valid_until) : ""}` : "");
  const sc = M.scopes.get(m.id), about = sc ? scopeTag(sc) + " · " : "";
  if (on && M.mode === "edit") {
    return `<div class="dmem" aria-selected="true" data-i="${i}"><div class="top"><textarea id="medit" rows="2" aria-label="Memory">${esc(m.content)}</textarea>
            <span style="font-size:12px;color:var(--muted);flex:none;display:inline-flex;gap:4px;align-items:center">${ICON.ret} save</span></div><div class="m">${about}${esc(meta)}</div></div>`;
  }
  const acts = on ? `<span class="acts"><button class="dbtn" data-m="edit">Edit</button><button class="dbtn warn" data-m="forget">Forget<kbd>⌘⌫</kbd></button></span>` : "";
  return `<div class="dmem${ended ? " ended" : ""}" role="option" aria-selected="${on}" data-i="${i}"><div class="top"><span class="c">${mark(m.content, M.query)}</span>${acts}</div>
            <div class="m">${about}${esc(meta)}</div>${m.quote ? `<q>${mark(m.quote, M.query)}</q>` : ""}</div>`;
}

export function drawMemories() {
  const list = $("#mlist");
  if (!list) return;
  const q = M.query.toLowerCase();
  // A search looks through everything, so the chips go while it is on.
  const filters = q ? [] : memFilters(), picked = filters.find(f => f.key === M.filter) ?? filters[0];
  if (!picked || picked.key === "all") M.filter = "all";
  M.list = M.all.filter(m => (!q || [m.content, m.category, m.quote].some(t => t?.toLowerCase().includes(q))) && (!picked?.ids || picked.ids.has(m.id)));
  const chips = $("#mchips");
  if (chips) {
    const chip = (f: MemFilter) => `<button class="dchip" aria-pressed="${f === picked}" data-mf="${esc(f.key)}">${esc(f.label)}${f.ids ? ` <span style="color:var(--muted)">${f.ids.size}</span>` : ""}</button>`;
    const firstRead = filters.findIndex(f => f.read);
    chips.innerHTML = filters.length ? `<div class="dchips" style="padding:8px 16px 2px">${filters.map((f, i) => (i === firstRead ? '<span class="dsep"></span>' : "") + chip(f)).join("")}</div>`
      + (picked?.about ? `<div class="dmuted" style="padding:2px 18px 4px">${esc(picked.about)}</div>` : "") : "";
  }
  // Grouped by category; the server lists newest first, so each group keeps that order.
  const cats = [...new Set(M.list.map(m => m.category || "Other"))];
  M.list = cats.flatMap(c => M.list.filter(m => (m.category || "Other") === c));
  clampSel(M);
  $$("#mcount").textContent = M.loaded ? (q || M.filter !== "all" ? `${M.list.length} of ${M.all.length}` : String(M.all.length)) : "";
  const today = dateValue(new Date());
  let html = M.error ? `<div class="derr" style="padding:8px 10px">${esc(M.error)}</div>` : "";
  if (M.mode === "clear") {
    html += `<div class="dcard" style="margin:8px"><div style="padding:10px 12px;display:grid;gap:8px">
      <b style="font-size:13.5px">Forget everything?</b><div class="dmuted" style="display:block">Beep will forget all ${M.all.length} memories. This cannot be undone.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" data-m="clear-yes">Forget all</button><button class="dbtn" data-m="list">Cancel</button></div></div></div>`;
  } else if (M.loaded && !M.all.length) {
    html += emptyPage(MEM_ICON, "Nothing remembered yet.", 'Beep learns as you chat, or tell it "remember that…"');
  } else {
    let last: string | undefined;
    M.list.forEach((m, i) => {
      const cat = m.category || "Other";
      if (cat !== last) html += `<div class="dsec" style="border:0;padding-left:10px">${esc(cat)}</div>`;
      last = cat;
      html += memRow(m, i, today);
    });
    if (M.all.length && !M.query && M.filter === "all") html += `<div style="padding:6px 10px 4px"><button class="dbtn warn" data-m="clear">Forget all…</button></div>`;
  }
  list.innerHTML = html;
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  const edit = $<HTMLTextAreaElement>("#medit");
  if (edit) { edit.focus(); edit.setSelectionRange(edit.value.length, edit.value.length); }
  $$("#mhints").innerHTML = M.mode === "edit"
    ? hints([[ICON.ret, "save"], [`⇧${ICON.ret}`, "new line"], ["esc", "cancel"]])
    : hints([["↑↓", "move"], [ICON.ret, "edit"], ["⌘⌫", "forget"], ...(filters.length ? [["tab", "filter"]] : []), ["esc", "back"]]);
}

export function memClick(e: MouseEvent) {
  const target = e.target as HTMLElement;
  const btn = target.closest<HTMLElement>("[data-m]");
  const row = target.closest<HTMLElement>(".dmem[data-i]");
  if (btn) return memAction(btn.dataset.m as MemAction);
  if (row && M.mode !== "edit") {
    M.sel = Number(row.dataset.i);
    M.mode = "list";
    drawMemories();
  }
}

export async function memAction(a: MemAction) {
  const m = M.list[M.sel];
  if (a === "list" || a === "clear" || a === "edit") {
    if (a === "edit" && !m) return;
    M.mode = a;
    return drawMemories();
  }
  try {
    if (a === "save" && m) {
      const content = $$<HTMLTextAreaElement>("#medit").value.trim();
      // Shown at once, so a second Return cannot send it again.
      M.mode = "list";
      if (content && content !== m.content) {
        const old = m.content;
        m.content = content;
        drawMemories();
        try {
          await invoke("update_memory", { id: m.id, content });
        } catch (e) {
          m.content = old;
          throw e;
        }
      }
    } else if (a === "forget" && m) {
      await invoke("delete_memory", { id: m.id });
      M.all = M.all.filter(x => x.id !== m.id);
    } else if (a === "clear-yes") {
      await invoke("clear_memories");
      M.all = [];
    }
    M.error = "";
  } catch (e) {
    M.error = String(e);
  }
  M.mode = "list";
  if (S.view === "memories") drawMemories();
  $("#mfind")?.focus();
}

export function memKeys(e: KeyboardEvent, cmd: boolean) {
  if (M.mode === "edit") {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); memAction("save"); }
    return;
  }
  if (M.mode === "clear") return confirmKey(e, () => memAction("clear-yes"));
  if (e.key === "Tab") {
    e.preventDefault();
    const filters = M.query ? [] : memFilters();
    if (!filters.length) return;
    const at = Math.max(0, filters.findIndex(f => f.key === M.filter));
    M.filter = filters[(at + (e.shiftKey ? filters.length - 1 : 1)) % filters.length].key;
    M.sel = 0;
    drawMemories();
  } else if (arrowKey(e, M)) {
    drawMemories();
  } else if (e.key === "Enter" && !e.isComposing && M.list.length) {
    e.preventDefault();
    memAction("edit");
  } else if (cmd && e.key === "Backspace" && M.list.length) {
    e.preventDefault();
    memAction("forget");
  }
}
