import { createRef, type MouseEvent as ReactMouseEvent } from "react";
import { placeWindowParts } from "../../parts/center/window-root.tsx";
import { PaletteView } from "./palette-view.tsx";
import type { NavigationArgs } from "../../stores/center/state.ts";
import { commandKey } from "../../ui/platform.ts";
// The Cmd+K palette: one search over actions, settings, projects, pull
// requests, coding agents and machines. The window holds the index, so a
// keystroke waits on nothing. The right arrow on a project lists its
// sessions in place of the hits.
import { invoke, listen } from "../../ui/tauri.ts";
import type { AgentRow } from "../../stores/agents/agents-state.ts";
import {
  projectPaletteIndex,
  projectPalettePlaceholder,
  projectPromptEntries,
  rank,
  type Entry,
  type Hit as FoundHit,
  type HitKind,
  type PaletteIndex,
  type ProjectPaletteScope,
} from "./palette-rank.ts";
import { PaletteSearch } from "./palette-search.ts";
import { projectWindowProps } from "../../parts/project/projectops.ts";
import { opensNewWindow } from "../../stores/project/projectwindow.ts";
import { HarnessIcon, IC, Icon, type IconName } from "../../ui/icons.tsx";

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
  projects?(): void;
  terminals?(): void;
  newSession?(): void;
  newTerminal?(): void;
  /// The Add a project sheet, which lists the folders agents worked in.
  addProject(): void;
  joinTeam?(): void;
  addMachine?(): void;
  shortcuts?(): void;
  report?(): void;
  /// The project's Branches list.
  branches(name: string): void;
  /// Opens a tile or thread of `project` in this window.
  openTile?(project: string, tile: string | null): void;
  openTerminal?(project: string, id: string): void;
  projectSkills?(project: string): void;
  projectHarnesses?(project: string): void;
  /// The detached project window's scope, when this window is one; null in the main window.
  scope(): ProjectPaletteScope<AgentRow> | null;
  /// Starts reading `project`'s lists when the palette opens on it.
  prepareScope?(project: string): void;
  /// Calls `refresh` when a project list read for `scope` changes.
  followScope?(refresh: () => void): void;
}

/// The icons the window's IC has none of.
const ICONS = {
  code: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><path d="M6 8v8M18 16V9a3 3 0 00-3-3h-4"/><path d="M13 4l-2 2 2 2"/></svg>',
  project: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6a2 2 0 012-2h4l2 2h8a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>',
  agent_session: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M12 15h5"/></svg>',
};

const GROUPS: Record<HitKind, string> = {
  action: "Actions",
  setting: "Settings",
  project: "Projects",
  folder: "Projects",
  agent_session: "Coding agents",
  pr: "Pull requests",
  issue: "Issues",
  code: "Projects and code",
  branch: "Projects and code",
  terminal: "Terminals",
  machine: "Machines",
};
/// The icon each kind draws with, from ICONS or IC; one not here has its own name.
const ICON_OF: Partial<Record<HitKind, keyof typeof ICONS | keyof typeof IC>> = { branch: "code", folder: "project", setting: "gear", action: "plus", terminal: "term" };

const ACTION_ICONS: Record<string, keyof typeof ICONS | keyof typeof IC> = {
  home: "home",
  projects: "project",
  terminals: "term",
  new_terminal: "term",
  coding_agents: "agent_session",
  project_sessions: "agent_session",
  project_overview: "project",
  settings: "gear",
};

const REACT_ACTION_ICONS: Record<string, IconName> = {
  project_prs: "pr",
  project_issues: "issue",
  project_branches: "branch",
  project_actions: "run",
  project_machines: "laptop",
  project_team: "group",
  project_skills: "skill",
  project_harnesses: "sliders",
};

const PROJECT_ACTION_TILES: Record<string, string | null> = {
  project_overview: null,
  project_prs: "prs",
  project_issues: "issues",
  project_branches: "branches",
  project_actions: "actions",
  project_sessions: "agents",
  project_machines: "machines",
  project_team: "team",
};

function hitReactIcon(hit: Hit): IconName | null {
  if (hit.kind === "pr") return "pr";
  if (hit.kind === "issue") return "issue";
  if (hit.kind === "action" && hit.id in REACT_ACTION_ICONS) return REACT_ACTION_ICONS[hit.id];
  return null;
}

