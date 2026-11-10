// Where the pane's docks sit and how big they are: the split of tabs
// between them, zoom, ctrl-tab between tabs, and the dividers.
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { type Dock, DOCKS, onZoomKey, paneSize, SIDE_BY_SIDE_MIN, splitTabs } from "./media-state.ts";
import { artifactCount } from "./artifacts.ts";
import { S } from "../bar/core.ts";
import { runningCount } from "./running.ts";
import { dragHandle } from "../ui/drag.ts";
import { P, rememberPane, saveLayout, showTab, FILES, CHANGES, ARTS, RUNNING, SUBS } from "./media.ts";

/// How far an arrow key moves a divider.
const ARROW_STEP = 24;

// ----- The pane's place -----

export const shell = () => document.getElementById("agshell");
/// The conversation and the bottom dock under it.
export const main = () => document.getElementById("agmain");
export const paneOf = (d: Dock) => document.getElementById(`agpane-${d}`);

/// Every tab's id, in the order the tab strips show them.
export function tabIds() {
  return [FILES, CHANGES, ...(runningCount() ? [RUNNING] : []), ...(artifactCount() ? [ARTS] : []), ...(P.subCount ? [SUBS] : []), ...P.tabs.map(t => t.id)];
}

/// The tabs in each dock, as the shell's width lets them sit.
export function docked(): Record<Dock, string[]> {
  return splitTabs(P.layout, tabIds(), shell()?.clientWidth ?? SIDE_BY_SIDE_MIN, P.moved);
}

/// The dock tab `id` shows in now.
export function dockOf(id: string): Dock {
  return docked().bottom.includes(id) ? "bottom" : "right";
}

/// Lays the docks out for the shell's size: which show, their size,
/// folded or not. A dock with no tabs goes.
let paneWasShown = false;

export function place() {
  const el = shell(), box = main();
  if (!el || !box) return;
  const narrow = el.clientWidth < SIDE_BY_SIDE_MIN;
  el.classList.toggle("narrow", narrow);
  const split = docked();

  for (const d of DOCKS) {
    const pane = paneOf(d), div = document.getElementById(`agdiv-${d}`), strip = document.getElementById(`agpstrip-${d}`);
    if (!pane || !div || !strip) continue;
    // The Files tab is always there while a session shows, so one dock is.
    const has = !!P.session && split[d].length > 0;
    pane.hidden = div.hidden = !has || P.layout.hidden[d];
    strip.hidden = !has || !P.layout.hidden[d];
    if (pane.hidden) continue;
    const total = d === "right" ? el.clientWidth : box.clientHeight;
    pane.style.flexBasis = `${paneSize(d, P.layout[d], total)}px`;
    div.setAttribute("aria-orientation", d === "right" ? "vertical" : "horizontal");
  }

  const shown = DOCKS.some(d => paneOf(d)?.hidden === false);
  if (shown && !paneWasShown) queueMicrotask(() => window.dispatchEvent(new Event("agent-pane-shown")));
  paneWasShown = shown;
  refreshTopicSubscriptions();
  // A zoomed dock that folded, emptied or moved away ends the zoom.
  if (P.zoom && paneOf(P.zoom)?.hidden !== false) P.zoom = null;
  el.classList.toggle("zoomed", !!P.zoom);
  for (const d of DOCKS) paneOf(d)?.classList.toggle("zoomed", P.zoom === d);
}

/// ⇧esc: the dock with focus, else the right one, else the bottom one, fills the session
/// over the conversation; again puts it back. Only classes change, so a model, a video
/// or a terminal keeps its state and fits the new size on its own.
function toggleZoom() {
  if (P.zoom) P.zoom = null;
  else {
    const shown = DOCKS.filter(d => paneOf(d)?.hidden === false);
    P.zoom = shown.find(d => paneOf(d)?.contains(document.activeElement)) ?? shown.find(d => d === "right") ?? shown[0] ?? null;
  }

  place();
  rememberPane();
  // From the input, the zoomed tab takes the keys: a terminal fills the view and is typed in.
  const pane = P.zoom && paneOf(P.zoom);

  if (pane && !pane.contains(document.activeElement)) {
    const term = P.tabs.find(t => t.id === P.on[P.zoom!])?.term;
    if (term) term.screen.focus(); else pane.focus({ preventScroll: true });
  }
}

// S.view stays "agent" while the center shows another page; only a session on screen zooms.
onZoomKey(() => S.view === "agent" && !!P.session && !!shell()?.clientWidth, toggleZoom);

// ⌃tab with focus in a dock: its next tab, round to the first; ⌃⇧tab the one before.
window.addEventListener("keydown", e => {
  if (e.key !== "Tab" || !e.ctrlKey || e.metaKey || e.altKey) return;
  const target = e.target as Element;
  const pane = target.closest<HTMLElement>(".agpane[data-dock]");
  if (!pane) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const d = pane.dataset.dock as Dock, ids = docked()[d];
  const step = e.shiftKey ? ids.length - 1 : 1;
  showTab(d, ids[(ids.indexOf(P.on[d]) + step) % ids.length]);
}, true);

export function wireDivider(dock: Dock) {
  const div = document.getElementById(`agdiv-${dock}`)!;
  const set = (size: number | null) => {
    const el = shell(), box = main();
    if (!el || !box) return;
    P.layout[dock] = size == null ? null : paneSize(dock, size, dock === "right" ? el.clientWidth : box.clientHeight);
    place();
  };
  dragHandle(div, () => {
    const el = shell()!, box = main()!;
    el.classList.add("resizing", `resizing-${dock}`);
    div.classList.add("active");
    const right = dock === "right";
    const r = (right ? el : box).getBoundingClientRect();
    return {
      move: ev => set(right ? r.right - ev.clientX : r.bottom - ev.clientY),
      end: () => {
        el.classList.remove("resizing", `resizing-${dock}`);
        div.classList.remove("active");
        saveLayout();
      },
    };
  });
  div.addEventListener("dblclick", () => { set(null); saveLayout(); });
  div.addEventListener("keydown", e => {
    const pane = paneOf(dock)!;
    const now = dock === "right" ? pane.offsetWidth : pane.offsetHeight;
    const grow: Record<string, number> = dock === "right" ? { ArrowLeft: 1, ArrowRight: -1 } : { ArrowUp: 1, ArrowDown: -1 };
    const by = grow[e.key];
    if (!by) return;
    e.preventDefault();
    e.stopPropagation();
    set(now + by * ARROW_STEP);
    saveLayout();
  });
}
