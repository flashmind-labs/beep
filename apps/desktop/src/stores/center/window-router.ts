import { createHistory, createRootRoute, createRoute, createRouter, type HistoryLocation } from "@tanstack/react-router";
import type { Location, ViewName } from "./state.ts";

declare module "@tanstack/history" {
  interface HistoryState { page?: Location | null }
}

const MAX_PAGES = 100;
const paths = { home: "/", pane: "/pane", projects: "/projects", providers: "/providers", instructions: "/instructions", terminals: "/terminals" } satisfies Record<ViewName, string>;
const root = createRootRoute();
const routeTree = root.addChildren([
  createRoute({ getParentRoute: () => root, path: paths.home }),
  createRoute({ getParentRoute: () => root, path: paths.pane }),
  createRoute({ getParentRoute: () => root, path: paths.projects }),
  createRoute({ getParentRoute: () => root, path: paths.providers }),
  createRoute({ getParentRoute: () => root, path: paths.instructions }),
  createRoute({ getParentRoute: () => root, path: paths.terminals }),
]);

type ParsedHistoryState = HistoryLocation["state"];
interface Entry { path: string; state: ParsedHistoryState }

/// Router history owns navigation state; paths name screens and typed state keeps native targets private.
export class WindowHistory {
  private entries: Entry[] = [{ path: "/", state: { __TSR_index: 0, page: null } }];
  private index = 0;
  private discardAhead = false;
  readonly history = createHistory({
    getLocation: () => { const entry = this.entries[this.index]; return { href: entry.path, pathname: entry.path, search: "", hash: "", state: entry.state }; },
    getLength: () => this.entries.length,
    pushState: (path, state: ParsedHistoryState) => {
      this.entries.splice(this.index + 1, this.entries.length, { path, state });
      this.index++;
      if (this.entries.length > MAX_PAGES + 1) { this.entries.shift(); this.index--; this.entries.forEach((entry, index) => { entry.state.__TSR_index = index; }); }
    },
    replaceState: (path, state: ParsedHistoryState) => {
      this.entries[this.index] = { path, state };
      if (this.discardAhead) this.entries.splice(this.index + 1);
      this.discardAhead = false;
    },
    go: delta => { this.index = Math.max(0, Math.min(this.entries.length - 1, this.index + delta)); },
    back: () => { this.index = Math.max(0, this.index - 1); },
    forward: () => { this.index = Math.min(this.entries.length - 1, this.index + 1); },
    createHref: path => path,
  });
  // Each retained view owns its scroll position; the router must not reset it.
  readonly router = createRouter({ routeTree, history: this.history, scrollRestoration: () => false });

  start(page: Location) { this.history.replace(paths[page.view], { page }); }
  visit(page: Location, next: Location, replace = false) {
    if (JSON.stringify(page) === JSON.stringify(next)) return;
    // Project tabs can change without a route change; record the live tab before leaving.
    this.history.replace(paths[page.view], { page });
    if (replace) { this.discardAhead = true; this.history.replace(paths[next.view], { page: next }); }
    else this.history.push(paths[next.view], { page: next });
  }
  back(current: Location) { return this.step(current, false); }
  forward(current: Location) { return this.step(current, true); }
  /// Whether there is a page that way.
  has(forward: boolean) { return forward ? this.index < this.entries.length - 1 : this.index > 0; }
  private step(current: Location, forward: boolean): Location | null {
    if (!this.has(forward)) return null;
    this.history.replace(paths[current.view], { page: current });
    if (forward) this.history.forward(); else this.history.back();
    return this.history.location.state.page ?? null;
  }
}

export const windowHistory = new WindowHistory();
