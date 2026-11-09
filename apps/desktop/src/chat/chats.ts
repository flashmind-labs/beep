import { createElement } from "react";
import { flushSync } from "react-dom";
import type { Root } from "react-dom/client";
import { openBarPage } from "../bar/page-root.ts";
import { ChatsShell } from "./chats-view.tsx";
import { openInWindow } from "./chat.ts";
import { copy } from "../ui/parts.ts";
import { $, $$, type ChatRow, type ChatsMode, G, live, PANE, S } from "../bar/core.ts";
import { authorName, type GroupSummary, renderGroup, renderInvite } from "./groups.ts";
import { hints } from "../bar/hints.ts";
import { openSession } from "./history.ts";
import { RESEARCHES } from "../research/researches.ts";
import { dayDiff } from "../lists/memories.ts";
import { withoutResearch } from "../research/research-list.ts";
import { esc } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";

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

let listRoot: { host: HTMLElement; root: Root } | null = null;
let listError = "", lastSelected = "", loadVersion = 0;
let searchTimer: ReturnType<typeof setTimeout> | null = null;
function disposeChats() {
  listRoot = null;
  if (searchTimer) { clearTimeout(searchTimer); searchTimer = null; }
}

export function renderChats() {
  listError = ""; lastSelected = ""; loadVersion++;
  const back = S.view === "group" ? G.id : null;
  S.view = "chats";
  invoke("close_group").catch(() => {});
  S.chats = { list: S.cache, sel: 0, query: "", mode: "list", back, share: null };
  listRoot = openBarPage(disposeChats);
  drawChats();
  loadChats("");
}

export async function loadChats(query: string) {
  const version = ++loadVersion;
  try {
    const [sessions, groups] = await Promise.all([
      query ? invoke<SessionMatch[]>("search_sessions", { query }).then(ms => ms.map(m => ({ ...m.session, snippet: m.snippet }))) : invoke<SessionSummary[]>("list_sessions"),
      invoke<GroupSummary[]>("list_groups"),
    ]);
    if (version !== loadVersion) return;
    const q = query.toLowerCase();
    // The window lists researches under Research; the bar marks them here.
    const chats = PANE ? withoutResearch(sessions, RESEARCHES) : sessions;
    const list = rows(chats, query ? groups.filter(g => g.name.toLowerCase().includes(q)) : groups);
    if (!query) S.cache = list;
    if (S.view !== "chats") return;
    // An older search can finish after a newer one: keep only the box's.
    if (($<HTMLInputElement>("#find")?.value.trim() ?? "") !== query) return;
    const same = query === S.chats.query;
    const previous = same ? S.chats.list[S.chats.sel] : null;
    const kept = previous ? list.findIndex(row => row.kind === previous.kind && row.id === previous.id) : -1;
    const sel = same ? kept >= 0 ? kept : Math.min(S.chats.sel, Math.max(0, list.length - 1)) : 0;
    S.chats = { ...S.chats, list, query, sel, mode: same ? S.chats.mode : "list" };
    listError = ""; drawChats();
  } catch (e) {
    if (version === loadVersion && S.view === "chats" && ($<HTMLInputElement>("#find")?.value.trim() ?? "") === query) { listError = String(e); drawChats(); }
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

export function drawChats() {
  const current = listRoot;
  if (!current?.host.isConnected || S.view !== "chats") return;
  const { list, sel } = S.chats;
  const selected = list[sel], identity = selected ? `${selected.kind}:${selected.id}` : "";
  flushSync(() => current.root.render(createElement(ChatsShell, {
    state: S.chats, error: listError, pane: PANE,
    hints: hints([[ICON.ret, "open"], ["tab", "actions"], ["⌘N", "new agent"], ["⌘G", "new group"], ["esc", "back"]]),
    search: (value: string) => { if (searchTimer) clearTimeout(searchTimer); searchTimer = setTimeout(() => { searchTimer = null; void loadChats(value.trim()); }, 180); },
    running: new Set([...live].flatMap(([id, value]) => id !== null && value.run ? [id] : [])),
    select: (index: number) => { S.chats.sel = index; }, open: openRow, action: (action: ChatAction) => void chatAction(action),
  })));
  if (identity && identity !== lastSelected) current.host.querySelector<HTMLElement>(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  lastSelected = identity;
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
    if (S.view === "chats") { listError = String(e); drawChats(); }
  }
}

