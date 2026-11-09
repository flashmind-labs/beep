import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { AgentLogShell, type LogShell } from "./log-shell-view.tsx";
import { disposeLog, setUnits, type Unit } from "./logwindow.ts";

interface ShellOwner {
  view: DocumentView;
  value: LogShell | null;
  log: HTMLDivElement | null;
  ref(node: HTMLDivElement | null): void;
}
const owners = new WeakMap<HTMLElement, ShellOwner>();

/// A cached body keeps its shell and transcript until the session is evicted.
export function disposeLogShell(body: HTMLElement) {
  const owner = owners.get(body);
  if (!owner) return;
  if (owner.log) disposeLog(owner.log);
  owner.view.unmount();
  owners.delete(body);
}

/// Defer changed siblings that contain a selection or focused control.
export function showLogShell(body: HTMLElement, value: LogShell, units: readonly Unit[] | null, held: readonly Node[]) {
  let owner = owners.get(body);
  if (!owner) {
    const old = body.querySelector<HTMLElement>(".dcoderlog");
    if (old) disposeLog(old);
    const view = createDocumentView(body);
    owner = { view, value: null, log: null, ref: node => { if (owner) owner.log = node; } };
    owners.set(body, owner);
  }
  const prior = owner.value;
  const next = { ...value };
  let deferred = false;
  if (prior) {
    const heldPart = (part: string, before: unknown, after: unknown) => {
      const changed = JSON.stringify(before) !== JSON.stringify(after);
      return changed && held.some(node => body.querySelector(`[data-log-part="${part}"]`)?.contains(node));
    };
    if (heldPart("why", prior.why, value.why)) { next.why = prior.why; deferred = true; }
    if (heldPart("starting", prior.starting, value.starting)) { next.starting = prior.starting; next.log = prior.log; deferred = true; }
    if (heldPart("card", [prior.question, prior.answering, prior.plan, prior.asking], [value.question, value.answering, value.plan, value.asking])) {
      next.question = prior.question; next.answering = prior.answering; next.plan = prior.plan; next.asking = prior.asking; deferred = true;
    }
    if (heldPart("error", [prior.error, prior.external], [value.error, value.external])) { next.error = prior.error; next.external = prior.external; deferred = true; }
    if (heldPart("note", prior.note, value.note)) { next.note = prior.note; deferred = true; }
    if (heldPart("retry", prior.retry, value.retry)) { next.retry = prior.retry; deferred = true; }
    if (owner.log && !value.log && held.some(node => owner?.log?.contains(node))) { next.log = true; deferred = true; }
  }
  if (owner.log && next.log && value.log && units && !setUnits(owner.log, units, held)) deferred = true;
  if (owner.log && !next.log) { disposeLog(owner.log); owner.log.replaceChildren(); owner.log = null; }
  if (!prior || JSON.stringify(prior) !== JSON.stringify(next)) {
    owner.value = next;
    flushSync(() => owner?.view.render(createElement(AgentLogShell, { value: next, logRef: owner.ref })));
  }
  if (owner.log && next.log && units && !prior?.log) setUnits(owner.log, units, held);
  return !deferred;
}
