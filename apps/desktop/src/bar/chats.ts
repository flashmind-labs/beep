import { openInWindow } from "./chat.ts";
import { $, $$, bar, type ChatRow, type ChatsMode, esc, G, h, ICON, invoke, live, PANE, S } from "./core.ts";
import { authorName, type GroupSummary, renderGroup, renderInvite } from "./groups.ts";
import { hints } from "./hints.ts";
import { openSession } from "./history.ts";
import { LIVE_CHATS, VOICE_ICON } from "./live.ts";
import { RESEARCHES } from "./researches.ts";
import { dayDiff } from "./memories.ts";

export type { GroupSummary } from "./groups.ts";

// ---------- Chats ----------

/// A chat as `list_sessions` lists it (Session). A search adds the words
/// around the match.
export interface SessionSummary {
  id: string;
  title: string;
  pinned: boolean;
  /// When its turns last changed, in milliseconds since the Unix epoch.
  updated_at: number;
  snippet?: string;
}

/// A chat whose messages hold the searched words (SessionMatch).
interface SessionMatch {
  session: SessionSummary;
  snippet: string;
  from_user: boolean;
}

/// A chat action, as the row buttons name it.
export type ChatAction =
  | "rename" | "delete" | "cancel" | "window" | "rename-save" | "delete-yes" | "pin"
  | "share" | "copy-share" | "unshare" | "invite" | "info" | "leave";

