// Prompts typed while the open chat's reply runs, sent one by one.
import { chatAsk } from "./chat-shell.tsx";
import { live, queued, S } from "../bar/core.ts";
import { refreshHints } from "../bar/hints.ts";
import { drawQueue } from "./chat.ts";
import { grow } from "./ask-height.ts";
import { send } from "./runs.ts";

/// Empties the prompt, or sets it to `text`, and sizes it to match.
export function setAsk(text: string) {
  const ask = chatAsk.current;
  if (!ask) return;
  ask.value = text;
  grow(ask);
}

/// Holds a prompt typed while the open chat's reply runs.
export function enqueue(text: string) {
  const id = S.session;
  if (!id) return;
  const q = queued.get(id) ?? { list: [], held: false };
  q.list.push(text);
  queued.set(id, q);
  setAsk("");
  drawQueue();
  refreshHints();
}

/// Sends the chat's first queued prompt; the rest follow as each reply finishes.
export function sendQueued(id: string | null) {
  const q = queued.get(id);
  if (!q?.list.length || id !== S.session || S.view !== "chat" || live.get(id)?.run || live.get(id)?.approval) return;
  const text = q.list.shift()!;
  q.held = false;
  if (!q.list.length) queued.delete(id);
  drawQueue();
  send(text);
}

/// Takes the open chat's last queued prompt back into the empty field to
/// edit. False when nothing is queued.
export function popQueued() {
  const q = queued.get(S.session);
  const text = q?.list.pop();
  if (text == null) return false;
  if (!q!.list.length) queued.delete(S.session);
  setAsk(text);
  drawQueue();
  refreshHints();
  return true;
}
