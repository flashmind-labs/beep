// Dragging a session pane's tab to the other dock, by pointer or keys.
import { type Box, type Dock, dropDock, moveTab, paneSize, SIDE_BY_SIDE_MIN } from "./media-state.ts";
import { DRAG_START, P, saveLayout, DOCK_RIGHT, DOCK_BOTTOM, shell, main, paneOf, dockOf, draw } from "./media.ts";

/// Where a tab drag stands: the tab, where the pointer went down, and once
/// it moved far enough, the tab's ghost under the pointer and the dock it
/// would drop in.
interface TabDrag {
  id: string;
  tab: HTMLElement;
  pointer: number;
  x: number;
  y: number;
  ghost: HTMLElement | null;
  to: Dock | null;
}

export let drag: TabDrag | null = null;

export function tabDown(e: PointerEvent) {
  if (e.button !== 0 || drag) return;
  const target = e.target as HTMLElement;
  if (target.closest("[data-mclose]")) return;
  const tab = target.closest<HTMLElement>("[data-mtab]");
  if (!tab) return;
  drag = { id: tab.dataset.mtab!, tab, pointer: e.pointerId, x: e.clientX, y: e.clientY, ghost: null, to: null };
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
    d.ghost = d.tab.cloneNode(true) as HTMLElement;
    d.ghost.classList.add("agtabghost");
    d.ghost.removeAttribute("id");
    document.body.append(d.ghost);
    d.tab.classList.add("dragging");
    el.classList.add("tabdrag");
    document.addEventListener("keydown", dragKey, true);
  }
  d.ghost.style.transform = `translate(${e.clientX + 8}px, ${e.clientY + 6}px)`;
  // Over a dock's own pane or strip, the tab goes there; else by the edge.
  const over = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>(".agpane, .agpstrip")?.dataset.dock as Dock | undefined;
  const r = el.getBoundingClientRect();
  const to = over ?? dropDock(r, e.clientX, e.clientY, el.clientWidth >= SIDE_BY_SIDE_MIN);
  d.to = to && to !== dockOf(d.id) ? to : null;
  drawDropZone(d.to);
}

/// Shows where a dragged tab would go: over that dock, or where it would
/// open.
function drawDropZone(to: Dock | null) {
  const zone = document.getElementById("agdropzone"), el = shell(), box = main();
  if (!zone || !el || !box) return;
  zone.hidden = !to;
  if (!to) return;
  const s = el.getBoundingClientRect();
  const pane = paneOf(to);
  let r: Box;
  if (pane && !pane.hidden) r = pane.getBoundingClientRect();
  else if (to === "right") {
    const w = paneSize("right", P.layout.right, s.width);
    r = { left: s.right - w, top: s.top, width: w, height: s.height };
  } else {
    const m = box.getBoundingClientRect(), hgt = paneSize("bottom", P.layout.bottom, m.height);
    r = { left: m.left, top: m.bottom - hgt, width: m.width, height: hgt };
  }
  Object.assign(zone.style, { left: `${r.left - s.left}px`, top: `${r.top - s.top}px`, width: `${r.width}px`, height: `${r.height}px` });
  zone.innerHTML = `${to === "right" ? DOCK_RIGHT : DOCK_BOTTOM}<span>${to === "right" ? "Dock on the right" : "Dock at the bottom"}</span>`;
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
  d.ghost.remove();
  d.tab.classList.remove("dragging");
  shell()?.classList.remove("tabdrag");
  drawDropZone(null);
  // The click that ends a drag does not pick the tab.
  P.dragged = true;
  setTimeout(() => { P.dragged = false; }, 0);
  // What changed while it dragged.
  draw();
}

/// Moves tab `id` to dock `to`, on screen there. Its view moves with it,
/// so a terminal keeps its shell and screen.
function moveTo(id: string, to: Dock) {
  P.layout = moveTab(P.layout, id, to);
  if (id.startsWith("data:")) P.moved.set(id, to);
  saveLayout();
  P.on[to] = id;
  draw();
}
