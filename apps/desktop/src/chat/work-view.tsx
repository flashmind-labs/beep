import { createElement, Fragment, memo } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { AgentTurnHeading, type TurnHeading } from "../agents/turn-heading-view.tsx";
import type { ChatBody, Turn } from "../ui/turn.ts";

interface Part { key: string; html: string }
export interface WorkView { heading: TurnHeading | null; thought: string; steps: Part[]; live: string }
interface Owner { root: Root; prompts: Map<HTMLElement, { key: number; html: string }>; rows: Map<HTMLElement, { key: number; value: WorkView }>; replies: Map<HTMLElement, { key: number; html: string }>; next: number }
const owners = new WeakMap<ChatBody, Owner>();
const promptTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const turns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const replyTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const promptPending = new Map<HTMLElement, { html: string; resume: () => void }>();
const replyPending = new Map<HTMLElement, { html: string; resume: () => void }>();
const pending = new Map<HTMLElement, { value: WorkView; after: () => void; resume: () => void }>();

const Html = memo(function Html({ html }: { html: string }) {
  return <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: html }} />;
});
const Prompt = memo(function Prompt({ html }: { html: string }) {
  return <><b>You</b><span dangerouslySetInnerHTML={{ __html: html }} /></>;
});
const Work = memo(function Work({ value }: { value: WorkView }) {
  return <>{value.heading && <AgentTurnHeading value={value.heading} />}{value.thought && <Html html={value.thought} />}{value.steps.map(step => <Html key={step.key} html={step.html} />)}{value.live && <Html html={value.live} />}</>;
});
function draw(owner: Owner) {
  flushSync(() => owner.root.render(createElement(Fragment, null,
    ...[...owner.prompts].map(([host, row]) => createPortal(createElement(Prompt, { html: row.html }), host, row.key)),
    ...[...owner.rows].map(([host, row]) => createPortal(createElement(Work, { value: row.value }), host, row.key)),
    ...[...owner.replies].map(([host, row]) => createPortal(createElement(Html, { html: row.html }), host, row.key)))));
}
function ownerOf(body: ChatBody) {
  let owner = owners.get(body);
  if (!owner) { owner = { root: createRoot(document.createElement("div")), prompts: new Map(), rows: new Map(), replies: new Map(), next: 0 }; owners.set(body, owner); }
  return owner;
}
export function renderChatPrompt(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  const owner = ownerOf(body);
  promptTurns.set(turn, { owner, box });
  owner.prompts.set(box, { key: ++owner.next, html });
  draw(owner);
}
export function updateChatPrompt(turn: Turn, html: string) {
  const entry = promptTurns.get(turn);
  if (!entry) return false;
  const prior = entry.owner.prompts.get(entry.box);
  if (!prior) return false;
  const selected = () => { const selection = document.getSelection(); return !!selection && !selection.isCollapsed && selection.containsNode(entry.box, true); };
  const apply = (value: string) => { entry.owner.prompts.set(entry.box, { ...prior, html: value }); draw(entry.owner); };
  if (selected()) {
    const waiting = promptPending.get(entry.box);
    if (waiting) { waiting.html = html; return true; }
    const resume = () => {
      const latest = promptPending.get(entry.box);
      if (!latest || selected()) return;
      promptPending.delete(entry.box); document.removeEventListener("selectionchange", resume); apply(latest.html);
    };
    promptPending.set(entry.box, { html, resume }); document.addEventListener("selectionchange", resume);
    return true;
  }
  const waiting = promptPending.get(entry.box);
  if (waiting) { document.removeEventListener("selectionchange", waiting.resume); promptPending.delete(entry.box); }
  apply(html);
  return true;
}
export function transferChatPrompt(from: Turn, to: Turn) {
  const entry = promptTurns.get(from);
  if (!entry) return;
  promptTurns.delete(from); promptTurns.set(to, entry);
}
export function renderChatWork(body: ChatBody, turn: Turn, box: HTMLElement, value: WorkView, after: () => void) {
  const current = ownerOf(body);
  const selected = () => {
    const selection = document.getSelection();
    return !!selection && !selection.isCollapsed && selection.containsNode(box, true);
  };
  const prior = current.rows.get(box);
  const apply = (next: WorkView, done: () => void) => {
    current.rows.set(box, { key: prior?.key ?? ++current.next, value: next });
    draw(current);
    done();
  };
  turns.set(turn, { owner: current, box });
  if (prior && selected()) {
    const waiting = pending.get(box);
    if (waiting) { waiting.value = value; waiting.after = after; return; }
    const resume = () => {
      const latest = pending.get(box);
      if (!latest || selected()) return;
      pending.delete(box);
      document.removeEventListener("selectionchange", resume);
      apply(latest.value, latest.after);
    };
    pending.set(box, { value, after, resume });
    document.addEventListener("selectionchange", resume);
    return;
  }
  const waiting = pending.get(box);
  if (waiting) { document.removeEventListener("selectionchange", waiting.resume); pending.delete(box); }
  apply(value, after);
}
export function renderChatReply(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  const owner = ownerOf(body), prior = owner.replies.get(box);
  replyTurns.set(turn, { owner, box });
  if (prior?.html === html && !replyPending.has(box)) return;
  const selected = () => { const selection = document.getSelection(); return !!selection && !selection.isCollapsed && selection.containsNode(box, true); };
  const apply = (value: string) => { owner.replies.set(box, { key: prior?.key ?? ++owner.next, html: value }); draw(owner); };
  if (prior && selected()) {
    const waiting = replyPending.get(box);
    if (waiting) { waiting.html = html; return; }
    const resume = () => {
      const latest = replyPending.get(box);
      if (!latest || selected()) return;
      replyPending.delete(box); document.removeEventListener("selectionchange", resume); apply(latest.html);
    };
    replyPending.set(box, { html, resume }); document.addEventListener("selectionchange", resume);
    return;
  }
  const waiting = replyPending.get(box);
  if (waiting) { document.removeEventListener("selectionchange", waiting.resume); replyPending.delete(box); }
  apply(html);
}
export function transferChatReply(from: Turn, to: Turn) {
  const entry = replyTurns.get(from);
  if (!entry) return;
  replyTurns.delete(from); replyTurns.set(to, entry);
}
export function discardChatWork(turn: Turn) {
  const prompt = promptTurns.get(turn), work = turns.get(turn), reply = replyTurns.get(turn);
  promptTurns.delete(turn); turns.delete(turn); replyTurns.delete(turn);
  if (prompt) {
    const waiting = promptPending.get(prompt.box);
    if (waiting) { document.removeEventListener("selectionchange", waiting.resume); promptPending.delete(prompt.box); }
    prompt.owner.prompts.delete(prompt.box);
  }
  if (work) {
    const waiting = pending.get(work.box);
    if (waiting) { document.removeEventListener("selectionchange", waiting.resume); pending.delete(work.box); }
    work.owner.rows.delete(work.box);
  }
  if (reply) {
    const waiting = replyPending.get(reply.box);
    if (waiting) { document.removeEventListener("selectionchange", waiting.resume); replyPending.delete(reply.box); }
    reply.owner.replies.delete(reply.box);
  }
  if (prompt) draw(prompt.owner);
  if (work && work.owner !== prompt?.owner) draw(work.owner);
  if (reply && reply.owner !== work?.owner && reply.owner !== prompt?.owner) draw(reply.owner);
}
