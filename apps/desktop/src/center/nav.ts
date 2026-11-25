// Where the window stands and how it moves: the page showing, its history, and `go`.
// views.tsx draws the pages and installs what `go` and `drawTop` run, so every
// view can send the window somewhere without importing the router.
import { flushSync } from "react-dom";
import { invoke } from "../ui/tauri.ts";
import type { CenterTarget } from "../project/projectwindow.ts";
import { createStore } from "zustand";
import type { Alone } from "../project/projectwindow.ts";
import type { Location, NavigationArgs, ViewName } from "./state.ts";
import { windowHost } from "./window-shell.tsx";
import { windowHistory } from "./window-router.ts";

/// The pages after Home and the pane, in the order their boxes follow them in `#main`.
export const PAGES = ["projects", "instructions", "providers", "terminals"] as const;
export type Page = typeof PAGES[number];

/// What the router, views.tsx, runs for `goTo`, `drawTop` and the projects page's current argument.
export const router = {
  goTo: (_page: Location): boolean => false,
  drawTop: (): void => {},
  projectArg: (): Extract<Location, { view: "projects" }>["arg"] | undefined => undefined,
};

/// The page showing, and how the window is moving to the next.
export const nav: {
  page: Location | null;
  /// Set while ⌘[ or ⌘] moves, so the move is not a new visit.
  moving: boolean;
  /// Set while a Back moves, so the page comes in from the left on the phone.
  goingBack: boolean;
} = { page: null, moving: false, goingBack: false };

export const pages = windowHistory;
/// Runs `step` as a Back: the phone's Back goes up a level without a history step.
export function asBack(step: () => void) {
  nav.goingBack = true;

  try {
    step();
  } finally {
    nav.goingBack = false;
  }
}

export const boxes = new Map<Page, HTMLElement>();
export const keepBox = Object.fromEntries(PAGES.map(name => [name, (el: HTMLDivElement | null) => { if (el) boxes.set(name, el); else boxes.delete(name); }]));
/// How often each page drew; 0 leaves its box empty.
export const shown = createStore<{ drawn: Record<Page, number> }>(() => ({
  drawn: { projects: 0, instructions: 0, providers: 0, terminals: 0 },
}));

/// Draws page `name` in its box again, or empties the box.
export function drawPage(name: Page, on = true) {
  flushSync(() => shown.setState(s => ({ drawn: { ...s.drawn, [name]: on ? s.drawn[name] + 1 : 0 } })));
}

/// The element view `name` draws in; Home and the pane have theirs in the page.
export function boxOf(name: ViewName): HTMLElement | null {
  switch (name) {
    case "home": return windowHost("home-box");
    case "pane": return windowHost("pane-box");
    default: return boxes.get(name) ?? null;
  }
}

/// The project or chat a window shows alone; null in the main window.
export let windowAlone: Alone | null = null;


export function setWindowAlone(alone: Alone) {
  windowAlone = alone;
}

/// The project a project's window shows; null in any other.
export function aloneProject() {
  return windowAlone?.kind === "project" ? windowAlone.project : null;
}



/// The page showing, as `go` opens it again.
export function currentLocation(): Location | null {
  if (!nav.page) return null;

  // Project sections change without mounting the page again.
  if (nav.page.view === "projects") {
    const arg = router.projectArg();
    if (arg && typeof arg === "object" && "project" in arg) return { view: "projects", arg };
  }

  return nav.page;
}

/// Whether there is a page to go back or forward to.
export function hasHistory(forward: boolean) {
  return pages.has(forward);
}

/// Goes one page back (⌘[) or forward (⌘]), if there is one; false when there is none.
export function goHistory(forward: boolean): boolean {
  const current = currentLocation();
  if (!current) return false;
  const page = forward ? pages.forward(current) : pages.back(current);
  if (!page) return false;
  nav.moving = true;

  try {
    if (forward) goTo(page); else asBack(() => goTo(page));
  } finally {
    nav.moving = false;
  }

  return true;
}

export function goTo(target: Location): boolean {
  return router.goTo(target);
}

/// Draws the head again: the title, tools and date of the page showing.
export function drawTop() {
  router.drawTop();
}

/// Shows view `name`, passing `arg` to its mount.
export function go(...[name, arg]: NavigationArgs): boolean {
  switch (name) {
    case "home": return goTo({ view: name, arg: null });
    case "terminals": return goTo({ view: name, arg: null });
    case "pane": return goTo({ view: name, arg: arg ?? null });
    case "projects": return goTo({ view: name, arg: arg ?? null });
    case "providers": return goTo({ view: name, arg: arg ?? null });
    case "instructions": return goTo({ view: name, arg: arg ?? null });
  }
}

/// Shows `target` in the main window, bringing it up.
export function toMain(target: CenterTarget) {
  invoke("center_open", { target }).catch(e => console.warn("opening in the main window:", e));
}
