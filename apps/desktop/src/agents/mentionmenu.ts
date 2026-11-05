// The session composer's `/` and `@` menus: the agent's commands and skills
// while the message starts with `/`, the session's files after an `@`. The
// commands are asked of the machine when a menu opens, the files are read
// once and kept. What opens a menu, what it lists and what its keys do is
// in mention-state.ts.
import { type Tree, mergeTree, unreadFolders, type Listing } from "../agentpane/files-state.ts";
import { paneRoot } from "../agentpane/worktree.ts";
import { mentioned } from "../agentpane/worktree-state.ts";
import { el } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import { AG } from "./agents.ts";
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

/// Makes `field`, a session's message field, offer commands and files as it
/// is typed in. Call it once for each field drawn.
export function bindMentionMenu(field: HTMLTextAreaElement) {
  const parent = field.parentElement;
  if (!parent) return;
  const box = el("div", "cm-menu");
  box.setAttribute("role", "listbox");
  box.hidden = true;
  parent.append(box);
  let rows: Row[] = [];
  let trigger: Trigger | null = null;
  let index = 0;
  let composing = false;

  const close = () => {
    if (box.hidden) return;
    box.hidden = true;
    box.replaceChildren();
    rows = [];
    trigger = null;
  };

  const light = (to: number) => {
    index = to;
    [...box.children].forEach((row, i) => {
      row.classList.toggle("on", i === to);
      row.setAttribute("aria-selected", String(i === to));
      if (i === to) (row as HTMLElement).scrollIntoView({ block: "nearest" });
    });
  };

  const draw = (found: Row[], t: Trigger) => {
    const same = trigger?.kind === t.kind && trigger.start === t.start;
    rows = found;
    trigger = t;
    box.replaceChildren(...found.map((row, i) => {
      const line = el("div", "cm-row", el("span", "n", row.label), el("span", "d", row.detail), row.hint ? el("small", "", row.hint) : null);
      line.setAttribute("role", "option");
      line.dataset.i = String(i);
      return line;
    }));
    box.hidden = false;
    light(same ? Math.min(index, found.length - 1) : 0);
  };

  const update = () => {
    const s = AG.page?.session;
    const t = composing || !s || s.external ? null : triggerAt(field.value, field.selectionStart);
    if (!t || !s) return close();
    const opening = box.hidden || trigger?.kind !== t.kind;
    if (t.kind === "command") {
      const key = `${s.machine_id}\n${s.id}`;
      if (opening && (commands?.key !== key || Date.now() - commands.at > COMMANDS_FRESH_MS)) void readCommands(key, s.machine_id, s.id, update);
      const list = commands?.key === key ? commands.value : [];
      const found = filterCommands(list, t.query);
      return found.length ? draw(found, t) : close();
    }
    const root = paneRoot() || s.cwd;
    const key = `${s.machine_id}\n${root}`;
    if (opening && (files?.key !== key || Date.now() - files.at > FILES_FRESH_MS)) void readFiles(key, s.machine_id, root, update);
    const found = files?.key === key ? filterFiles(files.value, t.query, path => mentioned(root, s.cwd, path)) : [];
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
    if (box.hidden) return;
    const act = menuKey(rows.length, index, e.key, { shift: e.shiftKey, alt: e.altKey, ctrl: e.ctrlKey, meta: e.metaKey, composing: composing || e.isComposing || e.keyCode === 229 });
    if (act.type === "none") return;
    // The window's keys (Enter sends, Esc goes back) are not for the menu's.
    e.preventDefault();
    e.stopPropagation();
    if (act.type === "move") light(act.index);
    else if (act.type === "close") close();
    else pick(index);
  });
  // A press on a row keeps the focus in the field, so the field's blur does not close the menu first.
  box.addEventListener("mousedown", e => {
    e.preventDefault();
    const row = (e.target as HTMLElement).closest<HTMLElement>(".cm-row");
    if (row) pick(Number(row.dataset.i));
  });
}
