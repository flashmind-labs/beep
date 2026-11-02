import { drawMap } from "./chat.ts";
import { $, bar, invoke, PANE } from "./core.ts";

// ---------- Size ----------

/// Floating over the bar, so they take no height of their own.
export const FLOATING = new Set(["latest", "map", "keys", "viewer", "saved"]);
/// How tall the bar gets while a picture is open, within the screen.
export const VIEWER_HEIGHT = 560;
export const scrolls = (el: Element) => el.classList.contains("dbody") || el.classList.contains("dlist") || el.classList.contains("dkeys");
/// How tall `el` is with all of its content, scrolled or not.
const tall = (el: HTMLElement) => scrolls(el) ? el.scrollHeight : el.offsetHeight;
export const natural = (el: Element) => ([...el.children] as HTMLElement[]).reduce((n, c) => n + tall(c), 0);
let fitFrame = 0, barHeight = 0;
/// Makes the next fit resize the bar even to the height it had.
export function forgetHeight() { barHeight = 0; }
/// Sizes the bar to its content once per frame, so a streaming reply does
/// not resize the window on every token.
export function fit() {
  fitFrame ||= requestAnimationFrame(() => {
    fitFrame = 0;
    let height = 2;
    for (const el of bar.children as HTMLCollectionOf<HTMLElement>) if (!FLOATING.has(el.id)) height += tall(el);
    if ($("#viewer")) height = Math.max(height, VIEWER_HEIGHT);
    const keys = $("#keys");
    if (keys) height = Math.max(height, natural(keys) + 2);
    if (!PANE && height !== barHeight) {
      barHeight = height;
      invoke("resize_bar", { height }).catch(() => {});
    }
    drawMap();
    bar.querySelectorAll<HTMLElement>(".dbody, .dlist").forEach(fadeEdges);
  });
}

/// Fades a scrolling list at each edge that has more above or below it.
export function fadeEdges(el: HTMLElement) {
  const up = el.scrollTop > 2, down = el.scrollHeight - el.scrollTop - el.clientHeight > 2;
  el.classList.toggle("dfade", up || down);
  el.style.setProperty("--ft", up ? "28px" : "0px");
  el.style.setProperty("--fb", down ? "28px" : "0px");
}
bar.addEventListener("scroll", e => {
  const el = e.target as HTMLElement;
  if (el.matches?.(".dbody, .dlist")) fadeEdges(el);
}, true);
export const sizer = new ResizeObserver(fit);
new MutationObserver(() => {
  sizer.disconnect();
  for (const el of bar.querySelectorAll(":scope > *, .dbody > *")) sizer.observe(el);
  fit();
}).observe(bar, { childList: true, subtree: true, characterData: true });

