// Small pieces Home, the sidebar and the Projects page draw with.
import { dbtn as button, svgEl as icon } from "./parts.ts";
import { el as h, type Kid } from "./dom.ts";
import { IC } from "./icons.ts";

/// Why a call failed, cut short; a conflict means another answer won.
export function failure(e: unknown) {
  const text = String(e);
  return conflict(e) ? "Already answered" : text.length > 90 ? text.slice(0, 90) + "..." : text;
}
export function conflict(e: unknown) { return /^conflict/i.test(String(e)); }

// ---------- Pieces ----------

/// A button with a + before `label`.
export function plusLabel(label: string, cls: string, on: () => void) {
  const b = button("", cls, on);
  b.append(icon(IC.plus)!, ` ${label}`);
  return b;
}

/// A button inside a row that does its own thing, not the row's.
export function rowButton(label: string | Node, cls: string, on: () => void) {
  return button(label, cls, e => { e.stopPropagation(); on(); });
}

/// One row of Running now, Needs you, Updates or Coming up: a title and one
/// muted line, opening `open` on a click, with `right` beside it.
export function line(lead: Node | null, title: string, sub: Kid, right: Kid[], open: () => void) {
  const t = h("button", "t", h("b", "", title), h("span", "", sub));
  t.type = "button";
  t.title = title;
  const row = h("div", `cc-row cc-line${lead ? "" : " bare"}`, lead, t, h("span", "r", ...right));
  row.addEventListener("click", open);
  return row;
}
