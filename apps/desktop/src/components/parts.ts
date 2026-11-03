// Small pieces of the command center window that several views draw the
// same way. They build on `el` and `button` from projects-state.ts.
import { button, el, type Kid } from "../projects-state.ts";

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

/// A panel with nothing to list yet; `kids` say why.
export function emptyPanel(...kids: Kid[]) {
  return el("div", "cc-panel cc-empty", ...kids);
}

/// A note above a list after a dot: `dot` is "", "off", "warn" or "bad".
export function note(dot: string, ...kids: Kid[]) {
  return el("div", "cc-note", el("span", dot ? `ddot ${dot}` : "ddot"), el("span", "", ...kids));
}

/// A note in red above a list: what failed.
export function errorNote(text: string) {
  return el("div", "cc-note", el("span", "ddot bad"), el("span", "cc-err", text));
}

/// A state chip such as "running" or "needs reply"; `cls` colours it, and
/// `spin` puts a spinner before the text.
export function stateChip(cls: string, text: string, spin = false) {
  return el("span", cls ? `pj-st ${cls}` : "pj-st", spin ? el("span", "dspin") : null, text);
}
