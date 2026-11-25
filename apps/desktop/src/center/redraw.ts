// What asks a view to draw again. A module that changes state calls `redraw`;
// the view that draws follows with `redraws.<name>.add`. Below every page, so
// readers of state need not import the views that show it.
import { currentLocation } from "./nav.ts";

export const redraws = {
  /// Home, while it shows.
  home: new Set<() => void>(),
  /// The sidebar's links and groups.
  side: new Set<() => void>(),
  /// The project or chat sessions beside a page.
  rail: new Set<() => void>(),
  /// The project page, the sessions beside it and the empty agent view's bar.
  projects: new Set<() => void>(),
  /// The Projects page and its header, while it shows.
  projectsPage: new Set<() => void>(),
};

export function redraw(name: keyof typeof redraws) {
  for (const draw of redraws[name]) draw();
}

/// The sidebar's conflict counts and Home's conflict cards.
export function redrawSync() {
  redraw("side");
  if (currentLocation()?.view === "home") redraw("home");
}
