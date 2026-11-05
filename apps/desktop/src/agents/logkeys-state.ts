// What ⌘U, ⌘Y and ⇧⌘Y act on in a session's log, apart from the screen.
import { imageLines, parseTool, subagent } from "./agentlog.ts";
import { type LogEntry, type Turn, toolKind } from "./agentturns.ts";

/// The places of the command calls in the log: the rows ⌘Y opens and folds.
/// A subagent's call is a card of its own and is left out.
export function commandRows(entries: LogEntry[], offset = 0) {
  const out: number[] = [];
  entries.forEach((en, i) => {
    if (en.kind !== "tool" || toolKind(en.text) !== "command") return;
    if (subagent(parseTool(imageLines(en.text).text).output)) return;
    out.push(offset + i);
  });
  return out;
}

/// The places of the thoughts in the log: the rows ⌘U opens and folds.
export function thoughtRows(entries: LogEntry[], offset = 0) {
  const out: number[] = [];
  entries.forEach((en, i) => { if (en.kind === "thought") out.push(offset + i); });
  return out;
}

/// The state every row gets: open when any is closed, else closed.
export function allOpen(open: boolean[]) {
  return open.some(o => !o);
}

/// The turns holding any of the places `ats`, by key. A thought folded into
/// its turn's heading shows only once the turn is open.
export function turnsHolding(turns: Turn[], ats: number[]) {
  const want = new Set(ats);
  return turns.filter(t => t.steps.some(at => want.has(at))).map(t => t.key);
}
