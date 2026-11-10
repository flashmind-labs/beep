import { createElement, createRef, type MouseEvent as ReactMouseEvent } from "react";
import { placeWindowParts } from "./window-root.ts";
import { PaletteView } from "./palette-view.tsx";
import type { NavigationArgs } from "./state.ts";
import { commandKey } from "../ui/platform.ts";
// The Cmd+K palette: one search over actions, settings, projects, pull
// requests, coding agents, chats, memories, routines and machines. The
// window holds the index, so a keystroke waits on nothing.
import { invoke, listen } from "../ui/tauri.ts";
import type { PaneTarget } from "../bar/pane.ts";
import type { AgentRow } from "../agents/agents.ts";
import { rank, type Entry, type Hit as FoundHit, type HitKind, type PaletteIndex } from "./palette-rank.ts";
import { PaletteSearch } from "./palette-search.ts";
import { projectFirst } from "../project/projectwindow.ts";
import { state } from "./state.ts";
import { IC } from "../ui/icons.tsx";

/// One search result. A branch's id is its project and the branch, split by
/// a space.
export type Hit = FoundHit<AgentRow>;
type Index = PaletteIndex<AgentRow>;

type PaletteNavigation = Extract<NavigationArgs, [view: "home" | "pane", ...rest: unknown[]]>;

/// What the window lends the palette.
export interface PaletteDeps {
  go(...args: PaletteNavigation): unknown;
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
  /// A new chat that starts with this text (⌘↵).
  ask(text: string): void;
  /// The project whose hits list first: a project window's own.
  scope(): string | null;
  /// Opens a chat hit: a research in its window, any other chat in the pane.
  openChat(id: string): void;
  /// Whether a chat is a research, which lists under Research, not Chats.
  research(id: string): boolean;
}

/// The icons the window's IC has none of.
const ICONS = {
  code: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><path d="M6 8v8M18 16V9a3 3 0 00-3-3h-4"/><path d="M13 4l-2 2 2 2"/></svg>',
  project: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6a2 2 0 012-2h4l2 2h8a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>',
  agent_session: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M12 15h5"/></svg>',
};

const GROUPS: Record<HitKind, string> = { action: "Actions", setting: "Settings", project: "Projects", project_chat: "Projects", folder: "Projects", agent_session: "Coding agents", code: "Projects and code", branch: "Projects and code", chat: "Chats", memory: "Memories", routine: "Routines", machine: "Machines" };
/// The icon each kind draws with, from ICONS or IC; one not here has its own name.
const ICON_OF: Partial<Record<HitKind | "research", keyof typeof ICONS | keyof typeof IC>> = { branch: "code", project_chat: "project", folder: "project", setting: "gear", action: "plus", chat: "chat", memory: "mem", research: "doc", routine: "loop" };
/// Where each action goes: a pane view, or Home.
const ACTION_VIEWS: Record<string, PaneTarget> = { all_chats: "chats", memories: "memories", routines: "routines", skills: "skills", apps: "apps", settings: "settings" };

let deps: PaletteDeps | null = null;
/// Whether the palette is open, and its search field once drawn.
let open = false;
const field = createRef<HTMLInputElement>();
let hits: Hit[] = [], sel = 0, drawnQuery = "";
let searching: PaletteSearch<Entry<AgentRow>> | null = null, back: HTMLElement | null = null;
/// What the palette finds, as of the app's last word. Empty until the first
/// read answers.
let index: Index = { actions: [], settings: [], code: [], projects: [], sessions: [], chats: [], memories: [], routines: [], machines: [] };
/// The app has a newer index than this window read.
let behind = false;

function readIndex() {
  behind = false;
  invoke<Index>("center_palette_index")
    .then(read => { index = read; searching?.refresh(); })
    .catch(e => console.warn("reading the palette's index:", e));
}

