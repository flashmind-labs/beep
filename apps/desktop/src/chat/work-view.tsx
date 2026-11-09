import { createElement, Fragment, memo } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { AgentTurnHeading, type TurnHeading } from "../agents/turn-heading-view.tsx";
import type { ChatBody, Turn } from "../ui/turn.ts";

interface Part { key: string; html: string }
export interface WorkView { heading: TurnHeading | null; thought: string; steps: Part[]; live: string }
interface Owner { root: Root; rows: Map<HTMLElement, { key: number; value: WorkView }>; next: number }
const owners = new WeakMap<ChatBody, Owner>();
const turns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const pending = new Map<HTMLElement, { value: WorkView; after: () => void; resume: () => void }>();

const Html = memo(function Html({ html }: { html: string }) {
  return <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: html }} />;
});
const Work = memo(function Work({ value }: { value: WorkView }) {
  return <>{value.heading && <AgentTurnHeading value={value.heading} />}{value.thought && <Html html={value.thought} />}{value.steps.map(step => <Html key={step.key} html={step.html} />)}{value.live && <Html html={value.live} />}</>;
});
function draw(owner: Owner) {
  flushSync(() => owner.root.render(createElement(Fragment, null, ...[...owner.rows].map(([host, row]) => createPortal(createElement(Work, { value: row.value }), host, row.key)))));
}
export function renderChatWork(body: ChatBody, turn: Turn, box: HTMLElement, value: WorkView, after: () => void) {
  let owner = owners.get(body);
  if (!owner) { owner = { root: createRoot(document.createElement("div")), rows: new Map(), next: 0 }; owners.set(body, owner); }
  const current = owner;
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
export function discardChatWork(turn: Turn) {
  const mounted = turns.get(turn);
  if (!mounted) return;
  turns.delete(turn);
  const waiting = pending.get(mounted.box);
  if (waiting) { document.removeEventListener("selectionchange", waiting.resume); pending.delete(mounted.box); }
  mounted.owner.rows.delete(mounted.box);
  draw(mounted.owner);
}