function hitIcon(hit: Hit): keyof typeof ICONS | keyof typeof IC {
  if (hit.kind === "action" && hit.id in ACTION_ICONS) return ACTION_ICONS[hit.id];
  return ICON_OF[hit.kind] ?? (hit.kind as keyof typeof ICONS | keyof typeof IC);
}

let deps: PaletteDeps | null = null;
/// Whether the palette is open, and its search field once drawn.
let open = false;
const field = createRef<HTMLInputElement>();
let hits: Hit[] = [], sel = 0, drawnQuery = "";
/// The project whose sessions are listed in place of the search's hits.
let inside: Hit | null = null;
let searching: PaletteSearch<Entry<AgentRow>> | null = null, back: HTMLElement | null = null;
/// What the palette finds, as of the app's last word. Empty until the first
/// read answers.
let index: Index = { actions: [], settings: [], code: [], projects: [], sessions: [], machines: [] };

function readIndex() {
  invoke<Index>("center_palette_index")
    .then(read => { index = read; searching?.refresh(); })
    .catch(e => console.warn("reading the palette's index:", e));
}

export function initPalette(window_: PaletteDeps) {
  deps = window_;
  readIndex();
  // The app sends each new index as JSON text; this window takes it as it comes.
  listen<string>("palette-changed", ({ payload }) => {
    try { index = JSON.parse(payload) as Index; } catch (e) { console.warn("the palette's index:", e); return; }
    searching?.refresh();
  });
  listen("palette-text-changed", () => searching?.requery());
  window_.followScope?.(() => { if (open) searching?.refresh(); });
}

export function openPalette() {
  if (!deps) return;
  if (open) return field.current?.focus();
  open = true;
  back = document.activeElement as HTMLElement | null;
  hits = [];
  sel = 0;
  drawnQuery = "";
  inside = null;
  const initialScope = deps.scope();
  if (initialScope) deps.prepareScope?.(initialScope.project);
  searching = new PaletteSearch<Entry<AgentRow>>(
    query => {
      const scoped = deps?.scope();
      return invoke<Entry<AgentRow>[]>(
        "center_find_prompts",
        scoped ? { query, project: scoped.project } : { query },
      );
    },
    (q, text) => {
      // A redraw of the same query keeps the row the user is on.
      const on = q === drawnQuery ? hits[sel] : undefined;
      const scoped = deps?.scope() ?? null;
      const activeIndex = scoped ? projectPaletteIndex(index, scoped) : index;
      const activeText = scoped ? projectPromptEntries(text, scoped) : text;
      const sessions = !scoped && inside ? sessionsOf(inside) : [];
      if (!sessions.length) inside = null;
      hits = inside ? sessions : rank(q, activeIndex, activeText);
      sel = on ? Math.max(0, hits.findIndex(h => h.kind === on.kind && h.id === on.id)) : 0;
      drawnQuery = q;
      draw(q.trim());
    },
  );
  searching.type("");
  // Hits come from what this window holds; anything newer draws when it comes.
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

/// Most sessions a project lists.
const SESSIONS_SHOWN = 10;

/// The sessions of a project or folder hit, the one that moved last first.
function sessionsOf(hit: Hit): Hit[] {
  if (hit.kind !== "project" && hit.kind !== "folder") return [];
  return index.sessions.filter(session => session.project === hit.id).slice(0, SESSIONS_SHOWN);
}

/// Back to the search's hits, on the project whose sessions were listed.
function leave() {
  if (!inside) return;
  // The redraw keeps the row it finds selected.
  hits = [inside];
  sel = 0;
  inside = null;
  searching?.refresh();
}

function draw(q: string) {
  if (!open) return;
  const scoped = deps?.scope() ?? null;
  const listing = inside?.title ?? null, more = !scoped && !inside && hits[sel] && sessionsOf(hits[sel]).length > 0;
  placeWindowParts({ palette: <div className="cc-scrim" onMouseDown={(e: ReactMouseEvent) => { if (e.target === e.currentTarget) { e.preventDefault(); close(); } }}>
    <PaletteView
      input={field} hits={hits} selected={sel} query={q} searchIcon={IC.find}
      placeholder={scoped ? projectPalettePlaceholder(scoped.label) : undefined}
      group={(hit: Hit) => listing ? `Sessions · ${listing}` : hit.top ? "Top hit" : scoped && hit.kind === "branch" ? "Branches" : GROUPS[hit.kind]}
      keys={[["↑↓", "move"], ["↵", "open"], ...(listing ? [["←", "back"]] : more ? [["→", "sessions"]] : []), ["esc", "close"]]}
      icon={(hit: Hit) => {
        if (hit.session) return <HarnessIcon agent={hit.session.agent} className="dmag-ic" />;
        const reactIcon = hitReactIcon(hit);
        if (reactIcon) return <Icon name={reactIcon} />;
        const name = hitIcon(hit);
        return <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: name in ICONS ? ICONS[name as keyof typeof ICONS] : IC[name as keyof typeof IC] }} />;
      }}
      windowProps={hit => {
        if (hit.kind !== "project") return null;
        const props = projectWindowProps(hit.id);
        const open = (event: ReactMouseEvent) => {
          props.clicks.onClickCapture(event);
          if (opensNewWindow(event)) close();
        };
        return { ...props, clicks: { ...props.clicks, onClickCapture: open, onAuxClick: open } };
      }}
      search={(value: string) => { inside = null; searching?.type(value); }} choose={pick}
      hover={(index: number) => { if (sel !== index) { sel = index; mark(); } }}
    />
  </div> });
}

