import { drawMap } from "./chat.ts";
import { $, bar, invoke, PANE } from "./core.ts";

// ---------- Size ----------

/// Floating over the bar, so they take no height of their own.
export const FLOATING = new Set(["latest", "map", "keys", "viewer", "saved"]);
/// How tall the bar gets while a picture is open, within the screen.
export const VIEWER_HEIGHT = 560;
export const scrolls = el => el.classList.contains("dbody") || el.classList.contains("dlist") || el.classList.contains("dkeys");
export const natural = el => [...el.children].reduce((n, c) => n + (scrolls(c) ? c.scrollHeight : c.offsetHeight), 0);
let fitFrame = 0, barHeight = 0;
/// Makes the next fit resize the bar even to the height it had.
export function forgetHeight() { barHeight = 0; }
/// Sizes the bar to its content once per frame, so a streaming reply does
/// not resize the window on every token.
export function fit() {
  fitFrame ||= requestAnimationFrame(() => {
    fitFrame = 0;
    let height = 2;
    for (const el of bar.children) if (!FLOATING.has(el.id)) height += scrolls(el) ? el.scrollHeight : el.offsetHeight;
    if ($("#viewer")) height = Math.max(height, VIEWER_HEIGHT);
    const keys = $("#keys");
    if (keys) height = Math.max(height, natural(keys) + 2);
    if (!PANE && height !== barHeight) {
      barHeight = height;
      invoke("resize_bar", { height }).catch(() => {});
    }
    drawMap();
    bar.querySelectorAll(".dbody, .dlist").forEach(fadeEdges);
  });
}

/// Fades a scrolling list at each edge that has more above or below it.
export function fadeEdges(el) {
  const up = el.scrollTop > 2, down = el.scrollHeight - el.scrollTop - el.clientHeight > 2;
  el.classList.toggle("dfade", up || down);
  el.style.setProperty("--ft", up ? "28px" : "0px");
  el.style.setProperty("--fb", down ? "28px" : "0px");
}
bar.addEventListener("scroll", e => { if (e.target.matches?.(".dbody, .dlist")) fadeEdges(e.target); }, true);
export const sizer = new ResizeObserver(fit);
new MutationObserver(() => {
  sizer.disconnect();
  for (const el of bar.querySelectorAll(":scope > *, .dbody > *")) sizer.observe(el);
  fit();
}).observe(bar, { childList: true, subtree: true, characterData: true });

