import { SidebarMachine } from "../machines/sidebar-machine-view.tsx";
// Project groups in the sidebar: heading, menu, rename and remove.
import { MACHINES } from "../machines/machine-store.ts";
import { invoke, useBuildCapabilities } from "../ui/tauri.ts";
import { groupMatches, type ProjectGroup } from "./projects-state.ts";
import { openGroupSheet } from "./groupsheet.tsx";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.tsx";
import { state } from "../center/state.ts";
import { currentLocation } from "../center/views.tsx";
import { registerSidebarGroup, drawSide, projectRows, openAddMachine, openProjects } from "../center/sidebar.tsx";
import { redrawProjects, redrawProjectsPage } from "./sections.ts";
import { jump } from "../center/home.tsx";
import { type MachineCard, type MacRow, thisMacRow } from "../machines/machine-state.ts";

/// `extra` goes first, for what only the place that shows the group can do.
export function groupMenu(e: MouseEvent, g: ProjectGroup, extra: MenuItem[] = []) {
  openMenu(e, g.name, [
    ...extra,
    { label: "Rename", on: () => startRename(g) },
    { label: "Folders", on: () => editGroup(g) },
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
  if (currentLocation()?.view !== "projects" || state.picked) openProjects();
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
  drawSide();
  redrawProjectsPage();
}

// The machines Home lists, each with its online dot. It shows once Home has
// loaded, empty or not.
registerSidebarGroup<MachineCard | { thisMac: MacRow }>("machines", {
  order: 5.5,
  title: "Machines",
  onTitle: () => jump("sec-machines"),
  action: { label: "Add a machine", run: openAddMachine },
  empty: "No machines yet",
  load: () => {
    const machines = state.home?.machines;
    if (!machines) return null;
    const mac = useBuildCapabilities.getState().local_host ? thisMacRow(MACHINES.mac, machines) : null;
    return mac ? [...machines, { thisMac: mac }] : machines;
  },
  key: m => "thisMac" in m ? "this-mac" : m.id,
  render: m => <SidebarMachine machine={m} />,
});

// This computer's row waits for the build to say it can host.
useBuildCapabilities.subscribe(() => drawSide());

