// Read the open session's older entries one page at a time. The log
// stays where the reader is.

import { S } from "../bar/core.ts";
import { invoke } from "../ui/tauri.ts";
import { AG, type OpenAgent, where } from "./agents-state.ts";
import { drawAgentBody, rowInView, syncJump } from "./logview.tsx";
import { OLDER_PAGE, type Run, olderBefore, takeOlder } from "./older-state.ts";
import { sessionBody } from "./session-frame-state.ts";
import { takeEarlier } from "./open-page.ts";

/// Home and Find wait for the same read that starts when the log opens.
export function loadOlder(): Promise<void> {
  const open = AG.open, older = AG.older;
  if (older.reading) return older.reading;
  if (!open || !AG.page || olderBefore(AG.page, older.stop) == null) return Promise.resolve();

  older.reading = readOlder(open, older).finally(() => {
    older.reading = null;
    if (S.view === "agent" && AG.open === open) drawAgentBody(rowInView(), false, true);
  });
  return older.reading;
}

async function readOlder(open: OpenAgent, older: typeof AG.older) {
  try {
    while (S.view === "agent" && AG.open === open && AG.page) {
      const before = olderBefore(AG.page, older.stop);
      if (before == null) return;

      const got = await invoke<Run>("agent_older", { ...where(open), last: OLDER_PAGE, before });
      if (S.view !== "agent" || AG.open !== open || !AG.page) return;
      // A fresh snapshot can move the start while this read is in flight.
      if (AG.page.earlier !== before) continue;

      const taken = takeOlder(AG.page, got);
      older.stop = taken.stop;
      if (taken.run === AG.page) return;

      const keep = rowInView();
      takeEarlier(taken.run);
      drawAgentBody(keep, false, true);
    }
  } catch (e) {
    console.warn("older entries not read", e);
  }
}

/// Home or ⌘↑: every older entry is read, then the log shows its first.
export async function jumpToStart() {
  const open = AG.open;
  await loadOlder();
  if (AG.open !== open || S.view !== "agent") return;
  const b = sessionBody();
  if (!b) return;
  b.stuck = false;
  b.scrollTop = 0;
  syncJump(b);
}

/// Which end of the log a key goes to: Home and ⌘↑ the start, End and ⌘↓
/// the end. Not while the field has text, which they move through.
export function logJumpKey(e: KeyboardEvent, cmd: boolean): "start" | "end" | null {
  if (e.altKey || e.shiftKey || e.ctrlKey || e.isComposing) return null;
  const t = e.target as HTMLInputElement | null;
  if (t?.matches?.("textarea, input, [contenteditable]") && (t.value ?? t.textContent ?? "") !== "") return null;
  if (e.key === "Home" || (cmd && e.key === "ArrowUp")) return "start";
  if (e.key === "End" || (cmd && e.key === "ArrowDown")) return "end";
  return null;
}
