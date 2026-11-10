// chat/: chats and groups in the bar: the open chat, the list, the composer,
// attachments and the cards under a reply. Entry: this file, the open chat's shell.
import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import { updateChatNavigation } from "./navigation-view.tsx";
import { createChatBody, disposeChatBody, transcriptShown } from "./transcript.ts";
import { AG } from "../agents/agents.ts";
import { attach, attached, drawAttached, unattach } from "./attach.ts";
import { composeField } from "./compose.ts";
import { $, $$, bar, here, live, PANE, queued, S } from "../bar/core.ts";
import { renderAgentNew } from "../agents/agentnew.ts";
import { currentChatHints, refreshHints } from "../bar/hints.ts";
import { takeDraft, type LiveView } from "../bar/live.ts";
import { followsAfterScroll, returnScroll } from "../agents/logscroll.ts";
import { followEnd, unfollowEnd } from "../ui/followend.ts";
import { invoke } from "../ui/tauri.ts";
import type { ChatBody } from "../ui/turn.ts";
import { dragGrip, grow } from "./ask-height.ts";
import { sendQueued, setAsk } from "./queue.ts";

const retiringBodies = new WeakSet<ChatBody>();

function evictBody(body: ChatBody) {
  retiringBodies.delete(body);
  unfollowEnd(body);
  disposeChatBody(body);
}

/// A body let go of while its run streamed goes once the run finishes,
/// unless it shows or a chat keeps it again.
export function retireFinished(body: ChatBody | null) {
  if (!body || !retiringBodies.has(body)) return;
  retiringBodies.delete(body);
  if (!body.isConnected && ![...live.values()].some(entry => entry.body === body)) evictBody(body);
}

/// A cached live body may be detached; release it only when the entry goes.
export function forgetChat(id: string | null) {
  const entry = live.get(id), cached = entry?.body;
  live.delete(id);
  if (!cached) return;
  // A removed entry can still have an in-flight stream using its turn.
  if (entry?.run) retiringBodies.add(cached);
  else if (!cached.isConnected && ![...live.values()].some(entry => entry.body === cached)) evictBody(cached);
}

let chatShell: { page: BarPageSlot; body: ChatBody; dock: LiveView | null } | null = null;

export function renderChat() {
  // The chat needs the server: without it every way here lands on the agent's start form.
  if (S.link !== "linked") return renderAgentNew("", PANE ? "agents" : "chat");
  S.view = "chat";
  const kept = here()?.body;
  const chatBody = kept ?? createChatBody();
  let host: HTMLElement | null = null;
  const page = openBarPage(() => {
    chatShell = null;
    if (!retiringBodies.has(chatBody) && ![...live.values()].some(entry => entry.body === chatBody)) evictBody(chatBody);
    if (host?.parentElement !== bar) return;
    for (const child of [...bar.children]) if (child !== host) child.remove();
  });
  host = page.host;
  chatShell = { page, body: chatBody, dock: null };
  drawQueue();
  const b = body()!;
  // A chat put back goes where the reader left it if they had scrolled up, else to its end.
  if (kept) { b.scrollTop = returnScroll(b, b.top ?? 0); transcriptShown(b); }
  let top = b.scrollTop;
  b.onscroll = () => { b.stuck = followsAfterScroll(b.stuck !== false, top, b); top = b.top = b.scrollTop; drawLatest(); };
  followEnd(b);
  drawLatest();
  drawQueue();
  drawAttached();
  const ask = $$<HTMLTextAreaElement>("#ask");
  let multi = false, typed = false;
  ask.addEventListener("input", () => {
    grow(ask);
    if (multi !== ask.value.includes("\n")) { multi = !multi; refreshHints(); }
    if (typed !== !!ask.value.trim()) { typed = !typed; refreshHints(); }
  });
  dragGrip(ask);
  composeField(ask, { images: attach });
  takeDraft(ask);
  focusAsk();
}

export function focusAsk() { $("#ask")?.focus(); }
export function body() { return $<ChatBody>("#body"); }
export function scrollDown() { const b = body(); if (b) { b.stuck = true; b.scrollTop = b.scrollHeight; } }
/// Follows a streaming reply down, unless the user scrolled up to read.
export function keepDown() { if (body()?.stuck !== false) scrollDown(); }

/// While the user reads back, a button that jumps to the newest text.
export function drawLatest() { const b = body(); if (b) updateChatNavigation(b); }


export function setChatDock(view: LiveView | null) {
  if (!chatShell) return;
  chatShell.dock = view;
  drawQueue();
}

/// The open chat's queued prompts, between the reply and the hints. x puts
/// one back in the field.
export function drawQueue() {
  const shell = chatShell;
  if (!shell || shell.page.host.parentElement !== bar || S.view !== "chat") return;
  const q = queued.get(S.session);
  const update = (index: number, edit: boolean) => {
    const current = queued.get(S.session);
    if (!current) return;
    const [text] = current.list.splice(index, 1);
    if (!current.list.length) queued.delete(S.session);
    if (edit) setAsk([text, $$<HTMLTextAreaElement>("#ask").value].filter(value => value.trim()).join("\n\n"));
    drawQueue();
    refreshHints();
    focusAsk();
  };
  flushSync(() => shell.page.show({ view: "chat", props: {
    body: shell.body, hints: currentChatHints(), running: !!here()?.run, dock: shell.dock, queue: q ?? null, attachments: attached,
    agent: AG.hosts?.length ? () => renderAgentNew($<HTMLTextAreaElement>("#ask")?.value.trim() ?? "") : null,
    unattach: (index: number) => { unattach(index); focusAsk(); },
    edit: (index: number) => update(index, true), remove: (index: number) => update(index, false), send: () => sendQueued(S.session),
  } }));
}

export async function newChat() {
  S.session = null;
  renderChat();
  await invoke("new_chat");
}

/// Hands a view to the command center window, which hides the bar.
export function toWindow(target: unknown) {
  return invoke("center_open", { target }).catch(() => {});
}
/// Opens chat `id` (null for a new one) in the window.
export const openInWindow = (id: string | null | undefined) => toWindow({ chat: { session: id ?? null } });
