// The Cmd+K palette: one search over actions, settings, projects, pull
// requests, coding agents, chats, memories, routines and machines. The
// ranking is in the app (center_find); this draws and picks.
import { invoke } from "./tauri.ts";
import type { PaneTarget } from "./bar/pane.ts";
import type { AgentRow } from "./bar/agents.ts";

const DEBOUNCE_MS = 120;

/// The groups of hits (HitKind).
/// `project_chat` is a board project that is not imported (id: its chat),
/// `folder` a folder coding agents worked in (id: its name).
type HitKind = "action" | "setting" | "code" | "branch" | "project" | "project_chat" | "folder" | "agent_session" | "chat" | "memory" | "routine" | "machine";

/// One search result (Hit). A branch's id is its project and the branch,
/// split by a space.
interface Hit {
  kind: HitKind;
  id: string;
  title: string;
  detail: string;
  /// The one best hit, listed first under Top hit.
  top: boolean;
  /// The session an agent_session hit opens.
  session?: AgentRow;
}

/// What the window lends the palette.
export interface PaletteDeps {
  go(view: "home" | "pane", arg?: PaneTarget): void;
  /// Home, scrolled to the element with this id.
  jump(id: string): void;
  settings(section: string): void;
  /// The Coding agents page.
  providers(): void;
  project(name: string): void;
  /// The Add a project sheet, which lists the folders agents worked in.
  addProject(): void;
  /// The project's Branches list.
  branches(name: string): void;
  pane: { compose(text: string): Promise<unknown> };
  /// The search icon's markup.
  findIcon: string;
}

type Kid = Node | string | null | undefined | false;

const ICONS: Record<Exclude<HitKind, "branch" | "machine" | "project_chat" | "folder">, string> = {
  setting: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"/></svg>',
  code: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><path d="M6 8v8M18 16V9a3 3 0 00-3-3h-4"/><path d="M13 4l-2 2 2 2"/></svg>',
  project: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6a2 2 0 012-2h4l2 2h8a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>',
  agent_session: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M12 15h5"/></svg>',
  action: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
  chat: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  memory: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  routine: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M17 2l4 4-4 4"/><path d="M3 11V9a3 3 0 013-3h15M7 22l-4-4 4-4"/><path d="M21 13v2a3 3 0 01-3 3H3"/></svg>',
};

const GROUPS: Record<HitKind, string> = { action: "Actions", setting: "Settings", project: "Projects", project_chat: "Projects", folder: "Projects", agent_session: "Coding agents", code: "Projects and code", branch: "Projects and code", chat: "Chats", memory: "Memories", routine: "Routines", machine: "Machines" };
/// The icon each kind draws with.
const ICON_OF: Partial<Record<HitKind, keyof typeof ICONS>> = { branch: "code", project_chat: "project", folder: "project" };
/// Where each action goes: a pane view, or Home.
const ACTION_VIEWS: Record<string, string> = { all_chats: "chats", memories: "memories", routines: "routines", skills: "skills", apps: "apps", settings: "settings" };

let deps: PaletteDeps | null = null;
let root: HTMLDivElement | null = null, input: HTMLInputElement | null = null, list: HTMLDivElement | null = null;
let hits: Hit[] = [], sel = 0, query = "";
let sent = 0, timer: ReturnType<typeof setTimeout> | undefined, ready: Promise<unknown> | null = null, back: HTMLElement | null = null;

export function initPalette(window_: PaletteDeps) { deps = window_; }

export function paletteOpen() { return root != null; }

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, ...kids: Kid[]): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  for (const k of kids) if (k != null && k !== false) e.append(k);
  return e;
}
function svg(markup: string) {
  const s = el("span");
  s.style.display = "inline-flex";
  s.innerHTML = markup; // fixed markup from this file, no server text
  return s.firstChild;
}

