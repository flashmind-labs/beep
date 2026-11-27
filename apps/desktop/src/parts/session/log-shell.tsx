import { flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../../ui/document-root.tsx";
import type { AgentQuestion } from "../../stores/agents/agents-state.ts";
import { AgentLogShell, type LogShell } from "./log-shell-view.tsx";
import { createLog, disposeLog, resetLog, setUnits, type LogWindow, type Unit } from "./logwindow.tsx";

interface ShellOwner {
  view: DocumentView;
  value: LogShell | null;
  log: LogWindow;
}
const owners = new WeakMap<HTMLElement, ShellOwner>();

function sameQuestion(a: AgentQuestion | null, b: AgentQuestion | null) {
  return a === b || (!!a && !!b && a.id === b.id && a.text === b.text && a.detail === b.detail
    && a.options.length === b.options.length && a.options.every((option, index) => {
      const other = b.options[index];
      return option.id === other.id && option.name === other.name && option.allow === other.allow;
    }));
}

function sameNote(a: LogShell["note"], b: LogShell["note"]) {
  return a === b || (!!a && !!b && a.text === b.text && a.tone === b.tone);
}

function sameShell(a: LogShell, b: LogShell) {
  return a.why === b.why && a.starting === b.starting && a.log === b.log && a.partial === b.partial && sameQuestion(a.question, b.question)
    && a.questionKey === b.questionKey && a.answering === b.answering
    && a.error === b.error && a.external === b.external && a.switchModel === b.switchModel && sameNote(a.note, b.note) && a.retry === b.retry;
}

/// A cached body keeps its shell and transcript until the session is evicted.
export function disposeLogShell(body: HTMLElement) {
  const owner = owners.get(body);
  if (!owner) return;
  disposeLog(owner.log);
  owner.view.unmount();
  owners.delete(body);
}

/// Defer changed siblings that contain a selection or focused control.
export function showLogShell(body: HTMLElement, value: LogShell, units: readonly Unit[] | null, held: readonly Node[]) {
  let owner = owners.get(body);
  if (!owner) {
    owner = { view: createDocumentView(body), value: null, log: createLog(body) };
    owners.set(body, owner);
  }
  const log = owner.log;
  const prior = owner.value;
  const next = { ...value };
  let deferred = false;
  if (prior) {
    const heldPart = (part: string, changed: boolean) => changed
      && held.some(node => body.querySelector(`[data-log-part="${part}"]`)?.contains(node));
    if (heldPart("why", prior.why !== value.why)) { next.why = prior.why; deferred = true; }
    if (heldPart("starting", prior.starting !== value.starting || prior.log !== value.log)) { next.starting = prior.starting; next.log = prior.log; deferred = true; }
    if (heldPart("card", !sameQuestion(prior.question, value.question) || prior.questionKey !== value.questionKey || prior.answering !== value.answering)) {
      next.question = prior.question; next.questionKey = prior.questionKey; next.answering = prior.answering; deferred = true;
    }
    if (heldPart("error", prior.error !== value.error || prior.external !== value.external || prior.switchModel !== value.switchModel)) {
      next.error = prior.error; next.external = prior.external; next.switchModel = prior.switchModel; deferred = true;
    }
    if (heldPart("note", !sameNote(prior.note, value.note))) { next.note = prior.note; deferred = true; }
    if (heldPart("retry", prior.retry !== value.retry)) { next.retry = prior.retry; deferred = true; }
    if (log.el && !value.log && held.some(node => log.el?.contains(node))) { next.log = true; deferred = true; }
  }
  if (log.el && next.log && value.log && units && !setUnits(log, units, held)) deferred = true;
  if (log.el && !next.log) resetLog(log);
  if (!prior || !sameShell(prior, next)) {
    owner.value = next;
    flushSync(() => owner?.view.render(<AgentLogShell value={next} log={log} />));
  }
  if (log.el && next.log && units && !prior?.log) setUnits(log, units, held);
  return !deferred;
}
