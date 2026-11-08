// The touch key bar above the soft keyboard, for termview.ts. Drawn as in the
// phone mock (mockups/ios/31-command-center.js, 06-command-center.css).
import { KEY_ROWS, labelKey, TermKeyState, type TermMod } from "./touchkeys-state.ts";

/// Whether the main pointer is a finger, as on a phone or tablet.
export function isTouchDevice(): boolean {
  return typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;
}

export interface TouchBar {
  el: HTMLElement;
  /// Soft keyboard input, with any armed modifier applied to it.
  apply(typed: string): string;
  setShown(on: boolean): void;
  height(): number;
}

/// `send` gets the bytes of a key press.
export function makeTouchBar(send: (data: string) => void): TouchBar {
  const state = new TermKeyState();
  const el = document.createElement("div");
  el.className = "tm-keys";
  el.hidden = true;
  const mods = new Map<TermMod, HTMLButtonElement>();
  const paint = () => { for (const [m, b] of mods) b.setAttribute("aria-pressed", String(state.mods.has(m))); };
  for (const label of KEY_ROWS.flat()) {
    if (!label) { el.append(document.createElement("span")); continue; }
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    // Keep focus in the terminal so the soft keyboard stays up.
    b.addEventListener("pointerdown", e => e.preventDefault());
    const what = labelKey(label);
    if ("mod" in what) {
      mods.set(what.mod, b);
      b.setAttribute("aria-pressed", "false");
      b.addEventListener("click", () => { state.toggle(what.mod); paint(); });
    } else {
      b.addEventListener("click", () => { send(state.press(what.key)); paint(); });
    }
    el.append(b);
  }
  return {
    el,
    apply: typed => { const out = state.applyTyped(typed); paint(); return out; },
    setShown: on => { el.hidden = !on; },
    height: () => (el.hidden ? 0 : el.offsetHeight),
  };
}
