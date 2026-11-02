import { mark } from "./chats.ts";
import { $, bar, esc, ICON, invoke, S } from "./core.ts";
import { hints } from "./hints.ts";
import { clock } from "./routines.ts";
import { cap } from "./tools.ts";

// ---------- Memories ----------

export const MEM_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 4a3 3 0 00-3 3 3 3 0 00-2 5 3 3 0 002 5 3 3 0 006 0V4a3 3 0 00-3 0zM15 4a3 3 0 013 3 3 3 0 012 5 3 3 0 01-2 5 3 3 0 01-6 0"/></svg>';
export const CLOCK_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>';
export const PLUS_ICON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';
export const PLAY_ICON = '<svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>';
export const MEM_SOURCE = { user: "You told Beep", learned: "Learned in a chat", email: "From your email", slack: "From Slack" };
export const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// mode: list | edit | clear. filter: all | you | scope:<key> | read:<id>
export const M = { all: [], list: [], sel: 0, query: "", mode: "list", error: "", loaded: false, scopes: new Map(), reads: [], filter: "all" };
export const SCOPE_ICON = {
  machine: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>',
  project: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H9l2 2.5h8.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z"/></svg>',
  account: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="10" r="3"/><path d="M6.5 18.5a6.5 6.5 0 0111 0"/></svg>',
};
/// A subject as the phone writes it: the machine's name, "Project atlas", "Github account".
export function scopeLabel(sc) {
  if (sc.kind === "machine") return sc.name || "A machine";
  return sc.kind === "project" ? `Project ${sc.name}` : `${cap(sc.app)} account`;
}
export function scopeSubject(sc) {
  if (sc.kind === "machine") return sc.name ? `the machine ${sc.name}` : "a machine";
  return sc.kind === "project" ? `the project ${sc.name}` : `your ${cap(sc.app)} account`;
}
export const scopeKey = sc => `scope:${sc.kind}:${sc.id ?? sc.name ?? sc.app}`;
export const scopeTag = sc => `<span class="dscope">${SCOPE_ICON[sc.kind] ?? ""}${esc(scopeLabel(sc))}</span>`;
/// "today", "yesterday" or "28 Oct", for a read's chip.
export function readDay(at) {
  const day = d => new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const ago = Math.round((day(new Date()) - day(at)) / 86400000);
  return ago === 0 ? "today" : ago === 1 ? "yesterday" : `${at.getDate()} ${MONTHS[at.getMonth()]}`;
}
/// The chips over the list: everything, the user, each subject by name, then
/// the latest reads that added or confirmed memories. Empty when there is
/// nothing to pick between.
export function memFilters() {
  const subjects = new Map();
  for (const sc of M.scopes.values()) {
    const key = scopeKey(sc), f = subjects.get(key) ?? { key, label: scopeLabel(sc), about: `What Beep knows about ${scopeSubject(sc)}.`, ids: new Set() };
    subjects.set(key, f);
  }
  for (const m of M.all) { const sc = M.scopes.get(m.id); if (sc) subjects.get(scopeKey(sc)).ids.add(m.id); }
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
export function shortDate(day) {
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
  const find = $("#mfind");
  find.focus();
  find.addEventListener("input", () => { M.query = find.value.trim(); M.sel = 0; M.mode = "list"; drawMemories(); });
  $("#mlist").addEventListener("click", memClick);
  $("#mchips").addEventListener("click", e => {
    const chip = e.target.closest("[data-mf]");
    if (chip) { M.filter = chip.dataset.mf; M.sel = 0; M.mode = "list"; drawMemories(); $("#mfind")?.focus(); }
  });
  drawMemories();
  loadMemories();
}

export async function loadMemories() {
  try {
    // Subjects and reads only add the chips; a server without them still lists memories.
    const [all, scopes, reads] = await Promise.all([invoke("list_memories"), invoke("list_memory_scopes").catch(() => []), invoke("list_memory_reads").catch(() => [])]);
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
  const list = $("#mlist");
  if (!list) return;
  const q = M.query.toLowerCase();
  // A search looks through everything, so the chips go while it is on.
  const filters = q ? [] : memFilters(), picked = filters.find(f => f.key === M.filter) ?? filters[0];
  if (!picked || picked.key === "all") M.filter = "all";
  M.list = M.all.filter(m => (!q || [m.content, m.category, m.quote].some(t => t?.toLowerCase().includes(q))) && (!picked?.ids || picked.ids.has(m.id)));
  const chips = $("#mchips");
  if (chips) {
    const chip = f => `<button class="dchip" aria-pressed="${f === picked}" data-mf="${esc(f.key)}">${esc(f.label)}${f.ids ? ` <span style="color:var(--muted)">${f.ids.size}</span>` : ""}</button>`;
    const firstRead = filters.findIndex(f => f.read);
    chips.innerHTML = filters.length ? `<div class="dchips" style="padding:8px 16px 2px">${filters.map((f, i) => (i === firstRead ? '<span class="dsep"></span>' : "") + chip(f)).join("")}</div>`
      + (picked?.about ? `<div class="dmuted" style="padding:2px 18px 4px">${esc(picked.about)}</div>` : "") : "";
  }
  // Grouped by category; the server lists newest first, so each group keeps that order.
  const cats = [...new Set(M.list.map(m => m.category || "Other"))];
  M.list = cats.flatMap(c => M.list.filter(m => (m.category || "Other") === c));
  M.sel = Math.max(0, Math.min(M.sel, M.list.length - 1));
  $("#mcount").textContent = M.loaded ? (q || M.filter !== "all" ? `${M.list.length} of ${M.all.length}` : String(M.all.length)) : "";
  const now = new Date();
  const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  let html = M.error ? `<div class="derr" style="padding:8px 10px">${esc(M.error)}</div>` : "";
  if (M.mode === "clear") {
    html += `<div class="dcard" style="margin:8px"><div style="padding:10px 12px;display:grid;gap:8px">
      <b style="font-size:13.5px">Forget everything?</b><div class="dmuted" style="display:block">Beep will forget all ${M.all.length} memories. This cannot be undone.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" data-m="clear-yes">Forget all</button><button class="dbtn" data-m="list">Cancel</button></div></div></div>`;
  } else if (M.loaded && !M.all.length) {
    html += `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${MEM_ICON}</span>
      <b style="font-size:14px">Nothing remembered yet.</b><span class="dmuted" style="display:block">Beep learns as you chat, or tell it "remember that…"</span></div>`;
  } else {
    let last;
    M.list.forEach((m, i) => {
      const cat = m.category || "Other";
      if (cat !== last) html += `<div class="dsec" style="border:0;padding-left:10px">${esc(cat)}</div>`;
      last = cat;
        const on = i === M.sel, ended = m.valid_until && m.valid_until < today;
        const meta = [MEM_SOURCE[m.source] ?? "", shortDate(m.updated)].filter(Boolean).join(" · ")
          + (m.valid_from || m.valid_until ? ` · ${m.valid_from ? shortDate(m.valid_from) : ""} to ${m.valid_until ? shortDate(m.valid_until) : ""}` : "");
        const sc = M.scopes.get(m.id), about = sc ? scopeTag(sc) + " · " : "";
        if (on && M.mode === "edit") {
          html += `<div class="dmem" aria-selected="true" data-i="${i}"><div class="top"><textarea id="medit" rows="2" aria-label="Memory">${esc(m.content)}</textarea>
            <span style="font-size:12px;color:var(--muted);flex:none;display:inline-flex;gap:4px;align-items:center">${ICON.ret} save</span></div><div class="m">${about}${esc(meta)}</div></div>`;
        } else {
          const acts = on ? `<span class="acts"><button class="dbtn" data-m="edit">Edit</button><button class="dbtn warn" data-m="forget">Forget<kbd>⌘⌫</kbd></button></span>` : "";
          html += `<div class="dmem${ended ? " ended" : ""}" role="option" aria-selected="${on}" data-i="${i}"><div class="top"><span class="c">${mark(m.content, M.query)}</span>${acts}</div>
            <div class="m">${about}${esc(meta)}</div>${m.quote ? `<q>${mark(m.quote, M.query)}</q>` : ""}</div>`;
      }
    });
    if (M.all.length && !M.query && M.filter === "all") html += `<div style="padding:6px 10px 4px"><button class="dbtn warn" data-m="clear">Forget all…</button></div>`;
  }
  list.innerHTML = html;
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  const edit = $("#medit");
  if (edit) { edit.focus(); edit.setSelectionRange(edit.value.length, edit.value.length); }
  $("#mhints").innerHTML = M.mode === "edit"
    ? hints([[ICON.ret, "save"], [`⇧${ICON.ret}`, "new line"], ["esc", "cancel"]])
    : hints([["↑↓", "move"], [ICON.ret, "edit"], ["⌘⌫", "forget"], ...(filters.length ? [["tab", "filter"]] : []), ["esc", "back"]]);
}

export function memClick(e) {
  const btn = e.target.closest("[data-m]");
  const row = e.target.closest(".dmem[data-i]");
  if (btn) return memAction(btn.dataset.m);
  if (row && M.mode !== "edit") {
    M.sel = Number(row.dataset.i);
    M.mode = "list";
    drawMemories();
  }
}

export async function memAction(a) {
  const m = M.list[M.sel];
  if (a === "list" || a === "clear" || a === "edit") {
    if (a === "edit" && !m) return;
    M.mode = a;
    return drawMemories();
  }
  try {
    if (a === "save" && m) {
      const content = $("#medit").value.trim();
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

export function memKeys(e, cmd) {
  if (M.mode === "edit") {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); memAction("save"); }
    return;
  }
  if (M.mode === "clear") {
    if (e.key === "Enter") { e.preventDefault(); memAction("clear-yes"); }
    return;
  }
  if (e.key === "Tab") {
    e.preventDefault();
    const filters = M.query ? [] : memFilters();
    if (!filters.length) return;
    const at = Math.max(0, filters.findIndex(f => f.key === M.filter));
    M.filter = filters[(at + (e.shiftKey ? filters.length - 1 : 1)) % filters.length].key;
    M.sel = 0;
    drawMemories();
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    M.sel = Math.max(0, Math.min(M.list.length - 1, M.sel + (e.key === "ArrowDown" ? 1 : -1)));
    drawMemories();
  } else if (e.key === "Enter" && !e.isComposing && M.list.length) {
    e.preventDefault();
    memAction("edit");
  } else if (cmd && e.key === "Backspace" && M.list.length) {
    e.preventDefault();
    memAction("forget");
  }
}

