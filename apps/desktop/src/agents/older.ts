// Read the open session's older entries one page at a time, as the reader
// nears the top. The log stays where the reader is.

import { S } from "../bar/core.ts";
import { invoke } from "../ui/tauri.ts";
import { AG, type OpenAgent, where } from "./agents-state.ts";
import { drawAgentBody, rowInView, syncJump } from "./logview.tsx";
import { OLDER_PAGE, type Run, olderBefore, readsOlder, takeOlder } from "./older-state.ts";
import { paneShown } from "../ui/active.ts";
import { sessionBody } from "./session-frame-state.ts";
import { takeEarlier } from "./open-page.ts";

/// Whether the read in flight goes on to the first entry, for Home and Find.
let everything = false;

/// Reads earlier entries while the log is short of the view or the reader is
/// near its top. With `all` it reads to the first entry, and a read in flight
/// is widened to do so: Home and Find wait for it.
export function loadOlder(all = false): Promise<void> {
  const open = AG.open, older = AG.older;
  if (all) everything = true;
  if (older.reading) return older.reading;
  if (!open || !AG.page || olderBefore(AG.page, older.stop) == null || !wantsOlder()) {
    everything = false;
    return Promise.resolve();
  }

  older.reading = readOlder(open, older).finally(() => {
    everything = false;
    older.reading = null;
    if (S.view === "agent" && AG.open === open) drawAgentBody(rowInView(), false, true);
  });
  return older.reading;
}

/// Whether the shown log asks for earlier entries now.
function wantsOlder() {
  return readsOlder(everything, paneShown(), sessionBody()?.scrollTop ?? null);
}

async function readOlder(open: OpenAgent, older: typeof AG.older) {
  try {
    while (S.view === "agent" && AG.open === open && AG.page) {
      if (!wantsOlder()) return;
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
      drawAgentBody(keep, false, "page");
    }
  } catch (e) {
    console.warn("older entries not read", e);
  }
}

/// Home or ⌘↑: every older entry is read, then the log shows its first.
export async function jumpToStart() {
  const open = AG.open;
  await loadOlder(true);
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
