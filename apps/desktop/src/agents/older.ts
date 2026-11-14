// Older entries of the open session's log: read a page at a time as the
// reader goes up, until the oldest turn shown has its prompt, or all of
// them for Home. The log stays where the reader is.

import { S } from "../bar/core.ts";
import { invoke } from "../ui/tauri.ts";
import { AG, where } from "./agents.ts";
import { drawAgentBody, rowInView, syncJump } from "./logview.tsx";
import { OLDER_MAX, OLDER_PAGE, PROMPT_READS, type Reading, type Run, drawsEachPage, lacksPrompt, olderBefore, takeOlder } from "./older-state.ts";
import { sessionBody } from "./session-frame.tsx";

/// How far from the top of the log, in pixels, the reader asks for older entries.
const NEAR_TOP = 900;
/// The most reads one scroll makes before it waits for the next.
const NEAR_READS = 10;

export const nearTop = (b: HTMLElement) => b.scrollTop < NEAR_TOP;

/// Whether a read of kind `mode` goes on, after `reads` reads.
function goesOn(mode: Reading, reads: number, b: HTMLElement | null) {
  const page = AG.page;
  if (!page || olderBefore(page, AG.older.stop) == null) return false;
  if (mode === "all") return reads < Math.ceil(OLDER_MAX / OLDER_PAGE) + 1;
  if (mode === "prompt") return reads < PROMPT_READS && lacksPrompt(page);
  return reads < NEAR_READS && !!b && nearTop(b);
}

let reading: Promise<void> | null = null;

/// Reads older entries before the first one held and puts them in front,
/// without moving what the reader sees. A whole-log read waits for one in flight.
export function loadOlder(mode: Reading): Promise<void> {
  if (reading) {
    const open = AG.open;
    return mode === "all" ? reading.then(() => AG.open === open ? loadOlder(mode) : undefined) : reading;
  }

  reading = readOlder(mode).finally(() => { reading = null; });
  return reading;
}

async function readOlder(mode: Reading) {
  const o = AG.open;
  if (!o || AG.older.loading) return;
  AG.older.loading = true;
  let reads = 0;

  try {
    while (goesOn(mode, reads, sessionBody())) {
      const held = AG.page!;
      const before = held.earlier;
      const got = await invoke<Run>("agent_older", { ...where(o), last: OLDER_PAGE, before });
      reads++;
      // Another session opened, or a read of the newest left a gap and
      // started the held entries over.
      if (S.view !== "agent" || AG.open !== o || !AG.page || AG.page.earlier !== before) break;
      const taken = takeOlder(AG.page, got);
      AG.older.stop = taken.stop;
      if (taken.run === AG.page) break;
      const keep = rowInView();
      AG.page = { ...AG.page, ...taken.run };
      if (drawsEachPage(mode)) drawAgentBody(keep, false, true);
    }

    if (mode === "prompt" && AG.page && lacksPrompt(AG.page) && !AG.older.stop) AG.older.gaveUp = true;
  } catch (e) {
    console.warn("older entries not read", e);
  } finally {
    AG.older.loading = false;
  }

  // One draw for a whole run not drawn page by page, and for what a run says at its end.
  if (reads && S.view === "agent" && AG.open === o) drawAgentBody(rowInView(), false, true);
}

/// Home or ⌘↑: every older entry is read, then the log shows its first.
export async function jumpToStart() {
  await loadOlder("all");
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
