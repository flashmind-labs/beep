// Moving a coding agent session to another machine, apart from the DOM so it
// can be tested: the menu's notes, the steps, what the lists say and which
// machine's copy of a session they show.
import type { AgentSession, OpenAgent } from "./agents-state.ts";
import { starters } from "./nesting-state.ts";
import { transferSpeed } from "../agentpane/files-state.ts";

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
  /// Bytes of a chat's files copied, and what they take on the source.
  files_copied: number;
  files_total: number;
}

/// When the window first saw a move's files come, and now, in ms.
export interface FilesTiming { since: number; now: number }

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
const MOVABLE = new Set(["claude", "codex", "pi", "flsh"]);

const NAMES: Record<string, string> = { claude: "Claude Code", codex: "Codex", pi: "pi", flsh: "flsh" };

/// Whether a session can be offered a move at all.
export const canMove = (s: { agent: string }) => MOVABLE.has(s.agent);

/// What the menu says under a machine, and whether it can be picked: it
/// needs every harness of `agents` (a chat's threads may use several). An
/// offline machine can: the move waits for it.
export function targetNote(t: MoveTarget, agents: readonly string[]): { note: string; enabled: boolean; offline: boolean } {
  if (t.old) return { note: "update this machine first", enabled: false, offline: false };
  if (!t.checkout) return { note: "no checkout: import first", enabled: false, offline: false };
  const missing = agents.find(agent => !t.harnesses.includes(agent));
  if (missing) return { note: `no ${NAMES[missing] ?? missing} here`, enabled: false, offline: false };
  if (!t.online) return { note: "offline: moves when it returns", enabled: true, offline: true };
  return { note: "folder ready", enabled: true, offline: false };
}

/// The sessions that move with `chat`, the chat first, then its threads and
/// theirs, oldest first, and the harnesses they use. A reviewer works on a
/// project, not in the chat's folder, and stays, as does an ended thread that
/// worked in another folder. Refused while a thread is on another machine,
/// or runs outside the chat's folder: the chat leaving would close it.
export function chatSessions(rows: readonly AgentSession[], chat: AgentSession): { sessions: AgentSession[]; agents: string[] } | { refused: string } {
  const from = starters(rows), inChat = new Set<AgentSession>([chat]);
  const threads: AgentSession[] = [];

  // Agents start agents three deep at most, so three passes find them all.
  for (let depth = 0; depth < 3; depth += 1) {
    for (const row of rows) {
      const parent = row.reviewer || inChat.has(row) ? undefined : from(row);
      if (parent && inChat.has(parent)) { inChat.add(row); threads.push(row); }
    }
  }

  const inFolder = (row: AgentSession) => row.cwd.startsWith(`${chat.cwd}/`) || row.cwd.startsWith(`${chat.cwd}\\`);
  const moving = threads.filter(row => inFolder(row) || row.status !== "ended");
  const away = moving.find(row => row.machine_id !== chat.machine_id);
  if (away) return { refused: `a thread of this chat is on ${away.machine_name}` };
  if (moving.some(row => !inFolder(row))) return { refused: "a thread works outside the chat's folder until it ends" };
  const sessions = [chat, ...moving.sort((a, b) => a.started_at - b.started_at)];

  return { sessions, agents: [...new Set(sessions.map(row => row.agent))] };
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
export function moveRows(p: MoveProgress, from: string, to: string, toOnline = true, what: "session" | "chat" = "session", timing: FilesTiming | null = null): MoveRow[] {
  const steps = moveSteps(p, from, to, toOnline, what, timing);
  return p.edits_left ? [...steps, { state: "note", text: `Uncommitted edits in the project folder stay on ${from}` }] : steps;
}

function moveSteps(p: MoveProgress, from: string, to: string, toOnline: boolean, what: "session" | "chat", timing: FilesTiming | null): MoveRow[] {
  const resume = `Resuming on ${to}`;
  // A chat brings the files of its folder; a session the project's checkouts.
  const [checkouts, copying] = what === "chat" ? ["Files", filesLine(p, timing)] : ["Checkouts and worktrees", "Checkouts"];

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
        { state: "run", text: p.reason ?? (p.worktree ? `Worktree ${p.worktree} ${p.worktree_at} of ${p.worktrees}` : copying) },
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

/// How full the bar over the input is: the share of the conversation, or of a chat's files,
/// copied, below 100 until the step is over. Null for a step with no size.
export function movePct(p: MoveProgress): number | null {
  const [done, size] = p.phase === "conversation" ? [p.copied, p.total] : p.phase === "files" ? [p.files_copied, p.files_total] : [0, 0];
  return size > 0 ? Math.min(99, Math.round((done * 100) / size)) : null;
}

/// A chat's files as they come: how much of how much, and their speed once they are slow.
function filesLine(p: MoveProgress, timing: FilesTiming | null) {
  const pct = movePct(p);
  if (pct === null) return "Files";
  const speed = timing && transferSpeed({ started: timing.since, done: p.files_copied, size: p.files_total }, timing.now);

  return `Files ${fmtBytes(p.files_copied)} of ${fmtBytes(p.files_total)} · ${pct}%${speed ? ` · ${speed}` : ""}`;
}

/// A new move may still be reaching its target through the mesh.
export const awaitingMove = (record: MoveRecord, now: number) => now - record.at < 2 * 60_000;

/// What the target says of a move, read for the window: a target that has
/// not heard of a recent move yet still has to take it up, so it waits; one
/// that never heard of an old one (it restarted since) says nothing; a node
/// from before moves cannot take the session at all.
export function effective(p: MoveProgress | null | undefined, record: MoveRecord | undefined, now: number): MoveProgress | null {
  if (!p || !record) return p ?? null;
  if (p.answer === "update") return { ...p, answer: "known", phase: "failed", reason: "that machine's beep-node is too old to take sessions" };
  if (p.answer === "unknown") return awaitingMove(record, now) ? { ...p, answer: "known", phase: "waiting" } : null;
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
