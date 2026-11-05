// Older entries of the open session's log: read a page at a time as the
// reader goes up, until the oldest turn shown has its prompt, or all of
// them for Home. The log stays where the reader is.

import { $, S } from "../bar/core.ts";
import type { ChatBody } from "../ui/turn.ts";
import { invoke } from "../ui/tauri.ts";
import { type AgentPage, AG, where } from "./agents.ts";
import { drawAgentBody, jumpToLatest, rowInView, syncJump } from "./logview.ts";
import { OLDER_MAX, OLDER_PAGE, PROMPT_READS, lacksPrompt, olderBefore, takeOlder } from "./older-state.ts";

/// How far from the top of the log, in pixels, the reader asks for older entries.
const NEAR_TOP = 900;
/// The most reads one scroll makes before it waits for the next.
const NEAR_READS = 10;

/// What a run of reads is for.
export type Reading = "near" | "prompt" | "all";

export const nearTop = (b: HTMLElement) => b.scrollTop < NEAR_TOP;

/// Whether a read of kind `mode` goes on, after `reads` reads.
function goesOn(mode: Reading, reads: number, b: HTMLElement | null) {
  const page = AG.page;
  if (!page || olderBefore(page, AG.older.stop) == null) return false;
  if (mode === "all") return reads < Math.ceil(OLDER_MAX / OLDER_PAGE) + 1;
  if (mode === "prompt") return reads < PROMPT_READS && lacksPrompt(page);
  return reads < NEAR_READS && !!b && nearTop(b);
}

/// Reads older entries before the first one held and puts them in front,
/// without moving what the reader sees. One run at a time.
export async function loadOlder(mode: Reading) {
  const o = AG.open;
  if (!o || AG.older.loading || AG.child != null) return;
  AG.older.loading = true;
  let reads = 0;
  try {
    while (goesOn(mode, reads, $<ChatBody>("#agbody"))) {
      const held = AG.page!;
      const before = held.earlier;
      const got = await invoke<AgentPage>("agent_view", { ...where(o), last: OLDER_PAGE, before });
      reads++;
      // Another session opened, or a read of the newest left a gap and
      // started the held entries over.
      if (S.view !== "agent" || AG.open !== o || !AG.page || AG.page.earlier !== before) break;
      const taken = takeOlder(AG.page, got);
      AG.older.stop = taken.stop;
      if (taken.run === AG.page) break;
      const keep = rowInView();
      AG.page = { ...AG.page, ...taken.run };
      if (mode !== "all") drawAgentBody(keep);
    }
    if (mode === "prompt" && AG.page && lacksPrompt(AG.page) && !AG.older.stop) AG.older.gaveUp = true;
  } catch (e) {
    console.warn("older entries not read", e);
  } finally {
    AG.older.loading = false;
  }
  // One draw for the whole run of "all", and for what a run says at its end.
  if (reads && S.view === "agent" && AG.open === o) drawAgentBody(rowInView());
}

/// Home or ⌘↑: every older entry is read, then the log shows its first.
export async function jumpToStart() {
  await loadOlder("all");
  const b = $<ChatBody>("#agbody");
  if (!b) return;
  b.stuck = false;
  b.scrollTop = 0;
  syncJump(b);
}

/// End or ⌘↓: the log shows its last entry, and follows new ones.
export const jumpToEnd = jumpToLatest;

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