/// `text` with the first match of `q` wrapped in <mark>; never markup.
function marked(text: string, q: string): Kid[] {
  const at = q ? text.toLowerCase().indexOf(q.toLowerCase()) : -1;
  if (at < 0) return [text];
  return [text.slice(0, at), el("mark", "", text.slice(at, at + q.length)), text.slice(at + q.length)];
}

export function openPalette() {
  if (!deps) return;
  if (root) return input!.focus();
  back = document.activeElement as HTMLElement | null;
  query = "";
  hits = [];
  sel = 0;
  input = el("input");
  input.type = "text";
  input.placeholder = "Search anything: chats, projects, coding agents, machines, settings, PRs…";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", "Search");
  input.setAttribute("role", "combobox");
  input.setAttribute("aria-controls", "pal-list");
  input.setAttribute("aria-expanded", "true");
  input.addEventListener("input", onType);
  list = el("div", "res");
  list.setAttribute("role", "listbox");
  list.id = "pal-list";
  const foot = el("div", "foot");
  for (const [k, t] of [["↑↓", "move"], ["↵", "open"], ["⌘↵", "ask Beep"], ["esc", "close"]]) foot.append(el("span", "", el("kbd", "", k), t));
  const box = el("div", "cc-pal", el("div", "q", svg(deps.findIcon), input), list, foot);
  box.setAttribute("role", "dialog");
  box.setAttribute("aria-label", "Search");
  const scrim = root = el("div", "cc-scrim", box);
  scrim.addEventListener("mousedown", e => { if (e.target === scrim) { e.preventDefault(); close(); } });
  document.querySelector(".cwin")!.append(scrim);
  input.focus();
  // The lists are fetched once, before the first search.
  const opened = ready = invoke("center_palette").catch(e => console.warn("loading the palette:", e));
  search("");
  // Branches and agent sessions are read on the machines, so they come in later.
  const later = (command: string, what: string) => opened.then(() => invoke(command))
    .then(() => { if (ready === opened && query.trim()) search(query); })
    .catch(e => console.warn(`${what} for the palette:`, e));
  later("center_palette_branches", "reading branches");
  later("center_palette_sessions", "listing coding agents");
}

function close() {
  if (!root) return;
  clearTimeout(timer);
  sent++;
  root.remove();
  root = input = list = ready = null;
  const to = back;
  back = null;
  // The sidebar is redrawn now and then, so its Search row may be a new element.
  (to?.isConnected ? to : document.querySelector<HTMLElement>(".cc-search"))?.focus();
}

function onType() {
  query = input!.value;
  // A reply to an earlier query must not draw while this one waits.
  sent++;
  clearTimeout(timer);
  timer = setTimeout(() => search(query), DEBOUNCE_MS);
}

async function search(q: string) {
  const mine = ++sent;
  let found;
  try {
    await ready;
    found = await invoke<Hit[]>("center_find", { query: q });
  } catch (e) {
    console.warn("searching:", e);
    return;
  }
  // A newer query was sent meanwhile, or the palette closed.
  if (mine !== sent || !root) return;
  hits = found;
  sel = 0;
  draw(q.trim());
}

function draw(q: string) {
  const rows: HTMLElement[] = [];
  let group: string | null = null;
  hits.forEach((hit, i) => {
    const next = hit.top ? "top" : GROUPS[hit.kind];
    if (next !== group) {
      group = next;
      rows.push(el("h5", "", hit.top ? "Top hit" : hit.kind === "chat" && !q ? "Recent chats" : group));
    }
    rows.push(row(hit, i, q));
  });
  if (!hits.length) rows.push(noHits(q));
  list!.replaceChildren(...rows);
  mark();
}

