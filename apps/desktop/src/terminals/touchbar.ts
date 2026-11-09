// The touch key bar above the soft keyboard, for termview.ts. Drawn as in the
// phone mock (mockups/ios/index.html).
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentView } from "../ui/document-root.tsx";
import { TermKeyState, type TermKey, type TermMod } from "./touchkeys-state.ts";
import { TouchKeys } from "./touchbar-view.tsx";

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
  dispose(): void;
}

/// `send` gets the bytes of a key press.
export function makeTouchBar(send: (data: string) => void): TouchBar {
  const state = new TermKeyState();
  const el = document.createElement("div");
  el.className = "tm-keys";
  el.hidden = true;
  const root = createDocumentView(el);
  const toggle = (mod: TermMod) => { state.toggle(mod); paint(); };
  const press = (key: TermKey) => { const armed = state.mods.size > 0; send(state.press(key)); if (armed) paint(); };
  const paint = () => flushSync(() => root.render(createElement(TouchKeys, { mods: new Set(state.mods), toggle, press })));
  paint();

  return {
    el,
    apply: typed => { const armed = state.mods.size > 0; const out = state.applyTyped(typed); if (armed && state.mods.size === 0) paint(); return out; },
    setShown: on => { el.hidden = !on; },
    height: () => (el.hidden ? 0 : el.offsetHeight),
    dispose: () => { root.unmount(); el.remove(); },
  };
}
