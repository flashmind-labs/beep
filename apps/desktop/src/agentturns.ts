// A coding agent's log in turns: each message from the user and what the
// agent did after it, up to the next message. A finished turn folds its
// steps into one line and keeps only its answer in view.

import { parseTool } from "./agentlog.ts";

/// The parts of an entry a turn needs.
export interface LogEntry {
  kind: string;
  text: string;
}

/// One turn, by the entries' places in the session.
export interface Turn {
  /// The place of the message that opened it, or of its first entry when
  /// the log shown starts in the middle of a turn. Turns are kept by it.
  key: number;
  /// The user's message, or null when the log starts mid-turn.
  user: number | null;
  /// Everything the agent did before its answer: tools, interim text,
  /// questions.
  steps: number[];
  /// The agent's text at the end of the turn, after its last step.
  answer: number[];
}

/// Splits entries into turns. `offset` is the place of the first entry in
/// the session.
export function splitTurns(entries: LogEntry[], offset = 0): Turn[] {
  const turns: Turn[] = [];
  entries.forEach((en, i) => {
    const at = offset + i;
    if (en.kind === "user") turns.push({ key: at, user: at, steps: [], answer: [] });
    else {
      if (!turns.length) turns.push({ key: at, user: null, steps: [], answer: [] });
      turns[turns.length - 1].steps.push(at);
    }
  });
  // The answer is the run of agent text the turn ends with.
  for (const t of turns) {
    while (t.steps.length && entries[t.steps[t.steps.length - 1] - offset].kind === "agent") t.answer.unshift(t.steps.pop()!);
  }
  return turns;
}

/// Whether a turn shows its steps as one line: only a finished turn with
/// steps that the user has not opened. The running turn always shows them.
export function folded(t: Turn, running: boolean, opened: boolean) {
  return !running && !opened && t.steps.length > 0;
}

export type ToolKind = "command" | "edit" | "read" | "search" | "web" | "other";

const NAMED: Record<string, ToolKind> = {
  bash: "command", shell: "command", exec: "command", run: "command", terminal: "command", execute: "command",
  edit: "edit", multiedit: "edit", write: "edit", notebookedit: "edit", delete: "edit", move: "edit", create: "edit", patch: "edit",
  read: "read", view: "read", cat: "read", open: "read",
  grep: "search", glob: "search", search: "search", find: "search", ls: "search", list: "search", rg: "search",
  webfetch: "web", websearch: "web", fetch: "web",
};

/// What kind of tool an entry's text names. A transcript entry names its
/// tool (`Bash: cargo test`); a live one has a title, which starts with a
/// verb (`Read src/main.rs`) or is the command itself (`cargo test`).
export function toolKind(text: string): ToolKind {
  const title = parseTool(text).title.trim();
  const named = /^([A-Za-z]+): /.exec(title);
  if (named) return NAMED[named[1].toLowerCase()] ?? "other";
  if (/^web search\b/i.test(title)) return "web";
  return NAMED[title.split(/\s/)[0].toLowerCase()] ?? "command";
}

/// What the tool acts on, so editing one file twice counts it once.
function target(text: string) {
  return parseTool(text).title.trim().replace(/^[A-Za-z]+:? /, "");
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/// A turn's steps as one line: "Ran 1 command, edited 2 files".
export function summary(entries: LogEntry[], t: Turn, offset = 0) {
  const tools = t.steps.map(at => entries[at - offset]).filter(en => en.kind === "tool");
  const of = (k: ToolKind) => tools.filter(en => toolKind(en.text) === k);
  const files = (k: ToolKind) => new Set(of(k).map(en => target(en.text))).size;
  const parts = [
    of("command").length && `ran ${plural(of("command").length, "command", "commands")}`,
    files("edit") && `edited ${plural(files("edit"), "file", "files")}`,
    files("read") && `read ${plural(files("read"), "file", "files")}`,
    of("search").length && `searched ${of("search").length === 1 ? "once" : `${of("search").length} times`}`,
    of("web").length && `made ${plural(of("web").length, "web request", "web requests")}`,
    of("other").length && `used ${plural(of("other").length, "other tool", "other tools")}`,
  ].filter(Boolean).join(", ");
  const line = parts || plural(t.steps.length, "step", "steps");
  return line[0].toUpperCase() + line.slice(1);
}

/// A duration: "48s", "1m 12s", "2h 5m".
export function duration(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor(s / 60) % 60}m`;
}

/// A time of day as HH:MM, in local time.
export function clock(ms: number) {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/// How a turn stands: finished, the agent working, or waiting on an answer.
export type TurnState = "done" | "working" | "waiting";

/// The turn's heading, with its length when it is known.
export function heading(state: TurnState, ms: number | null) {
  const d = ms == null ? "" : duration(ms);
  if (state === "waiting") return d ? `Waiting for you · ${d}` : "Waiting for you";
  if (state === "working") return d ? `Working for ${d}` : "Working";
  return d ? `Worked for ${d}` : "Worked";
}

/// What is known of when a session's turns ran. Entries carry no times,
/// so a turn's start and end come from the session (its first turn starts
/// with it, its last ends at its last activity) or from when this window
/// saw the turn begin or end.
export interface TurnTimes {
  /// The turn opens the session.
  first: boolean;
  /// The turn is the newest.
  last: boolean;
  running: boolean;
  /// When this window saw it start and end, if it did.
  seenStart?: number;
  seenEnd?: number;
  sessionStart?: number;
  lastActivity?: number;
}

/// When a turn started and ended, as far as the session says.
export function span(t: TurnTimes): { start?: number; end?: number } {
  const start = t.first && t.sessionStart ? t.sessionStart : t.seenStart;
  const end = t.running ? undefined : t.seenEnd ?? (t.last && t.lastActivity ? t.lastActivity : undefined);
  return { start, end: start != null && end != null && end < start ? undefined : end };
}
