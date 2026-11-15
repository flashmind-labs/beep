// Moving a coding agent session to another machine, apart from the DOM so it
// can be tested: the menu's notes, the steps, what the lists say and which
// machine's copy of a session they show.
import type { OpenAgent } from "./agents.ts";

/// A move record (MoveView).
export interface MoveRecord {
  session: string;
  agent: string;
  from: string;
  to: string;
  project: string;
  at: number;
}

type MovePhase = "waiting" | "conversation" | "files" | "resuming" | "done" | "failed" | "cancelled";

/// What the target says of a move (MoveProgressView).
export interface MoveProgress {
  answer: "known" | "unknown" | "update";
  phase: MovePhase;
  copied: number;
  total: number;
  source_away: boolean;
  /// Why it failed, or what it waits for, such as checkouts at two commits.
  reason: string | null;
  /// The worktree coming over, and which of how many.
  worktree: string | null;
  worktree_at: number;
  worktrees: number;
  /// The source's checkout has uncommitted edits, which stay there.
  edits_left: boolean;
}

/// A machine a session could move to, with what the desktop knows of it.
export interface MoveTarget {
  id: string;
  name: string;
  online: boolean;
  /// The harnesses it has.
  harnesses: readonly string[];
  /// It keeps the project's checkout.
  checkout: boolean;
  /// Its node is from before moves.
  old?: boolean;
}

/// The harnesses whose sessions move (the others keep theirs in a database,
/// and wait for later).
const MOVABLE = new Set(["claude", "codex", "pi"]);

const NAMES: Record<string, string> = { claude: "Claude Code", codex: "Codex", pi: "pi" };

/// Whether a session can be offered a move at all.
export const canMove = (s: { agent: string }) => MOVABLE.has(s.agent);

/// What the menu says under a machine, and whether it can be picked. An
/// offline machine can: the move waits for it.
export function targetNote(t: MoveTarget, agent: string): { note: string; enabled: boolean; offline: boolean } {
  if (t.old) return { note: "update this machine first", enabled: false, offline: false };
  if (!t.checkout) return { note: "no checkout: import first", enabled: false, offline: false };
  if (!t.harnesses.includes(agent)) return { note: `no ${NAMES[agent] ?? agent} here`, enabled: false, offline: false };
  if (!t.online) return { note: "offline: moves when it returns", enabled: true, offline: true };
  return { note: "folder ready", enabled: true, offline: false };
}

/// Megabytes with one decimal, or kilobytes under one.
export function fmtBytes(n: number) {
  return n < 1_000_000 ? `${Math.max(1, Math.round(n / 1000))} KB` : `${(n / 1_000_000).toFixed(1)} MB`;
}

export interface MoveRow {
  state: "done" | "run" | "todo" | "note";
  text: string;
}

/// The steps above the composer: the conversation, the checkouts and the
/// session's worktrees, the resume, each done, running or still to come,
/// and a note when uncommitted edits stay on the source.
export function moveRows(p: MoveProgress, from: string, to: string, toOnline = true): MoveRow[] {
  const steps = moveSteps(p, from, to, toOnline);
  return p.edits_left ? [...steps, { state: "note", text: `Uncommitted edits in the project folder stay on ${from}` }] : steps;
}

function moveSteps(p: MoveProgress, from: string, to: string, toOnline: boolean): MoveRow[] {
  const resume = `Resuming on ${to}`;
  const checkouts = "Checkouts and worktrees";

  switch (p.phase) {
    case "waiting":
      return [
        // The caller shows a working turn on its own, so here the turn is
        // over and the target has not taken the session yet.
        { state: "run", text: p.source_away ? `Waiting for ${from}` : toOnline ? `Waiting for ${to}` : `${to} is offline; the move goes on when it is back` },
        { state: "todo", text: "Conversation" },
        { state: "todo", text: checkouts },
        { state: "todo", text: resume },
      ];
    case "conversation":
      return [
        { state: "run", text: p.total > 0 ? `Conversation ${fmtBytes(p.copied)} of ${fmtBytes(p.total)}` : "Conversation" },
        { state: "todo", text: checkouts },
        { state: "todo", text: resume },
      ];
    case "files":
      return [
        { state: "done", text: `Conversation ${fmtBytes(p.total)}` },
        // What it waits for, such as checkouts at two commits, else the
        // worktree coming over.
        { state: "run", text: p.reason ?? (p.worktree ? `Worktree ${p.worktree} ${p.worktree_at} of ${p.worktrees}` : "Checkouts") },
        { state: "todo", text: resume },
      ];
    default:
      return [
        { state: "done", text: `Conversation ${fmtBytes(p.total)}` },
        { state: "done", text: checkouts },
        { state: "run", text: resume },
      ];
  }
}