export function when(ms: number) {
  const d = new Date(ms), now = new Date();
  const mins = Math.round((now.getTime() - d.getTime()) / 60000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins} min ago`;
  const days = dayDiff(d, now);
  if (days === 0) return `${Math.round(mins / 60)} h ago`;
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

// Chats and groups share the list: pinned first, then the latest.
const byRecent = (a: ChatRow, b: ChatRow) => (Number(b.pinned) - Number(a.pinned)) || (b.updated_at - a.updated_at);
export function rows(sessions: SessionSummary[], groups: GroupSummary[]) {
  const all: ChatRow[] = [
    ...sessions.map(s => ({ kind: "chat" as const, id: s.id, title: s.title || "New chat", pinned: s.pinned, updated_at: s.updated_at, snippet: s.snippet })),
    ...groups.map(g => ({ kind: "group" as const, id: g.id, title: g.name, pinned: g.pinned, updated_at: g.updated_at, typing: g.beep_typing, snippet: g.last ? `${authorName(g.last.author)}: ${g.last.text}` : "" })),
  ];
  return all.sort(byRecent).slice(0, 50);
}

/// Moves chat `id` to the top of the cached list now, so ⌘K shows it there
/// before the server's list comes back. A new chat is titled by its first message.
export function touch(id: string, message?: string) {
  const old = S.cache.find(c => c.id === id);
  if (!old && !message) return;
  // A new array: the open list may be drawing the old one.
  const c = { ...(old ?? { kind: "chat" as const, id, title: message!.slice(0, 80), pinned: false }), updated_at: Date.now() };
  S.cache = [c, ...S.cache.filter(x => x.id !== id)].sort(byRecent);
}

export function renderChats() {
  const back = S.view === "group" ? G.id : null;
  S.view = "chats";
  invoke("close_group").catch(() => {});
  S.chats = { list: S.cache, sel: 0, query: "", mode: "list", back, share: null };
  bar.innerHTML = `<div class="dinput"><input id="find" placeholder="Search chats and groups" autocomplete="off" spellcheck="false"><kbd>⌘K</kbd></div>
    <div class="dsec" id="sec">Recent</div><div class="dlist" id="list" role="listbox" aria-label="Chats"></div>
    ${hints([[ICON.ret, "open"], ["tab", "actions"], ["⌘N", "new chat"], ["⌘G", "new group"], ["esc", "back"]])}`;
  const find = $$<HTMLInputElement>("#find");
  find.focus();
  let pending = 0;
  find.addEventListener("input", () => {
    const q = find.value.trim(), mine = ++pending;
    setTimeout(() => { if (mine === pending) loadChats(q); }, 180);
  });
  drawChats();
  loadChats("");
}

export async function loadChats(query: string) {
  try {
    const [sessions, groups] = await Promise.all([
      query ? invoke<SessionMatch[]>("search_sessions", { query }).then(ms => ms.map(m => ({ ...m.session, snippet: m.snippet }))) : invoke<SessionSummary[]>("list_sessions"),
      invoke<GroupSummary[]>("list_groups"),
    ]);
    const q = query.toLowerCase();
    const list = rows(sessions, query ? groups.filter(g => g.name.toLowerCase().includes(q)) : groups);
    if (!query) S.cache = list;
    if (S.view !== "chats") return;
    // An older search can finish after a newer one: keep only the box's.
    if (($<HTMLInputElement>("#find")?.value.trim() ?? "") !== query) return;
    const same = query === S.chats.query;
    S.chats = { ...S.chats, list, query, sel: same ? Math.min(S.chats.sel, Math.max(0, list.length - 1)) : 0, mode: same ? S.chats.mode : "list" };
    drawChats();
  } catch (e) {
    if (S.view === "chats") $$("#list").innerHTML = `<div class="derr" style="padding:8px 10px">${esc(e)}</div>`;
  }
}

/// `text` escaped, with the words of `query` marked.
export function mark(text: string, query: string | null | undefined) {
  const words = (query ?? "").replace(/[.*+?^${}()|[\]\\]/g, "\\$&").split(/\s+/).filter(Boolean);
  if (!words.length) return esc(text);
  // Split the raw text, so a match never lands inside an escaped entity.
  const re = new RegExp(`(${words.join("|")})`, "gi");
  return text.split(re).map((part, i) => i % 2 ? `<mark>${esc(part)}</mark>` : esc(part)).join("");
}

function rowActions(c: ChatRow) {
  const pin = `<button class="dbtn" data-a="pin">${c.pinned ? "Unpin" : "Pin"}<kbd>⌘P</kbd></button>`;
  return c.kind === "group"
    ? `<span class="acts">${pin}<button class="dbtn" data-a="invite">Invite</button><button class="dbtn" data-a="info">Info</button><button class="dbtn warn" data-a="leave">Leave</button></span>`
    : `<span class="acts">${PANE ? "" : '<button class="dbtn" data-a="window">Open in window<kbd>⌘O</kbd></button>'}<button class="dbtn" data-a="rename">Rename<kbd>⌘R</kbd></button>${pin}<button class="dbtn" data-a="share">Share</button><button class="dbtn warn" data-a="delete">Delete<kbd>⌘⌫</kbd></button></span>`;
}

function chatRow(c: ChatRow, i: number) {
  const { sel, query, mode } = S.chats, on = i === sel;
  if (on && mode === "rename") return `<div class="dchat" aria-selected="true"><input id="rename" value="${esc(c.title)}" maxlength="200" spellcheck="false"></div>`;
  if (on && mode === "delete") return `<div class="dchat" aria-selected="true"><span class="t"><span>Delete <b>${esc(c.title)}</b>?</span></span><span class="acts"><button class="dbtn warn" data-a="delete-yes">Delete<kbd>${ICON.ret}</kbd></button><button class="dbtn" data-a="cancel">Cancel</button></span></div>`;
  const open = on && mode === "actions";
  const tail = open ? rowActions(c)
    : live.get(c.id)?.run || c.typing ? `<span class="run"><span class="dspin"></span>${c.typing ? "Beep is typing" : "Answering"}</span>`
    : `<span class="w">${when(c.updated_at)}</span>`;
  // A button cannot hold the action buttons, so the open row is a div.
  const tag = open ? "div" : "button";
  return `<${tag} class="dchat" role="option" aria-selected="${on}" data-i="${i}">
      ${c.pinned ? `<span class="pin" aria-label="Pinned">${ICON.pin}</span>` : ""}${c.kind === "group" ? `<span class="grp" aria-label="Group">${ICON.group}</span>` : ""}${RESEARCHES.has(c.id) ? `<span class="grp" aria-label="Research">${ICON.doc}</span>` : ""}${LIVE_CHATS.has(c.id) ? `<span class="grp vmic" aria-label="Spoken">${VOICE_ICON.mic}</span>` : ""}
      <span class="t"><span>${mark(c.title, query)}</span>${c.snippet ? `<small>${mark(c.snippet, query)}</small>` : ""}</span>${tail}</${tag}>`;
}

function shareCard() {
  const sh = S.chats.share;
  if (!sh || S.chats.mode !== "share") return "";
  const body = sh.error ? `<div class="derr">${esc(sh.error)}</div>`
    : !sh.url ? '<div class="dmuted"><span class="dspin"></span>Making a link…</div>'
    : `<div class="code">${esc(sh.url)}</div><div style="display:flex;gap:8px"><button class="dbtn pri" data-a="copy-share">Copy link</button><button class="dbtn warn" data-a="unshare">Stop sharing</button></div>`;
  return `<div class="dcard dshare">${body}<div class="dmuted" style="display:block">A copy of the chat as it is now. Only your messages and Beep's replies are shared, not tool results, thoughts or your location. Anyone with Beep can open the link.</div></div>`;
}

