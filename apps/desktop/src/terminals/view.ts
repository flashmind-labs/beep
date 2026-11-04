// The command center's Terminals page, and the terminals Home lists.
import { ago } from "../ui/format.ts";
import { dbtn as button } from "../ui/parts.ts";
import { invoke, Channel } from "../ui/tauri.ts";
import { el as h } from "../project/projects-state.ts";
import { ALL, byMachine, makeTerminals, terminalNote, terminalWhere, terminalsCount, type TerminalRow } from "./terminals.ts";
import { makeScreen } from "./termview.ts";
import { state, views } from "../center/state.ts";
import { line } from "../center/pieces.ts";
import { draw } from "../center/home.ts";
import { shown, registerView, go, drawTop } from "../center/views.ts";
import { drawSide } from "../center/sidebar.ts";
import { refresh } from "../center/refresh.ts";

export const allTerms = makeTerminals({
  invoke, Channel, makeScreen, h, button, ago,
  redraw: () => {
    if (shown === "terminals") { drawTerminals(); drawTop(); }
    else if (shown === "home") draw();
    drawSide();
  },
});

/// The machines terminals are listed under, as Home has them.
export const termMachines = () => (state.home?.machines || []).map(m => ({ id: m.id, name: m.name, online: m.online }));

/// Opens the Terminals page with terminal `id`, or the New terminal form on
/// machine `newOn`.
export function openTerminals(to: { id: string } | { newOn: string | null } | null) {
  go("terminals");
  if (to && "id" in to) allTerms.show(to.id);
  else if (to) allTerms.newOn(to.newOn);
}

/// A terminal indented under its machine's row; clicking opens it.
export function terminalRow(t: TerminalRow, online: boolean) {
  const row = h("button", "cc-row tl-row", h("span", `ddot${t.state === "running" && online ? "" : " off"}`),
    h("span", "t", h("b", "", t.name), h("span", "", terminalWhere(t))), h("span", "r", terminalNote(t, ago)));
  row.type = "button";
  row.title = `${t.machine_name}: ${t.command} in ${t.cwd}`;
  row.addEventListener("click", () => openTerminals({ id: t.id }));
  return row;
}

/// A terminal in an Overview box: its name, machine and folder, how it stands.
export function terminalLine(t: TerminalRow, open: () => void) {
  const row = line(h("span", `ddot${t.state === "running" ? "" : " off"}`), t.name, `${t.machine_name} · ${terminalWhere(t)}`, [terminalNote(t, ago)], open);
  row.title = `${t.machine_name}: ${t.command} in ${t.cwd}`;
  return row;
}


/// The Terminals page into its view, keeping the list's scroll.
function drawTerminals() {
  const el = views.get("terminals")?.el;
  if (!el) return;
  const view = h("div", "cc-view tl-view", ...allTerms.machinesPanel(termMachines()));
  const old = el.querySelector(":scope > .cc-view");
  const top = old?.querySelector(".tl-list")?.scrollTop ?? 0;
  if (old) old.replaceWith(view); else el.append(view);
  const list = view.querySelector(".tl-list");
  if (list) list.scrollTop = top;
}

registerView("terminals", {
  title() {
    const list = allTerms.list(ALL);
    const sub = list ? terminalsCount(byMachine(termMachines(), list)) : "";
    const add = button("New terminal", "pri small", () => allTerms.newOn(termMachines().find(m => m.online)?.id || null));
    return { title: "Terminals", sub, tools: list === null ? [] : [add] };
  },
  mount() {
    drawTerminals();
    allTerms.load(ALL);
    refresh();
  },
  unmount() {
    // Detached, not stopped: they keep running on their machines.
    allTerms.stop();
  },
});
