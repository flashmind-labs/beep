// The menus of a session's composer: `/` at the start of the message lists
// the agent's commands and skills, `@` after whitespace lists files. What
// opens a menu, what it shows for the text typed, what its keys do and what
// a pick writes. No DOM here, so the tests load it.
import { joinPath, mentionText } from "../agentpane/files-state.ts";

/// A slash command or skill the agent offers (AgentCommand).
export interface AgentCommand {
  name: string;
  description: string;
  hint: string;
}

/// The most rows the menus draw: a command list scrolls, a file list is
/// cut to its best matches.
export const MAX_COMMAND_ROWS = 40;
export const MAX_FILE_ROWS = 12;

/// What the text before the caret asks a menu for.
export interface Trigger {
  kind: "command" | "file";
  /// Where the `/` or `@` is.
  start: number;
  /// What was typed after it.
  query: string;
}

/// The menu `text` calls for with the caret at `caret`, or null. A command
/// is a `/` at the very start of the message and the name after it, so a
/// `/` in a path or a sentence opens nothing. A file is an `@` at the start
/// or after whitespace and the word after it, which is how a mention is
/// written; an `@` in an address does not count, and a quoted one is typed
/// by hand.
export function triggerAt(text: string, caret: number): Trigger | null {
  const before = text.slice(0, caret);
  const command = /^\/([^\s/]*)$/.exec(before);
  if (command) return { kind: "command", start: 0, query: command[1] };
  const at = before.lastIndexOf("@");
  if (at < 0 || (at > 0 && !/\s/.test(before[at - 1]))) return null;
  const query = before.slice(at + 1);
  if (/\s/.test(query) || query.startsWith('"')) return null;
  return { kind: "file", start: at, query };
}

/// A row of a menu.
export interface Row {
  /// What a pick writes in place of what was typed, without the space after it.
  insert: string;
  /// What the row is called.
  label: string;
  /// Its description, or a file's folder, in muted text.
  detail: string;
  /// What a command takes after its name; "" for a file.
  hint: string;
}

/// How a command is written: `/name`, except for a skill the agent names
/// with a sigil of its own (`$name`).
export function commandText(name: string) {
  return name.startsWith("$") ? name : `/${name}`;
}

/// The name without its `/` or `$`, as typed text is matched.
const bareName = (name: string) => name.replace(/^[/$]/, "").toLowerCase();

/// The commands that answer `query`, by name then description: a name that
/// starts with it first, then one that holds it, then a description that
/// does, each group in the agent's own order. Everything for an empty query.
export function filterCommands(commands: readonly AgentCommand[], query: string, cap = MAX_COMMAND_ROWS): Row[] {
  const q = query.toLowerCase();
  const scored: { at: number; score: number; command: AgentCommand }[] = [];
  commands.forEach((command, at) => {
    const name = bareName(command.name);
    const score = !q || name.startsWith(q) ? 0 : name.includes(q) ? 1 : command.description.toLowerCase().includes(q) ? 2 : -1;
    if (score >= 0) scored.push({ at, score, command });
  });
  scored.sort((a, b) => a.score - b.score || a.at - b.at);
  return scored.slice(0, cap).map(({ command }) => {
    const label = commandText(command.name);
    return { insert: label, label, detail: command.description, hint: command.hint };
  });
}

/// The files of a session's folder, lowercased once for the matching.
export interface FileIndex {
  paths: readonly string[];
  lower: readonly string[];
}

export const indexFiles = (paths: readonly string[]): FileIndex => ({ paths, lower: paths.map(p => p.toLowerCase()) });

/// Whether the letters of `q` are in `s` in order.
function subsequence(q: string, s: string) {
  let at = 0;
  for (let i = 0; i < s.length && at < q.length; i++) if (s[i] === q[at]) at++;
  return at === q.length;
}

/// The files that answer `query`, best first, at most `cap`: those whose name
/// starts with it, then whose name holds it, then whose path holds it, then
/// whose path has its letters in order; nearer the top and shorter first.
/// With nothing typed, the files nearest the top. `shown` turns a path
/// into what a message names it.
export function filterFiles(index: FileIndex, query: string, shown: (path: string) => string = p => p, cap = MAX_FILE_ROWS): Row[] {
  const q = query.toLowerCase();
  const found: { path: string; score: number; depth: number }[] = [];
  for (let i = 0; i < index.paths.length; i++) {
    const path = index.paths[i], lower = index.lower[i];
    const base = lower.slice(lower.lastIndexOf("/") + 1);
    const score = !q ? 0 : base.startsWith(q) ? 0 : base.includes(q) ? 1 : lower.includes(q) ? 2 : subsequence(q, lower) ? 3 : -1;
    if (score < 0) continue;
    found.push({ path, score, depth: path.split("/").length });
  }
  found.sort((a, b) => a.score - b.score || a.depth - b.depth || a.path.length - b.path.length || (a.path < b.path ? -1 : 1));
  return found.slice(0, cap).map(({ path }) => {
    const slash = path.lastIndexOf("/");
    return { insert: mentionText([shown(path)]), label: path.slice(slash + 1), detail: slash < 0 ? "" : path.slice(0, slash + 1), hint: "" };
  });
}

/// What a key does with a menu of `count` rows with row `index` lit:
/// - `move` lights another row (Up and Down, round the ends)
/// - `accept` picks the lit row (Tab or Enter, which then does not send)
/// - `close` shuts the menu and leaves the text (Escape)
/// - `none` leaves the key to the field
export type MenuAction = { type: "none" } | { type: "move"; index: number } | { type: "accept" } | { type: "close" };

/// The modifier keys held with a key, and whether an input method is
/// composing, which owns every key then.
export interface KeyState {
  shift: boolean;
  alt: boolean;
  ctrl: boolean;
  meta: boolean;
  composing: boolean;
}

export function menuKey(count: number, index: number, key: string, mods: KeyState): MenuAction {
  if (!count || mods.composing || mods.shift || mods.alt || mods.ctrl || mods.meta) return { type: "none" };
  const at = Math.min(Math.max(index, 0), count - 1);
  switch (key) {
    case "ArrowDown": return { type: "move", index: (at + 1) % count };
    case "ArrowUp": return { type: "move", index: (at - 1 + count) % count };
    case "Tab":
    case "Enter": return { type: "accept" };
    case "Escape": return { type: "close" };
    default: return { type: "none" };
  }
}

/// `text` after a pick: `row` takes the place of what the trigger covers up
/// to the caret, a space follows it unless one does, and the caret goes past.
export function picked(text: string, caret: number, trigger: Trigger, row: Row) {
  const head = text.slice(0, trigger.start), tail = text.slice(caret);
  const gap = tail.startsWith(" ") ? "" : " ";
  return { text: `${head}${row.insert}${gap}${tail}`, caret: head.length + row.insert.length + 1 };
}

/// Files of the tree's listings as paths, folders left out and what git
/// ignores too (the node does not read into it).
export function filePaths(listings: ReadonlyMap<string, { entries: { name: string; folder: boolean; ignored: boolean }[] | null }>) {
  const out: string[] = [];
  for (const [dir, listing] of listings) {
    for (const e of listing.entries ?? []) if (!e.folder && !e.ignored) out.push(joinPath(dir, e.name));
  }
  return out;
}