function row(hit: Hit, i: number, q: string) {
  const note = hit.detail && ["chat", "routine", "setting", "project", "project_chat", "folder", "agent_session", "code", "branch"].includes(hit.kind) ? el("small", "", hit.detail) : null;
  const side = hit.detail && (hit.kind === "memory" || hit.kind === "machine") ? el("span", "w", hit.detail) : null;
  const lead = hit.kind === "machine" ? el("span", `ddot${hit.detail === "online" ? "" : " off"}`) : svg(ICONS[ICON_OF[hit.kind] || (hit.kind as keyof typeof ICONS)]);
  const b = el("button", "it", lead, el("span", "t", el("span", "", ...marked(hit.title, q)), note), side);
  b.type = "button";
  b.tabIndex = -1;
  b.setAttribute("role", "option");
  b.id = `pal-opt-${i}`;
  b.addEventListener("mousemove", () => { if (sel !== i) { sel = i; mark(); } });
  b.addEventListener("click", () => pick(hit));
  return b;
}

function noHits(q: string) {
  const none = el("div", "none", `Nothing matches “${q}”.`);
  if (q) {
    const ask = el("button", "dbtn", `Ask Beep “${q}”`, el("kbd", "", "⌘↵"));
    ask.type = "button";
    ask.addEventListener("click", () => ask_(q));
    none.append(el("br"), ask);
  }
  return none;
}

/// Highlights the selected row and keeps it in view.
function mark() {
  const items = list!.querySelectorAll(".it");
  items.forEach((b, i) => b.setAttribute("aria-selected", String(i === sel)));
  if (items[sel]) input!.setAttribute("aria-activedescendant", items[sel].id);
  else input!.removeAttribute("aria-activedescendant");
  items[sel]?.scrollIntoView({ block: "nearest" });
}

function ask_(q: string) {
  const text = q.trim();
  if (!text) return;
  close();
  const { go, pane } = deps!;
  go("pane", { chat: { session: null } });
  pane.compose(text).catch(e => console.warn("asking from the palette:", e));
}

function pick(hit: Hit) {
  close();
  const { go, jump, settings, project, branches, addProject } = deps!;
  switch (hit.kind) {
    case "action":
      if (hit.id === "new_chat") return go("pane", { chat: { session: null } });
      if (hit.id === "home") return go("home");
      if (hit.id === "coding_agents") return deps!.providers();
      if (ACTION_VIEWS[hit.id]) return go("pane", ACTION_VIEWS[hit.id]);
      return;
    case "chat": return go("pane", { chat: { session: hit.id } });
    case "memory": return go("pane", "memories");
    case "routine": return go("pane", "routines");
    case "machine": return jump(`m-${hit.id}`);
    case "setting": return settings(hit.id);
    case "project": return project(hit.id);
    // On the board but not imported: its project chat.
    case "project_chat": return go("pane", { chat: { session: hit.id } });
    // Only coding agents worked there: the sheet that imports it.
    case "folder": return addProject();
    case "agent_session": return hit.session ? go("pane", { agent: hit.session }) : undefined;
    // The project's Branches list, where its Review button is.
    case "branch": return branches(hit.id.slice(0, hit.id.indexOf(" ")));
    case "code": return invoke("open_url", { url: hit.id }).catch(e => console.warn("opening a pull request:", e));
  }
}

/// Keys while the palette is open. Runs before the window's other handlers.
window.addEventListener("keydown", e => {
  if (!root) return;
  const stop = () => { e.preventDefault(); e.stopImmediatePropagation(); };
  if (e.key === "Escape") { stop(); return close(); }
  if (e.metaKey && e.key.toLowerCase() === "k") { stop(); return close(); }
  if (e.metaKey && e.key === "Enter") { stop(); return ask_(input!.value); }
  if (e.isComposing) return;
  if (e.key === "Enter") { stop(); if (hits[sel]) pick(hits[sel]); return; }
  if (e.key === "Tab") { stop(); return input!.focus(); }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    stop();
    if (!hits.length) return;
    sel = (sel + (e.key === "ArrowDown" ? 1 : hits.length - 1)) % hits.length;
    mark();
    return;
  }
  // Other window shortcuts do nothing under the overlay.
  if (e.metaKey && !["a", "c", "v", "x", "z"].includes(e.key.toLowerCase())) stop();
}, true);
