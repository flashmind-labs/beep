// Small pieces Home, the sidebar and the Projects page draw with.
import { plural } from "./format.ts";
import { dbtn as button, svgEl as icon } from "./parts.ts";
import { el as h, type Kid } from "./dom.ts";
import { IC, PROJECT_COLORS } from "./icons.ts";

/// Why a call failed, cut short; a conflict means another answer won.
export function failure(e: unknown) {
  const text = String(e);
  return conflict(e) ? "Already answered" : text.length > 90 ? text.slice(0, 90) + "..." : text;
}
export function conflict(e: unknown) { return /^conflict/i.test(String(e)); }

// ---------- Pieces ----------

/// Green online, yellow with agents running there, grey offline.
export function dot(m: { online: boolean; agents: number }) {
  const el = h("span", `ddot${!m.online ? " off" : m.agents ? " busy" : ""}`);
  el.title = !m.online ? "Offline" : m.agents ? `Online · ${plural(m.agents, "agent", "agents")} running` : "Online";
  return el;
}

/// A project's colour dot; `i` is its place in the project list, none
/// leaves it plain.
export function projectDot(i?: number) {
  const d = h("span", "cc-pdot");
  if (i !== undefined) d.style.background = PROJECT_COLORS[i % PROJECT_COLORS.length];
  return d;
}

/// A Home section: its title and count, an optional button on the right,
/// then its body. `notes` follow the count in the title.
export function section(id: string, title: Kid, count: number | string, action: Kid, body: Node, ...notes: Kid[]) {
  const el = h("div");
  el.id = id;
  const head = h("h4", "", title);
  if (count) head.append(h("span", "n", String(count)));
  for (const n of notes) if (n) head.append(n);
  el.append(h("div", "cc-sec", head, action), body);
  return el;
}
export function avatar(cls: string, ini: Kid) { return h("span", `cc-ag ${cls}`, ini); }
export function tag(cls: string, ...kids: Kid[]) { return h("span", `chip${cls ? " " + cls : ""}`, ...kids); }

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

/// Placeholder rows of these widths, in percent, while a list loads.
export function skeleton(widths: number[]) {
  return widths.map(w => {
    const row = h("div", "cc-skel");
    row.style.width = `${w}%`;
    return row;
  });
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
