// A coding agent's log in turns: each message from the user and what the agent
// did after it, up to the next message.

import { imageLines, parseTool, subagent } from "./agentlog.ts";
import { plural } from "../ui/format.ts";

/// The parts of an entry a turn needs.
export interface LogEntry {
  kind: string;
  text: string;
  /// How long a thought took, once it ended.
  ms?: number | null;
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

export type ToolKind = "command" | "edit" | "read" | "search" | "web" | "beep" | "mcp" | "other";

/// A call to the session's own Beep tools, read from the title the node
/// words it in (`Beep: Run on build-box: cargo test`): what it did, done
/// and while it runs, the machine, the command, prompt, project, job or
/// question, and how many files it sends. Fields it has none of are "".
export interface BeepCall {
  done: string;
  doing: string;
  machine: string;
  detail: string;
  files: string;
}

const BEEP_PREFIX = "Beep: ";

/// The Beep call a tool title names, or null for any other tool.
export function beepCall(title: string): BeepCall | null {
  const t = title.trim();
  if (!t.startsWith(BEEP_PREFIX)) return null;
  const rest = t.slice(BEEP_PREFIX.length);
  const call = (done: string, doing: string, o: Partial<BeepCall> = {}): BeepCall => {
    const machine = o.machine === "a machine" ? "" : o.machine ?? "";
    return { done, doing, machine, detail: o.detail ?? "", files: o.files ?? "" };
  };
  let m: RegExpExecArray | null;
  if (rest === "List machines") return call("Listed machines", "Listing machines");
  if ((m = /^Run on (.+?)(?:: ([\s\S]*))?$/.exec(rest))) return call("Ran on", "Running on", { machine: m[1], detail: m[2] });
  if ((m = /^Start (\S+|an agent) on (.+?)(?:: ([\s\S]*))?$/.exec(rest))) return call(`Started ${m[1]} on`, `Starting ${m[1]} on`, { machine: m[2], detail: m[3] });
  if ((m = /^Sync (.+?)(?: to (.+))?$/.exec(rest))) return call("Synced", "Syncing", { detail: m[1], machine: m[2] });
  if ((m = /^Send (\d+) (files?) to (.+)$/.exec(rest))) return call("Sent files to", "Sending files to", { machine: m[3], files: `${m[1]} ${m[2]}` });
  if ((m = /^Check job ?(.*)$/.exec(rest))) return call("Checked job", "Checking job", { detail: m[1] });
  if ((m = /^Cancel job ?(.*)$/.exec(rest))) return call("Cancelled job", "Cancelling job", { detail: m[1] });
  if ((m = /^Ask(?:: ([\s\S]*))?$/.exec(rest))) return call("Asked you", "Asking you", { detail: m[1] });
  return call(rest, rest);
}

const BEEP_VERBS: Record<string, [string, string]> = {
  List: ["Listed", "Listing"], Run: ["Ran", "Running"], Start: ["Started", "Starting"], Sync: ["Synced", "Syncing"],
  Send: ["Sent", "Sending"], Check: ["Checked", "Checking"], Cancel: ["Cancelled", "Cancelling"], Ask: ["Asked", "Asking"],
};

/// A Beep call's title as one line in the past tense, or while it runs in
/// the present: "Ran on build-box: cargo test". Null for any other tool.
export function beepLine(title: string, running = false) {
  const t = title.trim();
  if (!t.startsWith(BEEP_PREFIX)) return null;
  const rest = t.slice(BEEP_PREFIX.length);
  const first = /^[A-Za-z]+/.exec(rest)?.[0] ?? "";
  const verb = BEEP_VERBS[first];
  return verb ? verb[running ? 1 : 0] + rest.slice(first.length) : rest;
}

const NAMED: Record<string, ToolKind> = {
  beep: "beep",
  bash: "command", shell: "command", exec: "command", run: "command", terminal: "command", execute: "command",
  edit: "edit", multiedit: "edit", write: "edit", notebookedit: "edit", delete: "edit", move: "edit", create: "edit", patch: "edit",
  read: "read", view: "read", cat: "read", open: "read",
  grep: "search", glob: "search", search: "search", find: "search", ls: "search", list: "search", rg: "search",
  webfetch: "web", websearch: "web", fetch: "web",
};

/// What kind of tool an entry's text names.
export function toolKind(text: string): ToolKind {
  const title = parseTool(text).title.trim();
  if (MCP.test(title)) return "mcp";
  const named = /^([A-Za-z]+): /.exec(title);
  if (named) return NAMED[named[1].toLowerCase()] ?? "other";
  if (/^web search\b/i.test(title)) return "web";
  return NAMED[title.split(/\s/)[0].toLowerCase()] ?? "command";
}

/// An MCP server's tool as Claude Code names it: `mcp__linear__get_issue`.
const MCP = /^mcp__([A-Za-z0-9_-]+?)__([A-Za-z0-9_-]+)(.*)$/s;

/// A tool row's parts: its kind, the verb that leads it, what it acts on
/// and what the right side says (an edit's +2 -1, a read's line range,
/// a failure's exit code). `bad` marks the detail of a failure.
export interface RowParts {
  kind: ToolKind;
  verb: string;
  target: string;
  detail: string;
  bad: boolean;
}

/// Verbs a live title may start with, done and while it runs.
const VERBS: Record<string, [string, string]> = {
  run: ["Ran", "Running"], ran: ["Ran", "Running"], running: ["Ran", "Running"],
  read: ["Read", "Reading"], reading: ["Read", "Reading"],
  edit: ["Edited", "Editing"], edited: ["Edited", "Editing"], editing: ["Edited", "Editing"],
  write: ["Wrote", "Writing"], wrote: ["Wrote", "Writing"],
  search: ["Searched", "Searching"], searched: ["Searched", "Searching"], searching: ["Searched", "Searching"],
  fetch: ["Fetched", "Fetching"], fetched: ["Fetched", "Fetching"], fetching: ["Fetched", "Fetching"],
  list: ["Listed", "Listing"], find: ["Found", "Finding"], create: ["Created", "Creating"],
  delete: ["Deleted", "Deleting"], move: ["Moved", "Moving"],
};

const RANGE = /\s*(?:\((\d+)\s*-\s*(\d+)\)|:(\d+)(?:-(\d+))?)$/;

/// A tool entry as the parts of its row. `output` is what it showed.
export function rowParts(text: string, running = false): RowParts {
  const { title, status, output } = parseTool(imageLines(text).text);
  const kind = toolKind(text), failed = status === "failed";
  const exit = failed ? /(?:exit(?:ed with)?(?: code)?|status) (\d+)/i.exec(output)?.[1] : undefined;
  const bad = failed;
  let detail = failed ? (exit ? `exit ${exit}` : "failed") : "";
  const mcp = MCP.exec(title.trim());
  if (mcp) return { kind, verb: mcp[1], target: (mcp[2] + mcp[3]).trim(), detail: detail || "MCP", bad };
  const label = stepLabel(text, running).replace(/ …$/, "");
  const first = /^(\S+)\s+([\s\S]*)$/.exec(label);
  const known = first ? VERBS[first[1].toLowerCase()] : undefined;
  let verb = known ? known[running ? 1 : 0] : kind === "command" ? (running ? "Running" : "Ran") : "Used";
  let target = known ? first![2] : label;
  if (!known && kind === "other") {
    const named = /^([A-Za-z]+): ([\s\S]*)$/.exec(label);
    verb = named ? named[1] : label;
    target = named ? named[2] : "";
  }
  if (kind === "edit" && !failed) {
    const d = diffBlocks(output);
    if (d.length) detail = `+${d.reduce((n, b) => n + b.added, 0)} -${d.reduce((n, b) => n + b.removed, 0)}`;
  }
  if (kind === "read") {
    const r = RANGE.exec(target);
    if (r) { detail = detail || `L${r[1] ?? r[3]}${(r[2] ?? r[4]) ? `-${r[2] ?? r[4]}` : ""}`; target = target.slice(0, r.index); }
  }
  return { kind, verb, target: target.trim(), detail, bad };
}

/// One file's change as the node words it under an edit: `[diff] path +2
/// -1`, then lines marked " ", "-" or "+", and `… N more lines` when cut.
export interface DiffBlock {
  path: string;
  added: number;
  removed: number;
  lines: { mark: " " | "-" | "+"; text: string }[];
  more: number;
}

/// The diffs in a tool's output; none when it shows none.
export function diffBlocks(output: string): DiffBlock[] {
  const out: DiffBlock[] = [];
  for (const line of output.split("\n")) {
    const head = /^\[diff\] (.+) \+(\d+) -(\d+)$/.exec(line);
    if (head) { out.push({ path: head[1], added: Number(head[2]), removed: Number(head[3]), lines: [], more: 0 }); continue; }
    const cur = out.at(-1);
    if (!cur) continue;
    const more = /^… (\d+) more lines$/.exec(line);
    if (more) cur.more = Number(more[1]);
    else if (line[0] === "+" || line[0] === "-" || line[0] === " ") cur.lines.push({ mark: line[0], text: line.slice(1) });
  }
  return out;
}

/// What an output shows besides its diffs.
export function withoutDiffs(output: string) {
  const keep: string[] = [];
  let inDiff = false;
  for (const line of output.split("\n")) {
    if (/^\[diff\] /.test(line)) { inDiff = true; continue; }
    if (inDiff && (/^[ +-]/.test(line) || /^… \d+ more lines$/.test(line))) continue;
    inDiff = false;
    keep.push(line);
  }
  return keep.join("\n").trim();
}

/// Steps in order, a run of two or more finished calls of one kind as one
/// group.
export function toolRuns(entries: LogEntry[], ats: number[], offset = 0): number[][] {
  const out: number[][] = [];
  const kindOf = (at: number): ToolKind | null => {
    const en = entries[at - offset];
    if (en.kind !== "tool") return null;
    const { text, names } = imageLines(en.text);
    const { status, output } = parseTool(text);
    if (status !== "completed" || names.length || subagent(output)) return null;
    const k = toolKind(en.text);
    return k === "beep" ? null : k;
  };
  let run: number[] = [], runKind: ToolKind | null = null;
  const flush = () => { if (run.length) out.push(...(run.length > 1 ? [run] : [[run[0]]])); run = []; runKind = null; };
  for (const at of ats) {
    const k = kindOf(at);
    if (k && k === runKind) { run.push(at); continue; }
    flush();
    if (k) { run = [at]; runKind = k; } else out.push([at]);
  }
  flush();
  return out;
}

/// A run's line: "Read 6 files", "Ran 3 commands".
export function runLabel(kind: ToolKind, n: number) {
  switch (kind) {
    case "read": return `Read ${plural(n, "file", "files")}`;
    case "edit": return `Edited ${plural(n, "file", "files")}`;
    case "search": return `Searched ${n} times`;
    case "command": return `Ran ${plural(n, "command", "commands")}`;
    case "web": return `Fetched ${plural(n, "page", "pages")}`;
    default: return `Used ${plural(n, "tool", "tools")}`;
  }
}

/// What a turn did, for its heading: "12 tool calls · 3 files changed".
export function turnStats(entries: LogEntry[], t: Turn, offset = 0) {
  const tools = t.steps.map(at => entries[at - offset]).filter(en => en.kind === "tool");
  const files = new Set(tools.filter(en => toolKind(en.text) === "edit" && parseTool(en.text).status !== "failed").map(en => target(en.text))).size;
  return [tools.length && plural(tools.length, "tool call", "tool calls"), files && `${plural(files, "file", "files")} changed`].filter(Boolean).join(" · ");
}

/// What the tool acts on, so editing one file twice counts it once.
function target(text: string) {
  return parseTool(text).title.trim().replace(/^[A-Za-z]+:? /, "");
}


/// A turn's steps as one line, naming at most two kinds, commands and edits
/// first, and counting the rest: "Ran 3 commands (1 failed), edited 2 files,
/// performed 4 other actions".
export function summary(entries: LogEntry[], t: Turn, offset = 0, steps = t.steps) {
  const tools = steps.map(at => entries[at - offset]).filter(en => en.kind === "tool");
  const of = (k: ToolKind) => tools.filter(en => toolKind(en.text) === k);
  const files = (k: ToolKind) => new Set(of(k).map(en => target(en.text))).size;
  const withFailed = (text: string, list: LogEntry[]) => {
    const n = list.filter(en => parseTool(imageLines(en.text).text).status === "failed").length;
    return n ? `${text} (${n} failed)` : text;
  };
  const kinds: { kind: ToolKind; text: string | 0 }[] = [
    { kind: "command", text: of("command").length && `ran ${plural(of("command").length, "command", "commands")}` },
    { kind: "edit", text: files("edit") && `edited ${plural(files("edit"), "file", "files")}` },
    { kind: "read", text: files("read") && `read ${plural(files("read"), "file", "files")}` },
    { kind: "search", text: of("search").length && `searched ${of("search").length === 1 ? "once" : `${of("search").length} times`}` },
    { kind: "web", text: of("web").length && `made ${plural(of("web").length, "web request", "web requests")}` },
    { kind: "beep", text: of("beep").length && `used Beep ${of("beep").length === 1 ? "once" : `${of("beep").length} times`}` },
    { kind: "other", text: of("other").length && `used ${plural(of("other").length, "other tool", "other tools")}` },
  ];
  const named = kinds.filter(k => k.text).slice(0, 2);
  const rest = tools.filter(en => !named.some(k => k.kind === toolKind(en.text)));
  // Pictures the agent looked at fold away with the steps, counted.
  const images = tools.reduce((n, en) => n + imageLines(en.text).names.length, 0);
  const parts = [...named.map(k => withFailed(String(k.text), of(k.kind))), images && `viewed ${plural(images, "image", "images")}`,
    rest.length && withFailed(`performed ${plural(rest.length, "other action", "other actions")}`, rest)].filter(Boolean).join(", ");
  const line = parts || plural(steps.length, "step", "steps");
  return line[0].toUpperCase() + line.slice(1);
}

/// A step as one short line: "Ran cargo test", "Read src/x.rs", "Running cargo
/// test" while it runs.
export function stepLabel(text: string, running = false, whole = false) {
  const { title } = parseTool(text);
  const beep = beepLine(title, running);
  if (beep != null) {
    const lines = beep.split("\n");
    return whole || lines.length === 1 ? beep : `${lines[0]} …`;
  }
  const lines = title.trim().split("\n");
  const line = whole ? title.trim() : lines.length > 1 ? `${lines[0]} …` : lines[0];
  const named = /^([A-Za-z]+): ([\s\S]*)$/.exec(line);
  const kind = toolKind(text), what = named ? named[2] : line;
  const verb: Partial<Record<ToolKind, [string, string]>> = {
    command: ["Ran", "Running"], edit: ["Edited", "Editing"], read: ["Read", "Reading"], search: ["Searched", "Searching"], web: ["Fetched", "Fetching"],
  };
  const v = verb[kind];
  // A live title already starts with its verb ("Read src/x.rs"), except a
  // command, which is the command itself.
  if (!named && kind !== "command") return line;
  if (!v) return line;
  return `${v[running ? 1 : 0]} ${what}`;
}

/// A tool's title without the harness's name for the tool (`exec: `,
/// `Bash: `).
export function commandOf(title: string) {
  return title.trim().replace(/^[A-Za-z]+: /, "");
}

const squash = (s: string) => s.replace(/\s+/g, " ").trim();

/// What an opened step shows of its title under its label: the command
/// without the tool's name, or "" when the label already says it, so a
/// command never prints twice.
export function commandDetail(title: string, label: string) {
  const cmd = squash(commandOf(title)), shown = squash(label);
  if (!cmd || shown === cmd || shown.endsWith(` ${cmd}`)) return "";
  return commandOf(title);
}

/// Whether a step's row starts open: a failed step only while it is the newest
/// of a running turn, so earlier failures stay one line.
export function stepOpen(failed: boolean, newest: boolean, toggled: boolean) {
  return toggled !== (failed && newest);
}

/// The steps of a turn that stay in view when it folds: notes (a model switch,
/// a compaction), subagents and the agent's own words that showed pictures.
export function keptSteps(entries: LogEntry[], t: Turn, offset = 0) {
  return t.steps.filter(at => {
    const en = entries[at - offset];
    if (en.kind === "other") return true;
    if (en.kind !== "tool") return imageLines(en.text).names.length > 0;
    return subagent(parseTool(imageLines(en.text).text).output) != null;
  });
}

/// `kept` with the turn's thoughts added when no tool is left to fold:
/// thoughts fold with tools, and with none there is no line to open.
export function keptThoughts(entries: LogEntry[], t: Turn, kept: Set<number>, offset = 0) {
  const folds = t.steps.some(at => !kept.has(at) && entries[at - offset].kind === "tool");
  if (!folds) for (const at of t.steps) if (entries[at - offset].kind === "thought") kept.add(at);
  return kept;
}

/// Whether a turn with no message of its own is only notes, which show as
/// they are, without a heading.
export function onlyNotes(entries: LogEntry[], t: Turn, offset = 0) {
  return t.user == null && !t.answer.length && t.steps.every(at => entries[at - offset].kind === "other");
}

/// A duration: "48s", "1m 12s", "2h 5m".
export function duration(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor(s / 60) % 60}m`;
}

/// A thought's line once it ended: "Thought for 12s", or "Thought" when
/// its length is not known (a replayed session, an older node).
export function thoughtLabel(ms?: number | null) {
  return ms == null ? "Thought" : `Thought for ${duration(Math.max(ms, 1000))}`;
}

/// The place of the thought still arriving: the running turn's last entry,
/// a thought without a length. Null when there is none.
export function liveThought(entries: LogEntry[], t: Turn, running: boolean, offset = 0) {
  if (!running || t.answer.length) return null;
  const at = t.steps.at(-1);
  if (at == null) return null;
  const en = entries[at - offset];
  return en.kind === "thought" && en.ms == null ? at : null;
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

/// What is known of when a session's turns ran.
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
