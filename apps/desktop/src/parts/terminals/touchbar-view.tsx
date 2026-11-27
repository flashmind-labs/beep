import type { RefObject } from "react";
import { KEY_ROWS, labelKey, type TermKey, type TermMod } from "../../stores/terminals/touchkeys-state.ts";

/// The key bar, `bottom` pixels above the window's bottom edge; hidden for null.
export function TouchKeys({ bar, bottom, mods, toggle, press }: { bar: RefObject<HTMLDivElement | null>; bottom: number | null; mods: ReadonlySet<TermMod>; toggle(mod: TermMod): void; press(key: TermKey): void }) {
  return <div ref={bar} className="tm-keys" hidden={bottom === null} style={bottom ? { bottom: `${bottom}px` } : undefined}>{KEY_ROWS.flat().map((label, index) => {
    if (!label) return <span key={index} />;
    const key = labelKey(label);
    return <button key={index} type="button" aria-pressed={"mod" in key ? mods.has(key.mod) : undefined}
      onPointerDown={event => event.preventDefault()}
      onClick={() => { if ("mod" in key) toggle(key.mod); else press(key.key); }}>{label}</button>;
  })}</div>;
}
