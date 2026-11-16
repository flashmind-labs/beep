// What a running turn waits for once the agent replied: the subagents and
// commands it left in the background, as one line where "Working" sat.

/// Where a click on the line goes: the subagent whose call is entry `at`, or
/// the Running tab for a command.
type WaitTarget = { kind: "subagent"; at: number } | { kind: "command" };

/// A subagent or command still running in the background, and when this
/// window first saw it running.
export interface BackgroundWork { target: WaitTarget; title: string; start: number }

/// The status a session shows while the node compacts its conversation.
export const COMPACTING = "Compacting the conversation";

/// "Waiting for 2 subagents and 1 command", with the oldest one's title,
/// start and target.
export interface WaitLine { label: string; title: string; start: number; target: WaitTarget }

/// The line for `work`, or null when nothing runs.
export function waitLine(work: readonly BackgroundWork[]): WaitLine | null {
  if (!work.length) return null;

  const subagents = work.filter(w => w.target.kind === "subagent").length, commands = work.length - subagents;
  const counts = [count(subagents, "subagent"), count(commands, "command")].filter(Boolean).join(" and ");
  const oldest = work.reduce((a, b) => b.start < a.start ? b : a);
  return { label: `Waiting for ${counts}`, title: oldest.title, start: oldest.start, target: oldest.target };
}

/// How long it waited, in whole minutes: null under a minute, "12 min",
/// "1 h 5 min".
export function waitedFor(ms: number): string | null {
  const min = Math.floor(ms / 60_000);
  if (min < 1) return null;
  if (min < 60) return `${min} min`;

  const h = Math.floor(min / 60);
  return min % 60 ? `${h} h ${min % 60} min` : `${h} h`;
}

const count = (n: number, noun: string) => n === 0 ? "" : `${n} ${noun}${n === 1 ? "" : "s"}`;