/// How long a target may take to see a record in the mesh before the
/// window stops saying it waits for it.
const SEEN_WITHIN_MS = 2 * 60_000;

/// What the target says of a move, read for the window: a target that has
/// not heard of a recent move yet still has to take it up, so it waits; one
/// that never heard of an old one (it restarted since) says nothing; a node
/// from before moves cannot take the session at all.
export function effective(p: MoveProgress | null | undefined, record: MoveRecord | undefined, now: number): MoveProgress | null {
  if (!p || !record) return p ?? null;
  if (p.answer === "update") return { ...p, answer: "known", phase: "failed", reason: "that machine's beep-node is too old to take sessions" };
  if (p.answer === "unknown") return now - record.at < SEEN_WITHIN_MS ? { ...p, answer: "known", phase: "waiting" } : null;
  return p;
}

const GOING = new Set<MovePhase>(["waiting", "conversation", "files", "resuming"]);

/// A move still under way.
export const going = (p: MoveProgress | null | undefined) => !!p && p.answer === "known" && GOING.has(p.phase);

/// What a session's row says while its move goes or after it failed: the
/// machines in place of the one it is on, a chip and the reason. `null` for
/// a session with no move, or one that moved or was cancelled.
export function listLabel(
  record: MoveRecord | undefined,
  p: MoveProgress | null | undefined,
  name: (machine: string) => string,
): { sub: string; chip: "Moving" | "Move failed"; why: string | null } | null {
  if (!record || !p || p.answer !== "known") return null;
  if (GOING.has(p.phase)) return { sub: `${name(record.from)} to ${name(record.to)}`, chip: "Moving", why: null };

  if (p.phase === "failed") {
    return { sub: name(record.from), chip: "Move failed", why: `Move failed: ${p.reason ?? "the move did not finish"}. Still on ${name(record.from)}.` };
  }

  return null;
}

/// Sessions with one id on two machines show once: the machine the move
/// record names as the new owner keeps it. A session the target does not
/// list yet (a move that waits, or failed) stays on its source.
export function dedupe<T extends { machine_id: string; id: string; agent: string }>(rows: readonly T[], records: readonly MoveRecord[]): T[] {
  const drop = new Set<T>();

  for (const r of records) {
    const mine = rows.filter(s => s.id === r.session && s.agent === r.agent);
    const onTarget = mine.some(s => s.machine_id === r.to);
    if (!onTarget) continue;
    for (const s of mine) if (s.machine_id === r.from) drop.add(s);
  }

  return rows.filter(s => !drop.has(s));
}

/// Where the session open on `open.machine_id` went: its row on the machine
/// its latest move went to, once that machine lists it; else null.
export function movedTo<T extends { machine_id: string; id: string; agent: string }>(
  open: OpenAgent, records: ReadonlyMap<string, MoveRecord>, rows: readonly T[]): T | null {
  const r = records.get(open.session);
  if (!r || r.from !== open.machine_id || r.agent !== open.agent) return null;
  return rows.find(s => s.machine_id === r.to && s.id === open.session && s.agent === open.agent) ?? null;
}

/// The records by session, the latest of each.
export function bySession(records: readonly MoveRecord[]) {
  const out = new Map<string, MoveRecord>();

  for (const r of records) {
    const had = out.get(r.session);
    if (!had || r.at > had.at) out.set(r.session, r);
  }

  return out;
}
