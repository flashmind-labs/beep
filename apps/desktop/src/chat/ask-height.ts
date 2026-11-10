// The bar's prompt: how tall it grows, and the grip that sets its height.
import { $$ } from "../bar/core.ts";
import { dragHandle } from "../ui/drag.ts";

const ASK_HEIGHT = "bar.askHeight", ASK_MAX = 420;
/// A height the user dragged the bar's prompt to; null lets it follow its lines.
let askHeight: number | null = null;
try { askHeight = Number(localStorage.getItem(ASK_HEIGHT)) || null; } catch {}
/// The prompt grows with its lines, up to six, then scrolls. A height the
/// user dragged it to is the least it shows, so its lines still fit.
export function grow(ask: HTMLTextAreaElement) {
  ask.style.height = "auto";
  const lines = Math.min(ask.scrollHeight, 156);
  const dragged = askHeight && ask.id === "ask" ? Math.min(askHeight, ASK_MAX) : 0;
  ask.style.height = Math.max(lines, dragged) + "px";
}
/// Dragging the grip under the prompt sets its height, kept for next time.
export function dragGrip(ask: HTMLTextAreaElement) {
  const grip = $$("#grip");
  dragHandle(grip, e => {
    grip.classList.add("on");
    const startY = e.clientY, startH = ask.offsetHeight;
    return {
      move: m => {
        // A click on the grip is not a drag: it would pin the prompt to one line.
        if (Math.abs(m.clientY - startY) < 4) return;
        askHeight = Math.max(26, Math.min(startH + m.clientY - startY, ASK_MAX));
        grow(ask);
      },
      end: () => {
        grip.classList.remove("on");
        try { if (askHeight) localStorage.setItem(ASK_HEIGHT, String(askHeight)); } catch {}
      },
    };
  });
  grip.addEventListener("dblclick", () => {
    askHeight = null;
    try { localStorage.removeItem(ASK_HEIGHT); } catch {}
    grow(ask);
  });
  grow(ask);
}
