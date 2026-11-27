import { $, bar, PANE } from "./core.ts";
import { invoke } from "../../ui/tauri.ts";
import { ZOOMED } from "../../ui/zoom-state.ts";

// ---------- Size ----------

/// Floating over the bar, so they take no height of their own.
const FLOATING = new Set(["viewer", "saved"]);
/// How tall the bar gets while a picture is open, within the screen.
const VIEWER_HEIGHT = 560;
const scrolls = (el: Element) => el.classList.contains("dbody") || el.classList.contains("dlist") || el.classList.contains("agc-wrap");
/// How tall `el` is with all of its content, scrolled or not.
const tall = (el: HTMLElement) => scrolls(el) ? el.scrollHeight : el.offsetHeight;
let fitFrame = 0, barHeight = 0;
/// Display-contents wrappers have no box; their children contribute instead.
/// `wrappers` collects the wrappers passed through.
function boxes(parent: Element, wrappers: Set<Node> | null = null): HTMLElement[] {
  return [...parent.children].flatMap(child => {
    if (!(child instanceof HTMLElement)) return [];
    if (window.getComputedStyle(child).display !== "contents") return [child];

    wrappers?.add(child);
    return boxes(child, wrappers);
  });
}
/// Makes the next fit resize the bar even to the height it had.
export function forgetHeight() { barHeight = 0; }
/// Sizes the bar to its content once per frame, so a streaming reply does
/// not resize the window on every token. In a window's pane the bar does
/// not size its window, so nothing is measured.
function fit() {
  if (PANE) return;
  fitFrame ||= requestAnimationFrame(() => {
    fitFrame = 0;
    resize();
  });
}

function resize() {
  let height = 2;
  for (const el of boxes(bar)) if (!FLOATING.has(el.id)) height += tall(el);
  if ($("#viewer")) height = Math.max(height, VIEWER_HEIGHT);
  if (height === barHeight) return;
  barHeight = height;
  invoke("resize_bar", { height }).catch(() => {});
}

const sizer = new ResizeObserver(fit);
/// The elements whose children are the boxes `sizer` watches.
let parents = new Set<Node>();
function observeBoxes() {
  sizer.disconnect();
  const bodies: Element[] = [bar, ...bar.querySelectorAll(".dbody")];
  parents = new Set(bodies);
  for (const parent of bodies) for (const el of boxes(parent, parents)) sizer.observe(el);
}
/// By node type, so the bar also runs in a test DOM without its globals.
const isElement = (node: Node): node is Element => node.nodeType === 1;
const holdsBody = (node: Node) => isElement(node) && (node.matches(".dbody") || node.querySelector(".dbody") !== null);

// The window's height is the page's zoomed, so a new zoom resizes it at the same page height.
window.addEventListener(ZOOMED, () => { forgetHeight(); fit(); });
// Text and rows inside a box only fit; the boxes change only with their parents' children.
new MutationObserver(records => {
  const moved = records.some(r => r.type === "childList" && (parents.has(r.target) || [...r.addedNodes, ...r.removedNodes].some(holdsBody)));
  if (moved) observeBoxes();

  fit();
}).observe(bar, { childList: true, subtree: true, characterData: true });

