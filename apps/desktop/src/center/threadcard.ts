// Chats about one pull request or issue: the card at the top of the chat.
import { plural, reason, since } from "../ui/format.ts";
import { dbtn as button, stateChip } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { pane, type PaneTarget } from "../bar/pane.ts";
import type { PaneNow } from "../bar/core.ts";
import { el as h } from "../ui/dom.ts";
import type { ThreadChatRow } from "../project/threads.ts";
import { openLabel, threadLabel, type ThreadChatEntry } from "./sidechats.ts";
import { type ViewHead, type ChatItem, $, state } from "./state.ts";
import { openUrl } from "./openitems.ts";
import { shown, registerView, drawTop } from "./views.ts";
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
      if (shown === "pane") { drawTop(); pane.chatTop(); }
    })
    .finally(() => { if (threadsLoading === mine) threadsLoading = null; });
  return threadsLoading = mine;
}

/// A thread as `center_thread_view` reads it from its host (ThreadCardRow).
interface ThreadCardView {
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

/// The card atop a thread chat: its element, what it last read (null from
/// a server too old to read one), when, and its error.
interface ThreadCard {
  el: HTMLElement;
  view: ThreadCardView | null | undefined;
  at: number;
  error: string;
  loading: boolean;
  all: boolean;
}
const threadCards = new Map<string, ThreadCard>();
/// How long a thread card's read serves before the chat reads it again.
const THREAD_FRESH_MS = 60000;
/// The comments a thread card shows before "Show all".
const THREAD_COMMENTS = 3;
/// The most of one post a card shows before "Show all".
const THREAD_TEXT = 600;

/// The card atop thread chat `t`, read again when it is stale.
export function threadCard(t: ThreadChatEntry) {
  let card = threadCards.get(t.chat_id);
  if (!card) {
    card = { el: h("div", "th-card"), view: undefined, at: 0, error: "", loading: false, all: false };
    threadCards.set(t.chat_id, card);
  }
  if (!card.loading && Date.now() - card.at > THREAD_FRESH_MS) readThreadCard(t, card);
  drawThreadCard(t, card);
  return card.el;
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
  drawThreadCard(t, card);
  const on = paneOn();
  if (on.view === "chat" && on.session === t.chat_id) drawTop();
}

const clip = (text: string, all: boolean) => all || text.length <= THREAD_TEXT ? text : text.slice(0, THREAD_TEXT).trimEnd() + "...";

function drawThreadCard(t: ThreadChatEntry, card: ThreadCard) {
  const v = card.view, label = threadLabel(t.kind, t.number);
  card.el.hidden = v === null;
  if (v === undefined) return card.el.replaceChildren(card.error ? h("span", "who", card.error) : h("span", "who", h("span", "dspin"), ` Reading ${label}…`));
  if (!v) return card.el.replaceChildren();
  const total = v.earlier + v.comments.length;
  const where = openLabel(v.url) === "Open on GitHub" ? "On GitHub" : "On its host";
  const head = h("span", "who", `${where} · ${plural(total, "comment", "comments")}${v.needs_reply ? " · waits on your reply" : ""}`);
  const post = (author: string, at: number | null, body: string) => {
    const c = h("div", "c", h("b", "", author), " ", h("span", "who", at ? since(at) : "opened it"));
    if (body.trim()) c.append(h("div", "tx", clip(body, card.all)));
    return c;
  };
  const shown = card.all ? v.comments : v.comments.slice(-THREAD_COMMENTS);
  const hidden = v.comments.length - shown.length;
  const rows = [post(v.author, null, v.body), ...shown.map(c => post(c.author, c.at, c.body))];
  const long = [v.body, ...shown.map(c => c.body)].some(b => b.length > THREAD_TEXT);
  const more: Node[] = hidden > 0 || (long && !card.all)
    ? [button(hidden > 0 ? `Show ${plural(hidden, "earlier comment", "earlier comments")}` : "Show all", "quiet small", () => { card.all = true; drawThreadCard(t, card); })]
    : [];
  if (card.all && v.earlier) more.push(h("span", "who", `${plural(v.earlier, "older comment", "older comments")} on the host`));
  if (card.error) more.push(h("span", "who", card.error));
  card.el.replaceChildren(head, ...rows, ...more);
}

/// A thread chat's header: its title, its project to go back to, whether
/// it waits on a reply, and a link to the thread.
export function threadHead(t: ThreadChatEntry, c: ChatItem | undefined): ViewHead {
  const view = threadCards.get(t.chat_id)?.view;
  const scope = h("span", "th-scope", scopeTag(t.chat_id));
  if (view?.needs_reply) scope.append(stateChip("blocked", "needs reply"));
  const tools: Node[] = [];
  if (view?.url) tools.push(button(openLabel(view.url), "", () => openUrl(view.url)));
  if (c) tools.push(pinButton(c));
  const title = view ? `${threadLabel(view.kind, view.number)} ${view.title}`.trim() : (c?.title || t.title || threadLabel(t.kind, t.number));
  return { title, tools, scope };
}

export function pinButton(c: ChatItem) {
  return button(c.pinned ? "Unpin" : "Pin", "", () => setPinned(c, !c.pinned));
}

registerView("pane", {
  el: $("#pane-box"),
  title: paneTitle,
  mount(_el, arg) {
    pane.mounted(true);
    pane.open(arg as PaneTarget);
  },
  unmount() {
    pane.mounted(false);
    pane.leave();
  },
});

/// The window stops showing a chat or group when the pane moves off it, so
/// a focused window no longer keeps their notices quiet.
export let last: Pick<PaneNow, "session"> & { view: string | null; group?: string | null } = { view: null, session: null };
$("#bar").dataset.view = pane.current().view ?? "";
pane.onChange(now => {
  if (shown === "pane") {
    if (last.view === "chat" && now.view !== "chat" && now.view !== "group") invoke("new_chat").catch(() => {});
    if (last.group && !now.group) invoke("close_group").catch(() => {});
  }
  last = now;
  $("#bar").dataset.view = now.view ?? "";
  drawSide();
});
