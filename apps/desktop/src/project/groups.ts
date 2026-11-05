// Project groups in the sidebar: heading, menu, rename and remove.
import { plural } from "../ui/format.ts";
import { svgEl as icon } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { groupMatches, type ProjectGroup } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { openGroupSheet } from "./groupsheet.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { state } from "../center/state.ts";
import { dot, rowButton } from "../ui/pieces.ts";
import { useThisMac, machineMenu } from "../machines/table.ts";
import { shown } from "../center/views.ts";
import { registerSidebarGroup, reloadGroup, sideItem, projectRows, plusButton, emptyRow, openAddMachine, openProjects } from "../center/sidebar.ts";
import { redrawProjects, redrawProjectsPage } from "./sections.ts";
import { jump } from "../center/home.ts";
import { IC } from "../ui/icons.ts";
import { type MachineCard, type MacRow, thisMacRow } from "../machines/machine-state.ts";

/// A group's heading in the sidebar; right-click for its menu.
export function groupHeading(g: ProjectGroup, extra: MenuItem[] = []) {
  const head = h("button", "cc-org gp", icon(IC.folder), g.name);
  head.title = `${g.paths.join(", ")} on every machine`;
  head.addEventListener("contextmenu", e => groupMenu(e, g, extra));
  return head;
}

/// `extra` goes first, for what only the place that shows the group can do.
export function groupMenu(e: MouseEvent, g: ProjectGroup, extra: MenuItem[] = []) {
  openMenu(e, g.name, [
    ...extra,
    { label: "Rename…", on: () => startRename(g) },
    { label: "Folders…", on: () => editGroup(g) },
    { label: "Remove group…", warn: true, on: () => removeGroup(g) },
  ]);
}

/// Opens the group sheet: a new group, or `g`'s folders.
export function editGroup(g: ProjectGroup | null = null) {
  openGroupSheet({
    group: g,
    matches: paths => groupMatches(projectRows(), paths),
    onSaved: saved => {
      const list = state.groups || [];
      state.groups = list.some(x => x.id === saved.id) ? list.map(x => (x.id === saved.id ? saved : x)) : [...list, saved];
      redrawGroups();
    },
  });
}

/// Edits `g`'s name in place on the Projects overview.
export function startRename(g: ProjectGroup) {
  state.renaming = { id: g.id, draft: g.name };
  if (shown !== "projects" || state.picked) openProjects();
  else redrawProjects();
}

export async function renameGroup(g: ProjectGroup, name: string) {
  state.renaming = null;
  const to = name.trim();
  if (!to || to === g.name) return redrawProjects();
  const before = state.groups;
  state.groups = (before || []).map(x => (x.id === g.id ? { ...x, name: to } : x));
  redrawGroups();
  try {
    await invoke("project_group_save", { id: g.id, name: to, paths: g.paths });
  } catch (e) {
    console.warn("renaming a group:", e);
    state.groups = before;
    redrawGroups();
  }
}

/// Asks, then removes group `g`. Its projects and folders stay.
export function removeGroup(g: ProjectGroup) {
  confirmSheet({
    title: `Remove ${g.name}?`,
    text: "The group goes. Its projects stay.",
    action: "Remove",
    run: async () => {
      await invoke("project_group_delete", { id: g.id });
      state.groups = (state.groups || []).filter(x => x.id !== g.id);
      redrawGroups();
    },
  });
}

function redrawGroups() {
  reloadGroup("projects");
  redrawProjectsPage();
}

// The machines Home lists, each with its online dot. It shows once Home has
// loaded, empty or not, unless the server is too old to list machines.
registerSidebarGroup<MachineCard | { thisMac: MacRow }>("machines", {
  order: 5.5,
  title: "Machines",
  onTitle: () => jump("sec-machines"),
  action: () => plusButton("Add a machine", openAddMachine),
  empty: () => emptyRow("No machines yet"),
  load: async () => {
    const machines = state.home?.machines;
    if (!machines) return null;
    const mac = thisMacRow(state.mac, machines);
    return mac ? [...machines, { thisMac: mac }] : machines;
  },
  row(m) {
    if ("thisMac" in m) return thisMacItem(m.thisMac);
    const b = sideItem(null, m.name, m.agents || 0);
    b.prepend(dot(m));
    b.title = `${m.name}: ${m.online ? (m.agents ? plural(m.agents, "agent", "agents") + " running" : "online") : "offline"}`;
    machineMenu(m, b);
    return b;
  },
  onPick: m => jump("thisMac" in m ? "m-this-mac" : `m-${m.id}`),
});

/// The sidebar's This Mac: its state, or Add while it is off.
function thisMacItem(mac: MacRow) {
  const title = `This Mac: ${mac.title}`;
  if (mac.action) {
    // A row with a button in it, never a button inside a button.
    const row = h("div", "cc-si", h("span", "ddot off"), h("span", "tt", mac.name),
      rowButton(mac.action, "quiet small mac-add", useThisMac));
    row.title = title;
    return row;
  }
  const b = sideItem(null, mac.name, 0);
  b.prepend(h("span", "ddot off"));
  b.append(h("span", `n mac-state${mac.bad ? " bad" : ""}`, mac.state));
  b.title = title;
  return b;
}
