// The bar's page, and the pane the command center window shows it in.
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { invoke } from "../ui/tauri.ts";
import type { PanePage } from "./pane.ts";
import type { ViewName, ViewArg, Location } from "../center/state.ts";

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
export interface PaneNow {
  view: View;
}

export function current(): PaneNow {
  return { view: S.view };
}

/// Told when the view changes, for the window's sidebar.
export const changeListeners: ((now: PaneNow) => void)[] = [];
let changeQueued = false;
export function changed() {
  if (changeQueued) return;
  changeQueued = true;
  queueMicrotask(() => {
    changeQueued = false;
    const now = current();
    refreshTopicSubscriptions();
    for (const fn of changeListeners) { try { fn(now); } catch {} }
  });
}
export const S: BarState = new Proxy<BarState>({ view: "waiting" }, {
  set(t, k, v) {
    const key = k as keyof BarState;
    const old = t[key];
    (t as unknown as Record<string, unknown>)[key] = v;
    if (key === "view" && old !== v) changed();
    return true;
  },
});

export const $ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel);
/// An element `$` is sure to find, as the view on screen draws it.
export const $$ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel)!;

/// What the command center window does for the pane. The bar has none.
export interface WindowHooks {
  /// Opens the window's Settings sheet.
  openSettings(): void;
  /// Settings' Instructions row: the sheet closes and the view shows.
  openInstructions(): void;
  /// Settings' Coding agents row: the sheet closes and the page shows.
  openProviders(): void;
  /// Settings' Report a problem row: the sheet closes and the report sheet opens.
  openReport(): void;
  /// Settings' Keyboard shortcuts row: the sheet closes and the shortcuts sheet opens.
  openShortcuts(): void;
  /// A page Settings leads to, such as Machines: the sheet closes and the pane
  /// shows it; null only closes.
  settingsAway(view: PanePage | null): void;
  /// Draws window view `name` (such as "providers") inside `root`, a page of
  /// the Settings sheet, opened with `arg`.
  embedView?(name: ViewName, root: HTMLElement, arg: ViewArg): () => void;
  /// ⌘N in the pane: the window picks where the agent starts.
  newAgent(): void;
  /// Esc from an agent view goes back to the view that opened it. A session that
  /// closed leaves at once, with `move` false, so a failed close finds the page it left for.
  goBack(from: BackTo, move: boolean): void;
  /// The project a coding session runs in, and how to open it.
  sessionDestinations?(s: { machine_id: string; cwd: string }): { name: string; folder: string }[];
  sessionProject?(s: { machine_id: string; cwd: string }): { label: string; name: string; open(): void } | null;
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

/// Set by the window once it has loaded the pane.
export let windowHooks: WindowHooks | null = null;
export function connectWindow(hooks: WindowHooks) {
  windowHooks = hooks;
}
