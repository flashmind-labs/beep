// Read the open session's older entries one page at a time, as the reader
// nears the top. The log stays where the reader is.

import { S } from "../../stores/bar/core.ts";
import { invoke } from "../../ui/tauri.ts";
import { AG, type OpenAgent, where } from "../../stores/agents/agents-state.ts";
import { drawAgentBody, rowInView, syncJump } from "./logview.tsx";
import { OLDER_PAGE, type Run, olderBefore, readsOlder, takeOlder } from "../../stores/agents/older-state.ts";
import { paneShown } from "../../ui/active.ts";
import { sessionBody } from "../../stores/agents/session-frame-state.ts";
import { takeEarlier } from "../../stores/agents/open-page.ts";

/// Reads earlier entries while the log is short of the view or the reader is
/// near its top. With `all` it reads to the first entry, and a read in flight
/// is widened to do so: Home and Find wait for it. The wish belongs to the
/// open session (`older.all`): a hidden window pauses it, and the next call
/// after the pane shows again goes on, while another session starts clean.
export function loadOlder(all = false): Promise<void> {
  const open = AG.open, older = AG.older;
  if (all) older.all = true;
  if (older.reading) return older.reading;
  if (!open || !AG.page || olderBefore(AG.page, older.stop) == null) {
    older.all = false;
    return Promise.resolve();
  }
  if (!wantsOlder(older)) return Promise.resolve();

  older.reading = readOlder(open, older).finally(() => {
    older.reading = null;
    if (S.view === "agent" && AG.open === open) drawAgentBody(rowInView(), false, true);
  });
  return older.reading;
}

/// Whether the shown log asks for earlier entries now.
function wantsOlder(older: typeof AG.older) {
  return readsOlder(older.all, paneShown(), sessionBody()?.scrollTop ?? null);
}

async function readOlder(open: OpenAgent, older: typeof AG.older) {
  try {
    while (S.view === "agent" && AG.open === open && AG.page) {
      if (!wantsOlder(older)) return;
      const before = olderBefore(AG.page, older.stop);
      if (before == null) {
        older.all = false;
        return;
      }

      const got = await invoke<Run>("agent_older", { ...where(open), last: OLDER_PAGE, before });
      if (S.view !== "agent" || AG.open !== open || !AG.page) return;
      // A fresh snapshot can move the start while this read is in flight.
      if (AG.page.earlier !== before) continue;

      const taken = takeOlder(AG.page, got);
      older.stop = taken.stop;
      if (taken.run === AG.page) {
        older.all = false;
        return;
      }

      const keep = rowInView();
      takeEarlier(taken.run);
      drawAgentBody(keep, false, "page");
    }
  } catch (e) {
    older.all = false;
    console.warn("older entries not read", e);
  }
}

/// Home or ⌘↑: every older entry is read, then the log shows its first.
export async function jumpToStart() {
  const open = AG.open;
  await loadOlder(true);
  if (AG.open !== open || S.view !== "agent" || !AG.page) return;
  // A hidden window paused the read: the log stays where it is until it ends.
  if (olderBefore(AG.page, AG.older.stop) != null && AG.older.all) return;
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
