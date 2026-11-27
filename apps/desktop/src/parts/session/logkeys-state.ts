// What ⌘U, ⌘Y and ⇧⌘Y act on in a session's log, apart from the screen.
import { imageLines, parseTool, subagent } from "../../stores/agents/agentlog.ts";
import { type LogEntry, toolKind } from "./agentturns.ts";

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

/// The first place of each thought group: the rows ⌘U opens and folds.
export function thoughtRows(entries: LogEntry[], offset = 0) {
  const out: number[] = [];
  entries.forEach((en, i) => { if (en.kind === "thought" && entries[i - 1]?.kind !== "thought") out.push(offset + i); });
  return out;
}

/// The state every row gets: open when any is closed, else closed.
export function allOpen(open: boolean[]) {
  return open.some(o => !o);
}

/// PageUp and PageDown page the conversation when its composer or log has focus.
export function pageDirection(e: Pick<KeyboardEvent, "key" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey" | "isComposing">, inConversation: boolean) {
  if (!inConversation || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || e.isComposing) return 0;
  return e.key === "PageUp" ? -1 : e.key === "PageDown" ? 1 : 0;
}
