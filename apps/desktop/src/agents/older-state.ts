// A session's log read in pages: the newest entries come with every poll,
// older ones are asked for as the reader goes up. The page held is one run
// of entries from place `earlier`, so every place in it stays the one the
// session numbers it by.

import type { LogEntry } from "./agentturns.ts";

/// A run of the log: its entries, and how many came before them.
export interface Run {
  entries: LogEntry[];
  earlier: number;
}

/// The most entries a node answers one read with.
export const OLDER_PAGE = 200;
/// The most entries held. A live session keeps this many itself; a log
/// longer than that shows its newest.
export const OLDER_MAX = 5000;
/// The most reads one search for a turn's prompt makes in a row.
export const PROMPT_READS = 5;

/// Why no older entries will come: `old` when the machine's node does not
/// read older entries, `gone` when it no longer has them, `full` when
/// `OLDER_MAX` are held.
export type OlderStop = "" | "old" | "gone" | "full";

/// The held run with the newest read joined on. The newest read replaces
/// the held entries from its own place on; what is older stays. A newest
/// read that starts before the held run, or leaves a gap after it (more
/// came than one read holds), stands alone.
export function joinRuns<T extends Run>(held: Run | null, fresh: T): T {
  if (!held || fresh.earlier < held.earlier || fresh.earlier > held.earlier + held.entries.length) return fresh;
  const entries = [...held.entries.slice(0, fresh.earlier - held.earlier), ...fresh.entries];
  const cut = Math.max(0, entries.length - OLDER_MAX);
  return { ...fresh, entries: entries.slice(cut), earlier: held.earlier + cut };
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
  const stop: OlderStop = entries.length >= OLDER_MAX ? "full" : "";
  return { run: { entries, earlier: got.earlier }, stop };
}

/// The place to read older entries before, or null when there is nothing
/// older or no more should be held.
export function olderBefore(run: Run, stop: OlderStop) {
  return run.earlier > 0 && !stop && run.entries.length < OLDER_MAX ? run.earlier : null;
}

/// Whether the oldest turn shown lacks its prompt: older entries exist and
/// the run does not start with one.
export function lacksPrompt(run: Run) {
  return run.earlier > 0 && run.entries.length > 0 && run.entries[0].kind !== "user";
}

/// What the top of the log says when older entries are not all shown, or
/// "" when they are.
export function olderNote(run: Run, stop: OlderStop, loading: boolean) {
  if (run.earlier === 0) return "";
  if (stop === "old") return "Older entries cannot be read from this machine. Update Beep on it to see them.";
  if (stop === "gone") return "The agent no longer keeps the entries before this one.";
  if (stop === "full") return `Showing the newest ${run.entries.length} entries.`;
  return loading ? "Loading earlier entries…" : "";
}
