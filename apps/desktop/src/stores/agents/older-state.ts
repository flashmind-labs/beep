// A session's log read in pages: the newest entries come with every poll,
// older ones are read in the background. The page held is one run
// of entries from place `earlier`, so every place in it stays the one the
// session numbers it by.

import type { LogEntry } from "../../parts/session/agentturns.ts";

/// A run of the log: its entries, and how many came before them.
export interface Run {
  entries: LogEntry[];
  earlier: number;
}

/// The most entries a node answers one read with.
export const OLDER_PAGE = 200;

/// The shortest an entry is drawn, in pixels: one line of the log.
const ENTRY_MIN_PX = 20;
/// Entries read past a full view, so the first scroll up has rows at once
/// while older ones load.
const VIEW_EXTRA = 20;

/// How many of the newest entries fill a view `height` pixels tall, with
/// a margin: what a session reads when it opens.
export const viewRead = (height: number) =>
  Math.min(OLDER_PAGE, Math.ceil(Math.max(0, height) / ENTRY_MIN_PX) + VIEW_EXTRA);

/// How many of the newest entries a poll asks for: a view's worth when
/// none are held, so a session opens with the view full at once, else `poll`.
export const newestRead = (held: Run | null, poll: number, height: number) => held?.entries.length ? poll : viewRead(height);

/// Why no older entries will come: `old` when the machine's node does not
/// read older entries, `gone` when it no longer has them.
export type OlderStop = "" | "old" | "gone";

/// The held run with the newest read joined on. The newest read replaces
/// the held entries from its own place on; what is older stays. A newest
/// read that starts before the held run, or leaves a gap after it (more
/// came than one read holds), stands alone.
export function joinRuns<T extends Run>(held: Run | null, fresh: T): T {
  if (!held || fresh.earlier < held.earlier || fresh.earlier > held.earlier + held.entries.length) return fresh;
  const entries = [...held.entries.slice(0, fresh.earlier - held.earlier), ...fresh.entries];
  return { ...fresh, entries, earlier: held.earlier };
}

/// Whether `got` is the answer to a read before `before`: it ends there. A
/// node from before older reads answers with the newest entries, which do
/// not.
export function answersBefore(got: Run, before: number) {
  return got.earlier + got.entries.length === before;
}

/// The run with `got`, read before the run's first entry, put in front,
/// and why no more should be asked, if so.
export function takeOlder(held: Run, got: Run): { run: Run; stop: OlderStop } {
  if (!answersBefore(got, held.earlier)) return { run: held, stop: got.entries.length ? "old" : "gone" };
  if (!got.entries.length) return { run: held, stop: "gone" };
  const entries = [...got.entries, ...held.entries];
  return { run: { entries, earlier: got.earlier }, stop: "" };
}

/// The place to read older entries before, or null when there is nothing
/// older or the node cannot supply it.
export function olderBefore(run: Run, stop: OlderStop) {
  return run.earlier > 0 && !stop ? run.earlier : null;
}

/// What the top of the log says when older entries are not all shown, or
/// "" when they are.
export function olderNote(run: Run, stop: OlderStop, loading: boolean) {
  if (run.earlier === 0) return "";
  if (stop === "old") return "Older entries cannot be read from this machine. Update Beep on it to see them.";
  if (stop === "gone") return "The agent no longer keeps the entries before this one.";
  return loading ? "Loading earlier entries…" : "";
}

/// How near the top, in pixels, the view must come before earlier entries
/// are read: a log that fills the view reads on only as the reader nears
/// its start.
export const NEAR_TOP = 900;

/// Whether earlier entries are read now: every one was asked for, or the pane
/// shows a log scrolled near its top (a log shorter than the view always is).
/// Nothing is read while the pane is hidden or has no log.
export function readsOlder(all: boolean, shown: boolean, scrollTop: number | null) {
  return shown && (all || (scrollTop != null && scrollTop < NEAR_TOP));
}
