// Dragging a session pane's tab to the other dock, by pointer or keys. The
// ghost under the pointer and the drop zone are drawn by dock-view.tsx from
// the pane's model; the ghost's place follows the pointer through its ref.
import { createRef } from "react";
import { type Box, type Dock, DOCKS, dropDock, moveTab, paneSize, SIDE_BY_SIDE_MIN } from "./media-state.ts";
import { saveLayout } from "./pane-memory.ts";
import { DRAG_START, P, redraw } from "./pane-state.ts";
import { paneTabItem } from "./pane-tabs.ts";
import { shell, main, paneOf, dockOf, paneModel, drawPaneNow, setOn, setPaneModel, stripRefs } from "./media-layout.ts";

/// Where a tab drag stands: the tab, where the pointer went down, whether it
/// moved far enough to drag, and the dock it would drop in.
interface TabDrag {
  id: string;
  tab: HTMLElement;
  pointer: number;
  x: number;
  y: number;
  ghost: boolean;
  to: Dock | null;
}

export let drag: TabDrag | null = null;

/// The dragged tab's ghost, while it shows.
export const ghostRef = createRef<HTMLButtonElement>();

/// The pointer went down on tab `id`, whose button is `tab`.
export function tabDown(e: PointerEvent, id: string, tab: HTMLElement) {
  if (e.button !== 0 || drag) return;
  drag = { id, tab, pointer: e.pointerId, x: e.clientX, y: e.clientY, ghost: false, to: null };
  tab.setPointerCapture(e.pointerId);
  tab.addEventListener("pointermove", tabMove);
  tab.addEventListener("pointerup", tabUp);
  tab.addEventListener("pointercancel", endDrag);
  tab.addEventListener("lostpointercapture", endDrag);
}

function tabMove(e: PointerEvent) {
  const d = drag, el = shell();
  if (!d || e.pointerId !== d.pointer || !el) return;

  if (!d.ghost) {
    if (Math.hypot(e.clientX - d.x, e.clientY - d.y) < DRAG_START) return;
    d.ghost = true;
    setPaneModel({ drag: { item: paneTabItem(d.id), selected: paneModel.getState().on[dockOf(d.id)] === d.id, to: null, zone: null } });
    drawPaneNow();
    document.addEventListener("keydown", dragKey, true);
  }

  ghostRef.current?.style.setProperty("--x", `${e.clientX + 8}px`);
  ghostRef.current?.style.setProperty("--y", `${e.clientY + 6}px`);
  // Over a dock's own pane or strip, the tab goes there; else by the edge.
  const at = document.elementFromPoint(e.clientX, e.clientY);
  const over = DOCKS.find(dock => !!at && (!!paneOf(dock)?.contains(at) || !!stripRefs[dock].current?.contains(at)));
  const r = el.getBoundingClientRect();
  const to = over ?? dropDock(r, e.clientX, e.clientY, el.clientWidth >= SIDE_BY_SIDE_MIN);
  d.to = to && to !== dockOf(d.id) ? to : null;
  showDropZone(d.to);
}

/// Shows where a dragged tab would go: over that dock, or where it would
/// open.
function showDropZone(to: Dock | null) {
  const now = paneModel.getState().drag, { layout } = paneModel.getState(), el = shell(), box = main();
  if (!now || now.to === to || !el || !box) return;
  if (!to) { paneModel.setState({ drag: { ...now, to: null, zone: null } }); return; }
  const s = el.getBoundingClientRect();
  const pane = paneOf(to);
  let r: Box;

  if (pane && paneModel.getState().places[to].shown) r = (pane.firstElementChild ?? pane).getBoundingClientRect();
  else if (to === "right") {
    const w = paneSize("right", layout.right, s.width);
    r = { left: s.right - w, top: s.top, width: w, height: s.height };
  } else {
    const m = box.getBoundingClientRect(), hgt = paneSize("bottom", layout.bottom, m.height);
    r = { left: m.left, top: m.bottom - hgt, width: m.width, height: hgt };
  }

  paneModel.setState({ drag: { ...now, to, zone: { left: r.left - s.left, top: r.top - s.top, width: r.width, height: r.height } } });
}

function tabUp(e: PointerEvent) {
  const d = drag;
  if (!d || e.pointerId !== d.pointer) return;
  const to = d.ghost ? d.to : null;
  endDrag();
  if (to) moveTo(d.id, to);
}

function dragKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  endDrag();
}

function endDrag() {
  const d = drag;
  if (!d) return;
  drag = null;
  d.tab.removeEventListener("pointermove", tabMove);
  d.tab.removeEventListener("pointerup", tabUp);
  d.tab.removeEventListener("pointercancel", endDrag);
  d.tab.removeEventListener("lostpointercapture", endDrag);
  if (d.tab.hasPointerCapture(d.pointer)) d.tab.releasePointerCapture(d.pointer);
  document.removeEventListener("keydown", dragKey, true);
  if (!d.ghost) return;
  paneModel.setState({ drag: null });
  // The click that ends a drag does not pick the tab.
  P.dragged = true;
  setTimeout(() => { P.dragged = false; }, 0);
  // What changed while it dragged.
  redraw();
}

/// Moves tab `id` to dock `to`, on screen there. Its view moves with it,
/// so a terminal keeps its shell and screen.
function moveTo(id: string, to: Dock) {
  paneModel.setState(s => ({ layout: moveTab(s.layout, id, to) }));
  if (id.startsWith("data:")) P.moved.set(id, to);
  saveLayout();
  setOn(to, id);
  redraw();
}
