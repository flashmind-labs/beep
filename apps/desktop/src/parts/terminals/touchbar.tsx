// The touch key bar above the soft keyboard, for termview.ts. Drawn as in the
// phone layout.
import { createRef } from "react";
import { createDocumentScope } from "../../ui/document-root.tsx";
import { TermKeyState, type TermKey, type TermMod } from "../../stores/terminals/touchkeys-state.ts";
import { TouchKeys } from "./touchbar-view.tsx";

interface TouchBar {
  /// Soft keyboard input, with any armed modifier applied to it.
  apply(typed: string): string;
  /// Shows the bar `bottom` pixels above the window's bottom edge, or hides it for null.
  show(bottom: number | null): void;
  height(): number;
  dispose(): void;
}

/// `send` gets the bytes of a key press.
export function makeTouchBar(send: (data: string) => void): TouchBar {
  const state = new TermKeyState();
  const scope = createDocumentScope<"bar">();
  const bar = createRef<HTMLDivElement>();
  let bottom: number | null = null;
  const toggle = (mod: TermMod) => { state.toggle(mod); paint(); };
  const press = (key: TermKey) => { const armed = state.mods.size > 0; send(state.press(key)); if (armed) paint(); };
  const paint = () => scope.set([{ slot: "bar", host: document.body, content: <TouchKeys bar={bar} bottom={bottom} mods={new Set(state.mods)} toggle={toggle} press={press} /> }]);
  paint();

  return {
    apply: typed => { const armed = state.mods.size > 0; const out = state.applyTyped(typed); if (armed && state.mods.size === 0) paint(); return out; },
    show: at => { if (at === bottom) return; bottom = at; paint(); },
    height: () => (bottom === null ? 0 : bar.current?.offsetHeight ?? 0),
    dispose: () => scope.dispose(),
  };
}
