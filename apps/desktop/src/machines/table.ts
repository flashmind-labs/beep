// The command center's machines table: one row per machine, its terminals
// under it, and the menu that removes one.
import { since } from "../ui/format.ts";
import { dbtn as button, emptyPanel, note } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { machineCaps, machineDisk, machineMemory, machineNow, machineOs, machineSync, thisMacRow, type MacControl, type MachineCard, type MacRow, type MachineSyncing } from "../center/home-state.ts";
import { el as h } from "../ui/dom.ts";
import { confirmSheet, openMenu } from "../ui/menu.ts";
import { ALL, byMachine } from "../terminals/terminals.ts";
import { OFFLINE_NOTE_MS, state } from "../center/state.ts";
import { dot, section, plusLabel, rowButton } from "../center/pieces.ts";
import { draw } from "../center/home.ts";
import { allTerms, termMachines, openTerminals, terminalRow } from "../terminals/view.ts";
import { reloadGroup, openAddMachine } from "../center/sidebar.ts";
import { refresh } from "../center/refresh.ts";

export const addMachineButton = (cls: string) => plusLabel("Add a machine", cls, openAddMachine);

/// One machine per row: name and OS, agents, free disk, memory in use, what
/// it can run, what it is doing now and New terminal, with its terminals
/// indented under it. Right-click a machine to remove it.
function machineTable(machines: MachineCard[]) {
  const table = h("div", "cc-mt", ...["Machine", "Agents", "Disk", "Memory", "Can run", "Now", ""].map(t => h("span", "th", t)));
  const terms = byMachine(termMachines(), allTerms.list(ALL) || []);
  for (const m of machines) {
    const off = m.online ? "" : " off";
    const name = h("span", `nm${off}`, h("b", "", dot(m), h("span", "", m.name)), h("span", "os", machineOs(m)));
    name.id = `m-${m.id}`;
    name.title = m.name;
    const caps = machineCaps(m);
    const can = h("span", `caps${off}`, caps.join(" · ") || "–");
    if (caps.length) can.title = [...caps, ...m.gpus.map(g => g.model)].join(", ");
    const sync = m.online ? machineSync(m.id, state.sync) : null;
    const now = sync ? syncCell(sync) : h("span", `num${off}`, machineNow(m));
    if (!m.online) now.title = `Seen ${since(m.seen_at)}`;
    const d = machineDisk(m), mem = machineMemory(m);
    const disk = h("span", `num${off}${d.low ? " low" : ""}`, d.text);
    if (d.title) disk.title = d.title;
    const memory = h("span", `num${off}`, mem.text);
    if (mem.title) memory.title = mem.title;
    const agents = h("span", `num${off}`, m.online && m.agents ? String(m.agents) : "–");
    const add = button("New terminal", "small", () => openTerminals({ newOn: m.id }));
    add.disabled = !m.online;
    const act = h("span", "act", add);
    table.append(name, agents, disk, memory, can, now, act);
    machineMenu(m, name, agents, disk, memory, can, now, act);
    for (const t of terms.find(x => x.id === m.id)?.terms || []) table.append(terminalRow(t, m.online));
  }
  const mac = thisMacRow(state.mac, machines);
  if (mac) table.append(...thisMacCells(mac));
  return table;
}

/// The Now cell of a machine fetching a synced project: a bar and how far.
function syncCell(sync: MachineSyncing) {
  const fill = sync.pct == null ? null : h("i");
  if (fill) fill.style.width = `${sync.pct}%`;
  const cell = h("span", "num sync", h("span", "bar", fill), h("small", "", sync.text));
  cell.title = [`${sync.text.split(",")[0]} ${sync.project}`, sync.files].filter(Boolean).join(": ");
  return cell;
}

/// This Mac's row while the server does not list it: its state across the
/// middle columns and, while it is off, Add under Now.
function thisMacCells(mac: MacRow) {
  const name = h("span", "nm off", h("b", "", h("span", "ddot off"), h("span", "", mac.name)), h("span", "os", mac.sub));
  name.id = "m-this-mac";
  const st = h("span", `caps mac-state${mac.bad ? " bad" : ""}`, mac.state);
  st.title = mac.title;
  const now = h("span", "num", mac.action ? rowButton(mac.action, "pri small", useThisMac) : "–");
  return [name, st, now, h("span")];
}

/// Turns on Let Beep use this Mac, so it joins as a machine.
export async function useThisMac() {
  try {
    state.mac = await invoke<MacControl>("set_mac_control", { enabled: true });
  } catch (e) {
    console.warn("turning on this Mac:", e);
  }
  draw();
  reloadGroup("machines");
  // The link takes a moment; Home looks again soon after.
  for (const ms of [1500, 5000]) setTimeout(refresh, ms);
}

/// A machine the window can offer to remove: any but this Mac, which
/// unlinks from Settings.
interface Removable {
  id: string;
  name: string;
  online?: boolean;
}

/// Right-clicking `els` opens the menu for machine `m`, offline or not; none
/// for this Mac.
export function machineMenu(m: Removable, ...els: Element[]) {
  for (const el of els) el.addEventListener("contextmenu", e => {
    if (!m.id || m.id === state.mac?.id) return;
    openMenu(e as MouseEvent, m.name, [{ label: "Remove…", warn: true, on: () => removeMachine(m) }]);
  });
}

/// Asks, then unlinks `m` from the account.
function removeMachine(m: Removable) {
  confirmSheet({
    title: `Remove ${m.name}?`,
    text: m.online === false ? "It is unlinked from your account." : "It is unlinked and agents running there stop.",
    note: "Its files stay on the machine.",
    action: "Remove",
    run: async () => {
      await invoke("remove_machine", { id: m.id });
      if (state.home) state.home = { ...state.home, machines: state.home.machines?.filter(x => x.id !== m.id) ?? null };
      state.where.clear();
      draw();
      reloadGroup("machines");
      refresh();
    },
  });
}

export function machinesSection(machines: MachineCard[]) {
  if (!machines.length && !thisMacRow(state.mac, machines)) {
    return section("sec-machines", "Machines", "", addMachineButton("small"),
      emptyPanel(h("b", "", "No machines linked yet"), addMachineButton("pri")));
  }
  const online = machines.filter(m => m.online).length;
  const count = machines.length ? `${online} of ${machines.length} online` : "";
  return section("sec-machines", "Machines", count, addMachineButton("small"), machineTable(machines));
}

export function offlineNote(machines: MachineCard[]) {
  const gone = machines.filter(m => !m.online && Date.now() - m.seen_at < OFFLINE_NOTE_MS);
  if (!gone.length) return null;
  const names = gone.map(m => m.name).join(", ");
  return note("bad", h("b", "", `${names} ${gone.length === 1 ? "is" : "are"} offline.`), " Their sessions left Running now. They come back when beep-node runs there again.");
}