export function initPalette(window_: PaletteDeps) {
  deps = window_;
  readIndex();
  // A window with the palette shut reads it the next time it opens.
  listen("palette-changed", () => { if (open) readIndex(); else behind = true; });
  listen("palette-text-changed", () => searching?.requery());
}

export function openPalette() {
  if (!deps) return;
  if (open) return field.current?.focus();
  open = true;
  back = document.activeElement as HTMLElement | null;
  hits = [];
  sel = 0;
  drawnQuery = "";
  searching = new PaletteSearch<Entry<AgentRow>>(
    [
      query => invoke<Entry<AgentRow>[]>("center_find_prompts", { query }),
      query => invoke<Entry<AgentRow>[]>("center_find_text", { query }),
    ],
    (q, text) => {
      // A redraw of the same query keeps the row the user is on.
      const on = q === drawnQuery ? hits[sel] : undefined;
      hits = projectFirst(rank(q, index, text), deps?.scope());
      sel = on ? Math.max(0, hits.findIndex(h => h.kind === on.kind && h.id === on.id)) : 0;
      drawnQuery = q;
      draw(q.trim());
    },
  );
  searching.type("");
  // Hits come from what this window holds; anything newer draws when it comes.
  if (behind) readIndex();
  invoke("center_palette_refresh").catch(e => console.warn("refreshing the palette:", e));
}

function close() {
  if (!open) return;
  searching?.stop();
  searching = null;
  placeWindowParts({ palette: null });
  open = false;
  const to = back;
  back = null;
  // The sidebar is redrawn now and then, so its Search row may be a new element.
  (to?.isConnected ? to : document.querySelector<HTMLElement>(".cc-search"))?.focus();
}

function draw(q: string) {
  if (!open) return;
  placeWindowParts({ palette: createElement("div", { className: "cc-scrim",
    onMouseDown: (e: ReactMouseEvent) => { if (e.target === e.currentTarget) { e.preventDefault(); close(); } } }, createElement(PaletteView, {
    input: field, hits, selected: sel, query: q, subscribed: state.subscribed, searchIcon: IC.find,
    group: (hit: Hit, query: string) => hit.top ? "Top hit" : hit.kind === "chat" && !query ? "Recent chats" : GROUPS[hit.kind],
    icon: (hit: Hit) => {
      const name = ICON_OF[hit.kind === "chat" && deps?.research(hit.id) ? "research" : hit.kind] ?? hit.kind;
      return name in ICONS ? ICONS[name as keyof typeof ICONS] : IC[name as keyof typeof IC];
    },
    search: (value: string) => searching?.type(value), choose: pick,
    hover: (index: number) => { if (sel !== index) { sel = index; mark(); } }, ask: ask_,
  })) });
}

/// Highlights the selected row and keeps it in view.
function mark() { draw(drawnQuery.trim()); }

function ask_(q: string) {
  const text = q.trim();
  if (!text) return;
  close();
  deps!.ask(text);
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
    case "chat": return deps!.openChat(hit.id);
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
  if (!open) return;
  const stop = () => { e.preventDefault(); e.stopImmediatePropagation(); };
  if (e.key === "Escape") { stop(); return close(); }
  if (commandKey(e) && e.key.toLowerCase() === "k") { stop(); return close(); }
  if (commandKey(e) && e.key === "Enter") { stop(); return state.subscribed && field.current ? ask_(field.current.value) : undefined; }
  if (e.isComposing) return;
  if (e.key === "Enter") { stop(); if (hits[sel]) pick(hits[sel]); return; }
  if (e.key === "Tab") { stop(); return field.current?.focus(); }

  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    stop();
    if (!hits.length) return;
    sel = (sel + (e.key === "ArrowDown" ? 1 : hits.length - 1)) % hits.length;
    mark();
    return;
  }

  // Other window shortcuts do nothing under the overlay.
  if (commandKey(e) && !["a", "c", "v", "x", "z"].includes(e.key.toLowerCase())) stop();
}, true);
