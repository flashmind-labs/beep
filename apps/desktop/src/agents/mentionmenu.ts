// The `/` and `@` menus of a session's composer and of the start fields: the
// agent's commands and skills while the message starts with `/`, the folder's
// files after an `@`. The commands are asked of the machine when a menu
// opens, the files are read once and kept. What opens a menu, what it lists
// and what its keys do is in mention-state.ts.
import { type Tree, mergeTree, unreadFolders, type Listing } from "../agentpane/files-state.ts";
import { paneRoot } from "../agentpane/worktree.ts";
import { mentioned } from "../agentpane/worktree-state.ts";
import { showAgentMenu } from "./composer-ui.tsx";
import { S } from "../bar/core.ts";
import { paneShown } from "../ui/active.ts";
import { invoke } from "../ui/tauri.ts";
import { AG, type AgentSession } from "./agents.ts";
import { sessions } from "./sessions.ts";
import {
  type AgentCommand, type FileIndex, type Row, type Trigger,
  filePaths, filterCommands, filterFiles, indexFiles, menuKey, picked, triggerAt,
} from "./mention-state.ts";

/// A list of commands this old is asked for again when a menu opens.
const COMMANDS_FRESH_MS = 5000;
/// Files this old are read again in the background when a menu opens.
const FILES_FRESH_MS = 60_000;
/// Trees read at once, and the files the walk stops at.
const WALK_AT_ONCE = 8;
const WALK_FILES = 50_000;

interface Cached<T> {
  key: string;
  at: number;
  value: T;
}

let commands: Cached<AgentCommand[]> | null = null;
let askedCommands = "";
let files: Cached<FileIndex> | null = null;
let walking = "";

/// Asks the session's machine for its commands, then `again`. A machine
/// that cannot say (an older node, a failed read) offers none.
async function readCommands(key: string, machine: string, session: string, again: () => void) {
  if (askedCommands === key) return;
  askedCommands = key;
  let value: AgentCommand[] = [];

  try {
    value = await invoke<AgentCommand[]>("agent_commands", { machineId: machine, session });
  } catch {
    // No commands: the menu does not open.
  }

  askedCommands = "";
  commands = { key, at: Date.now(), value };
  again();
}

/// Reads the session's tree to its files, showing what is read so far through `again`.
async function readFiles(key: string, machine: string, root: string, again: () => void) {
  if (walking === key) return;
  walking = key;
  const listings = new Map<string, Listing>();
  let next = [""];

  try {
    while (next.length && walking === key) {
      const got = await Promise.all(next.map(path => invoke<Tree>("agent_tree", { machineId: machine, root, path, ignored: false, deep: true })));
      for (const tree of got) mergeTree(listings, tree);
      const paths = filePaths(listings);
      files = { key, at: Date.now(), value: indexFiles(paths) };
      again();
      if (paths.length >= WALK_FILES) break;
      next = unreadFolders(listings).slice(0, WALK_AT_ONCE);
    }
  } catch {
    // Keeps the files read so far.
  }

  if (walking === key) walking = "";
}

/// Where a field's menus look: the folder on `machine` whose files are
/// offered, under `cwd`, and the running session whose commands are, on its
/// own machine (null offers no commands).
export interface MenuPlace {
  machine: string;
  root: string;
  cwd: string;
  commands: { machine: string; session: string } | null;
}

/// The open session page's place, while its pane shows.
function sessionPlace(): MenuPlace | null {
  if (S.view !== "agent" || !paneShown()) return null;
  const s = AG.page?.session;
  return s && !s.external ? { machine: s.machine_id, root: paneRoot() || s.cwd, cwd: s.cwd, commands: { machine: s.machine_id, session: s.id } } : null;
}

/// The menus for an agent `agent` about to start in `cwd` on `machine`. A
/// harness reports its commands only while a session runs, and a node
/// answers only for the sessions it runs, so the commands are those of the
/// latest running Beep session of that harness: in `cwd`, else on `machine`,
/// else on any machine. Another machine's list is a best effort: the
/// built-in commands match, its skills may not.
export function menuPlaceFor(machine: string, agent: string, cwd: string): MenuPlace {
  const latest = (rows: AgentSession[]) => rows.reduce<AgentSession | null>((best, s) => !best || s.last_activity > best.last_activity ? s : best, null);
  const running = sessions.filter(s => s.agent === agent && !s.external && s.status !== "ended" && s.status !== "closing");
  const here = running.filter(s => s.machine_id === machine);
  const session = latest(here.filter(s => s.cwd === cwd)) ?? latest(here) ?? latest(running);
  return { machine, root: cwd, cwd, commands: session && { machine: session.machine_id, session: session.id } };
}

/// Makes `field` offer commands and files as it is typed in, for the place
/// `place` gives (null while it offers none; a session's page by default).
/// Call it once for each field drawn.
export function bindMentionMenu(field: HTMLTextAreaElement, place: () => MenuPlace | null = sessionPlace) {
  let open = false;
  let rows: Row[] = [];
  let trigger: Trigger | null = null;
  let index = 0;
  let composing = false;

  const refresh = () => { open = rows.length > 0; showAgentMenu(rows, index, pick); };
  const close = () => {
    if (!open) return;
    rows = []; trigger = null; refresh();
  };
  const light = (to: number) => { index = to; refresh(); };
  const draw = (found: Row[], t: Trigger) => {
    const same = trigger?.kind === t.kind && trigger.start === t.start;
    rows = found; trigger = t;
    light(same ? Math.min(index, found.length - 1) : 0);
  };

  const update = () => {
    if (!field.isConnected) return;
    const at = place();
    const t = composing || !at ? null : triggerAt(field.value, field.selectionStart);
    if (!t || !at) return close();
    const opening = !open || trigger?.kind !== t.kind;

    if (t.kind === "command") {
      const from = at.commands;
      if (!from) return close();
      const key = `${from.machine}\n${from.session}`;
      if (opening && (commands?.key !== key || Date.now() - commands.at > COMMANDS_FRESH_MS)) void readCommands(key, from.machine, from.session, update);
      const list = commands?.key === key ? commands.value : [];
      const found = filterCommands(list, t.query);
      return found.length ? draw(found, t) : close();
    }

    const { machine, root, cwd } = at;
    const key = `${machine}\n${root}`;
    if (opening && (files?.key !== key || Date.now() - files.at > FILES_FRESH_MS)) void readFiles(key, machine, root, update);
    const found = files?.key === key ? filterFiles(files.value, t.query, path => mentioned(root, cwd, path)) : [];
    return found.length ? draw(found, t) : close();
  };

  const pick = (i: number) => {
    const row = rows[i], t = trigger;
    if (!row || !t) return;
    const done = picked(field.value, field.selectionStart, t, row);
    close();
    field.value = done.text;
    field.setSelectionRange(done.caret, done.caret);
    field.focus();
  };

  field.addEventListener("input", update);
  field.addEventListener("compositionstart", () => { composing = true; });
  field.addEventListener("compositionend", () => { composing = false; update(); });
  field.addEventListener("blur", close);
  field.addEventListener("keydown", e => {
    if (!open) return;
    const act = menuKey(rows.length, index, e.key, { shift: e.shiftKey, alt: e.altKey, ctrl: e.ctrlKey, meta: e.metaKey, composing: composing || e.isComposing || e.keyCode === 229 });
    if (act.type === "none") return;
    // The window's keys (Enter sends, Esc goes back) are not for the menu's.
    e.preventDefault();
    e.stopPropagation();
    if (act.type === "move") light(act.index);
    else if (act.type === "close") close();
    else pick(index);
  });
}
