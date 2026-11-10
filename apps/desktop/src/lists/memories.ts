// lists/: Memories, Routines and Skills, each a list with one row picked.
// Entry: this file, which holds the shared list parts.
import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import { $, $$, S } from "../bar/core.ts";
import { hintModel } from "../bar/hints.ts";
import { clock, dateValue } from "./routines.ts";
import { cap } from "../chat/tools.ts";
import { dayDiff, esc } from "../ui/format.ts";
import { ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { confirmSheet, openMenu } from "../ui/menu.ts";

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
export type MemAction = MemMode | "save" | "forget" | "clear-yes";

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

let memoriesPage: BarPageSlot | null = null;
export function renderMemories() {
  S.view = "memories";
  invoke("close_group").catch(() => {});
  Object.assign(M, { sel: 0, query: "", mode: "list", error: "", filter: "all" });
  memoriesPage = openBarPage(() => { memoriesPage = null; });
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

export function drawMemories() {
  const page = memoriesPage;
  if (!page || S.view !== "memories") return;
  const q = M.query.toLowerCase();
  // A search looks through everything, so the chips go while it is on.
  const filters = q ? [] : memFilters(), picked = filters.find(f => f.key === M.filter) ?? filters[0];
  if (!picked || picked.key === "all") M.filter = "all";
  M.list = M.all.filter(m => (!q || [m.content, m.category, m.quote].some(t => t?.toLowerCase().includes(q))) && (!picked?.ids || picked.ids.has(m.id)));
  // Grouped by category; the server lists newest first, so each group keeps that order.
  const byCategory = new Map<string, Memory[]>();

  for (const m of M.list) {
    const cat = m.category || "Other";
    const group = byCategory.get(cat);
    if (group) group.push(m); else byCategory.set(cat, [m]);
  }

  M.list = [...byCategory.values()].flat();
  clampSel(M);
  const count = M.loaded ? (q || M.filter !== "all" ? `${M.list.length} of ${M.all.length}` : String(M.all.length)) : "";
  const keys = M.mode === "edit"
    ? hintModel([[ICON.ret, "save"], [`⇧${ICON.ret}`, "new line"], ["esc", "cancel"]])
    : hintModel([["↑↓", "move"], [ICON.ret, "edit"], ["⌘⌫", "forget"], ...(filters.length ? [["tab", "filter"]] : []), ["esc", "back"]]);
  flushSync(() => page.show({ view: "memories", props: {
    filters, picked: M.filter, count, hints: keys, memories: M.list, selected: M.sel, mode: M.mode, scopes: M.scopes,
    query: M.query, today: dateValue(new Date()), loaded: M.loaded, total: M.all.length, error: M.error,
    search: (value: string) => { M.query = value.trim(); M.sel = 0; M.mode = "list"; drawMemories(); },
    filter: (key: string) => { M.filter = key; M.sel = 0; M.mode = "list"; drawMemories(); },
    act: (a: MemAction) => void memAction(a), pick: pickMemory, menu: memMenu,
  } }));

}

/// A memory row's right-click menu: Edit, and Forget, which asks first.
function memMenu(e: MouseEvent, index: number) {
  if (M.mode === "edit") return;
  M.sel = index;
  const m = M.list[M.sel];
  if (!m) return;
  M.mode = "list";
  drawMemories();
  openMenu(e, "Memory", [
    { label: "Edit", on: () => void memAction("edit") },
    {
      label: "Forget…",
      warn: true,
      on: () => confirmSheet({
        title: "Forget this memory?",
        text: m.content,
        action: "Forget",
        run: () => memAction("forget"),
      }),
    },
  ]);
}

/// A click on a row selects it, unless a memory is being edited.
function pickMemory(index: number) {
  if (M.mode === "edit") return;
  M.sel = index;
  M.mode = "list";
  drawMemories();
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
    return drawMemories();
  }

  if (arrowKey(e, M)) return drawMemories();
  if (e.key === "Enter" && !e.isComposing && M.list.length) { e.preventDefault(); return memAction("edit"); }
  if (cmd && e.key === "Backspace" && M.list.length) { e.preventDefault(); return memAction("forget"); }
}
