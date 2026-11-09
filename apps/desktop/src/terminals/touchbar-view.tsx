import { KEY_ROWS, labelKey, type TermKey, type TermMod } from "./touchkeys-state.ts";

export function TouchKeys({ mods, toggle, press }: { mods: ReadonlySet<TermMod>; toggle(mod: TermMod): void; press(key: TermKey): void }) {
  return <>{KEY_ROWS.flat().map((label, index) => {
    if (!label) return <span key={index} />;
    const key = labelKey(label);
    return <button key={index} type="button" aria-pressed={"mod" in key ? mods.has(key.mod) : undefined}
      onPointerDown={event => event.preventDefault()}
      onClick={() => { if ("mod" in key) toggle(key.mod); else press(key.key); }}>{label}</button>;
  })}</>;
}
