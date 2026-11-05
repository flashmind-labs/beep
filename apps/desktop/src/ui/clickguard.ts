import { swallowsClick } from "./clickguard-state.ts";

/// Drops secondary and Ctrl+clicks before any row sees them. Once per window.
export function guardClicks() {
  document.addEventListener("click", e => {
    const on = e.target instanceof Element ? e.target : null;
    const native = !!on?.closest("input, textarea, [contenteditable], .tm-xterm, a[href]");
    if (swallowsClick({ button: e.button, ctrlKey: e.ctrlKey, native })) {
      e.stopPropagation();
      e.preventDefault();
    }
  }, true);
}
