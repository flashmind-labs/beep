import { createElement } from "react";
import { ThreadCardContent } from "./threadcard-view.tsx";
// Chats about one pull request or issue: the card at the top of the chat.
import { reason } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { pane } from "../bar/pane.ts";
import { bar, type PaneNow } from "../bar/core.ts";
import type { ThreadChatRow } from "../project/threads.ts";
import { openLabel, threadLabel, type ThreadChatEntry } from "./sidechats.ts";
import { type CenterView, type ViewHead, type ChatItem, state } from "./state.ts";
import { openUrl } from "./openitems.ts";
import { currentLocation, drawTop } from "./views.ts";
import { reloadGroup, paneOn, drawSide, reloadChats, setPinned } from "./sidebar.ts";
import { paneTitle, scopeTag } from "./pane.ts";

/// The chats about one pull request or issue, by chat id, read for each
/// project; null until first read.
export let threadChats: Map<string, ThreadChatEntry> | null = null;
export let threadsLoading: Promise<void> | null = null;

// ---------- Chats about one thread ----------

/// Reads which chats are about one thread, for every project, then redraws what
/// lists or heads them.
export function loadThreadChats() {
  if (threadsLoading) return threadsLoading;
  const agents = state.agents || [];
  const before = threadChats;
  const mine: Promise<void> = Promise.all(agents.map(a => invoke<ThreadChatRow[] | null>("center_thread_chats", { projectId: a.project_id })
    .then(rows => (rows || []).map((c): ThreadChatEntry => ({ ...c, project_id: a.project_id, project: a.name })))
    .catch((e): ThreadChatEntry[] => {
      console.warn("thread chats:", e);
      return [...(before?.values() ?? [])].filter(c => c.project_id === a.project_id);
    })))
    .then(lists => {
      threadChats = new Map(lists.flat().map(c => [c.chat_id, c]));
      reloadChats();
      reloadGroup("projects");
      if (currentLocation()?.view === "pane") { drawTop(); pane.chatTop(); }
    })
    .finally(() => { if (threadsLoading === mine) threadsLoading = null; });
  return threadsLoading = mine;
}

/// A thread as `center_thread_view` reads it from its host (ThreadCardRow).
export interface ThreadCardView {
  kind: string;
  number: number;
  title: string;
  url: string;
  author: string;
  body: string;
  needs_reply: boolean;
  /// `at` in milliseconds since the Unix epoch.
  comments: { author: string; at: number; body: string }[];
  earlier: number;
}

/// The card atop a thread chat: what it last read (null from a server too
/// old to read one), when, and its error.
interface ThreadCard {
  view: ThreadCardView | null | undefined;
  at: number;
  error: string;
  loading: boolean;
  all: boolean;
}
const threadCards = new Map<string, ThreadCard>();
/// How long a thread card's read serves before the chat reads it again.
const THREAD_FRESH_MS = 60000;

/// The card atop thread chat `t`, read again when it is stale.
export function threadCard(t: ThreadChatEntry) {
  let card = threadCards.get(t.chat_id);

  if (!card) {
    card = { view: undefined, at: 0, error: "", loading: false, all: false };
    threadCards.set(t.chat_id, card);
  }

  if (!card.loading && Date.now() - card.at > THREAD_FRESH_MS) readThreadCard(t, card);
  const shown = card, where = card.view && openLabel(card.view.url) === "Open on GitHub" ? "On GitHub" : "On its host";
  return createElement("div", { key: t.chat_id, className: "th-card dchattop", hidden: card.view === null }, createElement(ThreadCardContent, {
    view: card.view, label: threadLabel(t.kind, t.number), where, error: card.error, all: card.all,
    expand: () => { shown.all = true; pane.chatTop(); },
  }));
}

async function readThreadCard(t: ThreadChatEntry, card: ThreadCard) {
  card.loading = true;

  try {
    card.view = await invoke<ThreadCardView | null>("center_thread_view", { projectId: t.project_id, repo: t.repo, number: t.number });
    card.error = "";
  } catch (e) {
    card.error = reason(e);
  } finally {
    card.loading = false;
    card.at = Date.now();
  }

  const on = paneOn();
  if (on.view === "chat" && on.session === t.chat_id) { drawTop(); pane.chatTop(); }
}

/// A thread chat's header: its title, its project to go back to, whether
/// it waits on a reply, and a link to the thread.
export function threadHead(t: ThreadChatEntry, c: ChatItem | undefined): ViewHead {
  const view = threadCards.get(t.chat_id)?.view;
  const scope = createElement("span", { className: "th-scope" }, scopeTag(t.chat_id), view?.needs_reply ? createElement("span", { className: "chip blocked" }, "needs reply") : null);
  const tools = [];
  if (view?.url) { const url = view.url; tools.push(createElement("button", { type: "button", className: "dbtn", key: "link", onClick: () => openUrl(url) }, openLabel(url))); }
  if (c) tools.push(pinButton(c));
  const title = view ? `${threadLabel(view.kind, view.number)} ${view.title}`.trim() : (c?.title || t.title || threadLabel(t.kind, t.number));
  return { title, reactTools: tools, reactScope: scope };
}

export function pinButton(c: ChatItem) {
  return createElement("button", { type: "button", className: "dbtn", key: "pin", onClick: () => { void setPinned(c, !c.pinned); } }, c.pinned ? "Unpin" : "Pin");
}

export const paneView: CenterView<"pane"> = {
  title: paneTitle,
  mount(_el, arg) {
    pane.mounted(true);
    return pane.open(arg).then(() => {});
  },
  unmount() {
    pane.mounted(false);
    pane.leave();
  },
};

/// The window stops showing a chat or group when the pane moves off it, so
/// a focused window no longer keeps their notices quiet.
export let last: Pick<PaneNow, "session"> & { view: string | null; group?: string | null } = { view: null, session: null };
bar.dataset.view = pane.current().view ?? "";
pane.onChange(now => {
  if (currentLocation()?.view === "pane") {
    if (last.view === "chat" && now.view !== "chat" && now.view !== "group") invoke("new_chat").catch(() => {});
    if (last.group && !now.group) invoke("close_group").catch(() => {});
  }

  last = now;
  bar.dataset.view = now.view ?? "";
  drawSide();
});
