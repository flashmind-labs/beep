import type { ProjectWindowProps } from "../project/projectwindow.ts";
// The bar's page, and the pane the command center window shows it in.
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { invoke } from "../ui/tauri.ts";
import type { ViewName, ViewArg, Location } from "../center/state.ts";
import type { AgentRow } from "../agents/agents-state.ts";

export const bar = document.getElementById("bar")!;
/// True when this page runs as a pane in the command center window rather
/// than as the bar. A few bar-only spots check it.
export const PANE = bar.dataset.mode === "window";

/// What the bar or the pane shows.
export type View = "settings" | "machines" | "agent" | "agentnew" | "agents" | "waiting";

export interface BarState {
  view: View;
}

/// What the pane shows, for the window's sidebar.
export type PaneNow = BarState;

export function current(): PaneNow {
  return { view: S.view };
}

/// Told when the view changes, for the window's sidebar.
export const changeListeners: ((now: PaneNow) => void)[] = [];
let changeQueued = false;
function changed() {
  if (changeQueued) return;
  changeQueued = true;
  queueMicrotask(() => {
    changeQueued = false;
    const now = current();
    refreshTopicSubscriptions();
    for (const fn of changeListeners) { try { fn(now); } catch {} }
  });
}
const route: BarState = { view: "waiting" };
/// What the bar or the pane shows now; `setView` is the one place that changes it.
export const S: Readonly<BarState> = route;

export function setView(view: View) {
  if (route.view === view) return;
  route.view = view;
  // The pane's layout keys on its view: marked at once, before the view draws and measures.
  if (PANE) bar.dataset.view = view;
  changed();
}
if (PANE) bar.dataset.view = S.view;

export const $ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel);

/// What the command center window does for the pane. The bar has none.
export interface WindowHooks {
  /// Opens the window's Settings sheet.
  openSettings(): void;
  /// Settings' Report a problem row: the sheet closes and the report sheet opens.
  openReport(): void;
  /// Settings' Keyboard shortcuts row: the sheet closes and the shortcuts sheet opens.
  openShortcuts(): void;
  /// Draws window view `name` (such as "providers") inside `root`, a page of
  /// the Settings sheet, opened with `arg`.
  embedView?(name: ViewName, root: HTMLElement, arg: ViewArg): () => void;
  /// ⌘N in the pane: the window picks where the agent starts.
  newAgent(): void;
  /// Esc from an agent view goes back to the view that opened it. A session that
  /// closed leaves at once, with `move` false, so a failed close finds the page it left for.
  goBack(from: BackTo, move: boolean): void;
  /// Esc in an agent view, before `AG.back`: steps out of a chat's threads rail or leaves the
  /// Assistant page for Home. False when `AG.back` should handle it.
  stepOut?(): boolean;
  /// The session a start became takes over the page that shows the start, and its navigation
  /// entry. Nothing happens while the window shows another page.
  openStarted(session: AgentRow): void;
  /// The project a coding session runs in, and how to open it.
  sessionDestinations?(s: { machine_id: string; cwd: string }): { name: string; folder: string }[];
  projectWindowProps?(project: string): ProjectWindowProps;
  sessionProject?(s: { machine_id: string; cwd: string }): { label: string; name: string; open(): void; afterClose(s: { machine_id: string; id: string; started_at?: number }): void } | null;
}

/// A window view to go back to, as the window opened an agent view from it.
export type BackTo = Location;
/// Where Esc from an agent view goes: Every session, the bar's waiting list, a window view,
/// or with "hide" nowhere: the bar hides and the window goes Home.
export type AgentReturn = "agents" | "waiting" | "hide" | BackTo;

/// Hands a view to the command center window, which hides the bar.
export function toWindow(target: unknown) {
  return invoke("center_open", { target }).catch(() => {});
}

/// A new chat starts in a project: the window's own pick, else Projects.
export function newChat() {
  if (windowHooks) return windowHooks.newAgent();
  void toWindow({ projects: { project: null } });
}

/// Set by the window once it has loaded the pane.
export let windowHooks: WindowHooks | null = null;
export function connectWindow(hooks: WindowHooks) {
  windowHooks = hooks;
}