export function drawChats() {
  const { list, sel, query } = S.chats;
  const box = $$("#list");
  $$("#sec").textContent = query ? `${list.length} result${list.length === 1 ? "" : "s"}` : "Recent";
  box.innerHTML = list.map((c, i) => chatRow(c, i) + (i === sel ? shareCard() : "")).join("") || '<div class="dmuted" style="padding:8px 10px">No chats</div>';
  box.querySelectorAll<HTMLElement>(".dchat[data-i]").forEach(b => b.addEventListener("click", e => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-a]");
    S.chats.sel = +b.dataset.i!;
    if (act) { e.stopPropagation(); return chatAction(act.dataset.a as ChatAction); }
    openRow(list[+b.dataset.i!]);
  }));
  box.querySelectorAll<HTMLElement>("[data-a]").forEach(b => {
    if (!b.closest(".dchat[data-i]")) b.addEventListener("click", () => chatAction(b.dataset.a as ChatAction));
  });
  const rename = $<HTMLInputElement>("#rename");
  if (rename) { rename.focus(); rename.select(); }
  $(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
}

export function openRow(c: ChatRow | undefined) {
  if (!c) return;
  if (RESEARCHES.has(c.id)) return invoke("open_research", { session: c.id }).catch(() => openSession(c.id));
  return c.kind === "group" ? renderGroup(c.id) : openSession(c.id);
}

export function chatMode(mode: ChatsMode) {
  S.chats.mode = mode;
  drawChats();
  if (mode === "list" || mode === "actions") $("#find")?.focus();
}

export async function chatAction(a: ChatAction) {
  const c = S.chats.list[S.chats.sel];
  if (!c) return;
  const reload = () => { S.chats.mode = "list"; loadChats(S.chats.query); };
  try {
    switch (a) {
      case "rename": case "delete": if (c.kind === "chat") chatMode(a); break;
      case "cancel": chatMode("list"); break;
      case "window": if (c.kind === "chat") await openInWindow(c.id); break;
      case "rename-save": {
        const title = $$<HTMLInputElement>("#rename").value.trim();
        if (title && title !== c.title) await invoke("rename_session", { id: c.id, title });
        reload();
        break;
      }
      case "delete-yes":
        await invoke("delete_session", { id: c.id });
        live.delete(c.id);
        if (S.session === c.id) S.session = null;
        reload();
        break;
      case "pin":
        await invoke(c.kind === "group" ? "pin_group" : "pin_session", { id: c.id, pinned: !c.pinned });
        reload();
        break;
      case "share":
        S.chats.share = { id: c.id };
        chatMode("share");
        try {
          const link = await invoke<{ url: string }>("share_session", { id: c.id });
          if (S.chats.share?.id === c.id) S.chats.share!.url = link.url;
        } catch (e) {
          if (S.chats.share?.id === c.id) S.chats.share!.error = String(e);
        }
        if (S.view === "chats" && S.chats.mode === "share") drawChats();
        break;
      case "copy-share":
        await copy(S.chats.share!.url!, $('[data-a="copy-share"]'));
        break;
      case "unshare":
        await invoke("unshare_session", { id: c.id });
        S.chats.share = null;
        chatMode("list");
        break;
      case "invite": renderInvite(c.id, c.title); break;
      case "info": await renderGroup(c.id, "info"); break;
      case "leave": await renderGroup(c.id, "leave"); break;
    }
  } catch (e) {
    $$("#list").insertAdjacentHTML("afterbegin", `<div class="derr" style="padding:6px 10px">${esc(e)}</div>`);
  }
}

/// Copies `text` and says so on the button for a moment.
export async function copy(text: string, button?: HTMLElement | null) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // The webview may refuse the async clipboard; the old way still works.
    const t = h<HTMLTextAreaElement>("<textarea></textarea>");
    t.value = text;
    document.body.append(t);
    t.select();
    document.execCommand("copy");
    t.remove();
  }
  if (!button) return;
  const was = button.innerHTML;
  button.textContent = "Copied";
  setTimeout(() => { if (button.isConnected) button.innerHTML = was; }, 1200);
}
