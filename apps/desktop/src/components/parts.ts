// Small pieces of the command center window that several views draw the
// same way. They build on `el` and `button` from projects-state.ts.
import { button, el, type Kid } from "../projects-state.ts";
import type { Freshness } from "../pagecache.ts";

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

/// A section header's note on its data: a spinner while it reads again
/// data that is not new, else "updated 4 min ago"; null for nothing to say.
export function freshNote(f: Freshness) {
  if (f.spin) {
    const spin = el("span", "dspin sw-spin");
    spin.title = "Updating";
    return spin;
  }
  return f.text ? el("small", "sw-age", f.text) : null;
}

/// A state chip such as "running" or "needs reply"; `cls` colours it, and
/// `spin` puts a spinner before the text.
export function stateChip(cls: string, text: string, spin = false) {
  return el("span", cls ? `pj-st ${cls}` : "pj-st", spin ? el("span", "dspin") : null, text);
}

/// A segmented control: one button per option, the `current` one pressed.
export function segmented<T>(options: readonly { label: string; value: T }[], current: T, choose: (value: T) => void) {
  return el("span", "pb-seg", ...options.map(o => {
    const b = button(o.label, "", () => choose(o.value));
    b.setAttribute("aria-pressed", String(o.value === current));
    return b;
  }));
}
