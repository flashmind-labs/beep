// Project groups in the sidebar: heading, menu, rename and remove.
import { plural } from "../ui/format.ts";
import { svgEl as icon } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { thisMacRow, type MachineCard, type MacRow } from "../center/home-state.ts";
import { groupMatches, el as h, type ProjectGroup } from "./projects-state.ts";
import { openGroupSheet } from "./groupsheet.ts";
import { openMenu } from "../ui/menu.ts";
import { IC, state } from "../center/state.ts";
import { dot, rowButton } from "../center/pieces.ts";
import { useThisMac, machineMenu } from "../machines/table.ts";
import { shown } from "../center/views.ts";
import { registerSidebarGroup, reloadGroup, sideItem, projectRows, plusButton, emptyRow, openAddMachine, openProjects } from "../center/sidebar.ts";
import { redrawProjects, redrawProjectsPage } from "./sections.ts";
import { jump } from "../center/home.ts";

export function groupHeading(g: ProjectGroup) {
  const head = h("div", "cc-org gp", icon(IC.folder), g.name);
  head.title = `${g.paths.join(", ")} on every machine`;
  head.addEventListener("contextmenu", e => groupMenu(e, g));
  return head;
}

export function groupMenu(e: MouseEvent, g: ProjectGroup) {
  openMenu(e, g.name, [
    { label: "Rename…", on: () => startRename(g) },
    { label: "Folders…", on: () => editGroup(g) },
    { label: "Remove group", warn: true, on: () => removeGroup(g) },
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

export async function removeGroup(g: ProjectGroup) {
  const before = state.groups;
  state.groups = (before || []).filter(x => x.id !== g.id);
  redrawGroups();
  try {
    await invoke("project_group_delete", { id: g.id });
  } catch (e) {
    console.warn("removing a group:", e);
    state.groups = before;
    redrawGroups();
  }
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
