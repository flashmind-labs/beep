// The `/` and `@` menus of a session's composer and of the start fields: the
// agent's commands and skills while the message starts with `/`, the folder's
// files after an `@`. The commands are asked of the machine when a menu
// opens, the files are read once and kept
// until their folder says it changed. What opens a menu, what it lists
// and what its keys do is in mention-state.ts.
import { type Tree, isTree, mergeTree, unreadFolders, type Listing } from "../../stores/agentpane/files-state.ts";
import { paneRoot, watchRoot } from "../../stores/agentpane/worktree.ts";
import { mentioned } from "../../stores/agentpane/worktree-state.ts";
import { showAgentMenu } from "./composer-ui.tsx";
import { S } from "../../stores/bar/core.ts";
import { paneShown } from "../../ui/active.ts";
import { pushedEach } from "../../ui/snapshot-state.ts";
import { invoke } from "../../ui/tauri.ts";
import { AG, type AgentSession } from "../../stores/agents/agents-state.ts";
import { sessions } from "../../stores/agents/sessions.ts";
import {
  type AgentCommand, type FileIndex, type Row, type Trigger,
  filePaths, filterCommands, filterFiles, indexFiles, menuKey, parseRemembered, picked, triggerAt,
} from "../../stores/agents/mention-state.ts";

/// A list of commands this old is asked for again when a menu opens.
const COMMANDS_FRESH_MS = 5000;
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
/// The files read of a folder, and whether it said it changed in a way its pushed listings did not
/// cover: the next menu reads it again.
let files: (Cached<FileIndex> & { stale: boolean }) | null = null;
/// The folder listings behind `files`, kept for the updates that push them.
let walked: { key: string; listings: Map<string, Listing> } | null = null;
/// The watch of the folder `files` is of.
let watched: { key: string; stop: () => void } | null = null;
/// Where the last commands each harness reported are kept between launches.
const REMEMBERED_KEY = "beep.agentCommands";
let remembered: Map<string, AgentCommand[]> | null = null;

/// The last commands `agent` reported, on any machine.
function rememberedOf(agent: string): AgentCommand[] {
  if (!remembered) {
    try { remembered = parseRemembered(localStorage.getItem(REMEMBERED_KEY)); } catch { remembered = new Map(); }
  }
  return remembered.get(agent) ?? [];
}

function remember(agent: string, value: AgentCommand[]) {
  rememberedOf(agent);
  remembered?.set(agent, value);
  try { localStorage.setItem(REMEMBERED_KEY, JSON.stringify(Object.fromEntries(remembered ?? []))); } catch { /* kept for this launch only */ }
}
let walking = "";

/// Asks the session's machine for its commands, then `again`. A machine
/// that cannot say (an older node, a failed read) offers none.
async function readCommands(key: string, from: MenuCommands & { session: string }, again: () => void) {
  if (askedCommands === key) return;
  askedCommands = key;
  let value: AgentCommand[] = [];

  try {
    value = await invoke<AgentCommand[]>("agent_commands", { machineId: from.machine, session: from.session });
    if (value.length) remember(from.agent, value);
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
  walked = { key, listings };
  watchFiles(key, machine, root, again);
  let next = [""];

  try {
    while (next.length && walking === key) {
      const got = await Promise.all(next.map(path => invoke<Tree>("agent_tree", { machineId: machine, root, path, ignored: false, deep: true })));
      for (const tree of got) mergeTree(listings, tree);
      const paths = filePaths(listings);
      files = { key, at: Date.now(), value: indexFiles(paths), stale: false };
      again();
      if (paths.length >= WALK_FILES) break;
      next = unreadFolders(listings).slice(0, WALK_AT_ONCE);
    }
  } catch {
    // Keeps the files read so far.
  }

  if (walking === key) walking = "";
}

/// Follows the folder `files` is of: an update that pushes its listings is merged into the index,
/// any other marks it for a read at the next menu.
function watchFiles(key: string, machine: string, root: string, again: () => void) {
  if (watched?.key === key) return;
  watched?.stop();
  watched = { key, stop: watchRoot(machine, root, data => {
    if (walked?.key !== key || !files || files.key !== key) return;
    let merged = false;

    for (const { args, pushed } of pushedEach<unknown>(data, "agent_tree")) {
      if (args.machineId !== machine || args.root !== root || args.deep !== true || args.ignored !== false || !("value" in pushed) || !isTree(pushed.value)) continue;
      mergeTree(walked.listings, pushed.value);
      merged = true;
    }

    if (!merged) { files.stale = true; return; }
    files = { key, at: Date.now(), value: indexFiles(filePaths(walked.listings)), stale: false };
    again();
  }) };
}

/// Where a field's menus look: the folder on `machine` whose files are
/// offered, under `cwd`, and the running session whose commands are, on its
/// own machine (null offers no commands).
export interface MenuPlace {
  machine: string;
  root: string;
  cwd: string;
  commands: MenuCommands | null;
}

/// Whose commands a menu offers: the running session `session` of harness
/// `agent`, or, while it is null or not read yet, what `agent` last reported.
export interface MenuCommands {
  agent: string;
  machine: string;
  session: string | null;
}

/// The open session page's place, while its pane shows.
function sessionPlace(): MenuPlace | null {
  if (S.view !== "agent" || !paneShown()) return null;
  const s = AG.page?.session;
  return s && !s.external ? { machine: s.machine_id, root: paneRoot() || s.cwd, cwd: s.cwd, commands: { agent: s.agent, machine: s.machine_id, session: s.id } } : null;
}

/// The menus for an agent `agent` about to start in `cwd` on `machine`. A
/// harness reports its commands only while a session runs, and a node
/// answers only for the sessions it runs, so the commands are those of the
/// latest running Beep session of that harness: in `cwd`, else on `machine`,
/// else on any machine, else the last ones it reported. Another machine's
/// list is a best effort: the built-in commands match, its skills may not.
export function menuPlaceFor(machine: string, agent: string, cwd: string): MenuPlace {
  const latest = (rows: AgentSession[]) => rows.reduce<AgentSession | null>((best, s) => !best || s.last_activity > best.last_activity ? s : best, null);
  const running = sessions.filter(s => s.agent === agent && !s.external && s.status !== "ended" && s.status !== "closing");
  const here = running.filter(s => s.machine_id === machine);
  const session = latest(here.filter(s => s.cwd === cwd)) ?? latest(here) ?? latest(running);
  return { machine, root: cwd, cwd, commands: { agent, machine: session?.machine_id ?? machine, session: session?.id ?? null } };
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
      const { session } = from;
      const key = `${from.machine}\n${session}`;
      if (session && opening && (commands?.key !== key || Date.now() - commands.at > COMMANDS_FRESH_MS)) void readCommands(key, { ...from, session }, update);
      const read = session && commands?.key === key ? commands.value : [];
      const list = read.length ? read : rememberedOf(from.agent);
      const found = filterCommands(list, t.query);
      return found.length ? draw(found, t) : close();
    }

    const { machine, root, cwd } = at;
    const key = `${machine}\n${root}`;
    if (opening && (files?.key !== key || files.stale)) void readFiles(key, machine, root, update);
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
