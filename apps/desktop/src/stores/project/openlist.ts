// The controls Home's and a project's Issues and Pull requests lists share:
// when it opened, newest or oldest, Select on issues, and Start N agents.
// Each list keeps its choices while the app runs; the callers draw the rows.
import { arrange, newChoice, type ListChoice, type StartRow } from "./openlist-state.ts";

const choices = new Map<string, ListChoice>();

export const choiceOf = (key: string) => {
  let c = choices.get(key);
  if (!c) choices.set(key, c = newChoice());
  return c;
};

/// How the last Start N agents of a list ended.
export interface Outcome {
  started: StartRow[];
  failed: StartRow | null;
  error: string | null;
  /// The rows left out, with why.
  left: StartRow[];
}

export const outcomes = new Map<string, Outcome>();
export const starting = new Set<string>();

/// `items` for list `key`, in the window and order picked there.
export function arranged<T>(key: string, items: readonly T[], opened: (item: T) => number | null | undefined) {
  const c = choiceOf(key);
  return arrange(items, opened, c.window, c.order, Date.now());
}

/// Selection is owned by the list choice and shared with the bulk action bar.
export function issueSelection(key: string, id: string): boolean | null {
  const choice = choiceOf(key);
  return choice.selecting ? choice.picked.has(id) : null;
}

export function setIssueSelection(key: string, id: string, checked: boolean, redraw: () => void) {
  const choice = choiceOf(key);
  if (checked) choice.picked.add(id); else choice.picked.delete(id);
  choice.confirming = false;
  redraw();
}
