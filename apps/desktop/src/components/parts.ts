// Small pieces of the command center window that several views draw the
// same way. They build on `el` and `button` from projects-state.ts.
import { button, el } from "../projects-state.ts";

/// Fixed SVG markup as a node, never server text.
export function svgEl(markup: string) {
  const s = el("span");
  s.style.display = "inline-flex";
  s.innerHTML = markup;
  return s.firstChild;
}

/// A `dbtn` button with `cls` that runs `on`.
export function dbtn(label: string | Node, cls: string, on: (e: MouseEvent) => void) {
  return button(label, `dbtn${cls ? " " + cls : ""}`, on);
}