/// Highlights the selected row and keeps it in view.
function mark() { draw(drawnQuery.trim()); }

function pick(hit: Hit) {
  const scopedProject = deps?.scope()?.project ?? null;
  close();
  const { go, jump, settings, project, branches, addProject } = deps!;

  switch (hit.kind) {
    case "action":
      if (hit.id === "new_session") return deps!.newSession?.();
      if (hit.id === "new_terminal") return deps!.newTerminal?.();
      if (scopedProject && hit.id in PROJECT_ACTION_TILES) return deps!.openTile?.(scopedProject, PROJECT_ACTION_TILES[hit.id]);
      if (scopedProject && hit.id === "project_skills") return deps!.projectSkills?.(scopedProject);
      if (scopedProject && hit.id === "project_harnesses") return deps!.projectHarnesses?.(scopedProject);
      if (hit.id === "home") return go("home");
      if (hit.id === "projects") return deps!.projects ? deps!.projects() : project("");
      if (hit.id === "terminals") return deps!.terminals?.();
      if (hit.id === "add_project") return addProject();
      if (hit.id === "join_team") return deps!.joinTeam?.();
      if (hit.id === "add_machine") return deps!.addMachine?.();
      if (hit.id === "coding_agents") return deps!.providers();
      if (hit.id === "settings") return go("pane", "settings");
      if (hit.id === "shortcuts") return deps!.shortcuts?.();
      if (hit.id === "report_problem") return deps!.report?.();
      return;
    case "machine": return jump(`m-${hit.id}`);
    case "setting": return settings(hit.id);
    case "project": return project(hit.id);
    // Only coding agents worked there: the sheet that imports it.
    case "folder": return addProject();
    case "agent_session": return hit.session ? go("pane", { agent: hit.session }) : undefined;
    case "pr": return scopedProject ? deps!.openTile?.(scopedProject, `pr:${hit.id}`) : undefined;
    case "issue": return scopedProject ? deps!.openTile?.(scopedProject, `issue:${hit.id}`) : undefined;
    case "terminal": return scopedProject ? deps!.openTerminal?.(scopedProject, hit.id) : undefined;
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

  if (e.key === "ArrowRight" && !inside) {
    const hit = hits[sel], input = field.current;
    // Only from the end of what was typed, where the key moves no caret.
    if (!hit || !input || input.selectionStart !== input.value.length || !sessionsOf(hit).length) return;
    stop();
    inside = hit;
    searching?.refresh();
    return;
  }

  if (e.key === "ArrowLeft" && inside) { stop(); return leave(); }

  // Other window shortcuts do nothing under the overlay.
  if (commandKey(e) && !["a", "c", "v", "x", "z"].includes(e.key.toLowerCase())) stop();
}, true);
