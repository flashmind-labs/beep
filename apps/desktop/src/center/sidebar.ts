import { projectName } from "../project/projects-state.ts";
import { renameProject } from "../project/rename.tsx";
import { WINDOWS, onPhone } from "../ui/platform.ts";
// The sidebar: its groups (pinned, views, projects, machines) and the
// sheets that add a machine or a project.
import { onClosing, visibleRunning } from "../agents/closing.ts";
import { frontLayout, layoutChanged } from "../ui/skin.ts";
import { plural, reason } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { openPalette } from "./palette.ts";
import { pane } from "../bar/pane.ts";
import type { PaneNow } from "../bar/core.ts";
import { openAddMachine as openAddMachineSheet } from "../machines/addmachine.ts";
import { openImportProject } from "../addproject/importproject.ts";
import { openAddProject, importFound, rescanAll } from "../addproject/addproject.ts";
import { openAddFolder } from "../addproject/addfolder.ts";
import { openNewProject } from "../addproject/newproject.ts";
import { foldKey, mergeProjects, parseFolded, sideProjects, splitOnTop, removeProjectSheet, REMOVE_PROJECT, type FoundRow, type GroupHeading, type OrgHeading, type Placed, type Project } from "../project/projects-state.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { openInEditor } from "../project/editors.ts";
import { whereRows, openOnLabel } from "../project/projects-state.ts";
import { openUrl } from "./openitems.ts";
import { NEW_WINDOW, opensNewWindow } from "../project/projectwindow.ts";
import { ALL } from "../terminals/terminals.ts";
import { state, type GroupSlot, type SidebarGroup, groups } from "./state.ts";
import { stuck, draw } from "./home.ts";
import { allTerms } from "../terminals/view.ts";
import { currentLocation, go, drawTop } from "./views.ts";
import { groupMenu } from "../project/groups.ts";
import { fsync, redrawProjects, redrawProjectsPage } from "../project/sections.ts";
import { conflictList } from "../project/filesync.ts";
import { prefetchOnHover, sessionsDue } from "../project/loads.ts";
import type { ProjectWindowProps } from "../project/projectwindow.ts";
import { drawProjects } from "../project/page.ts";
import { refresh } from "./refresh.ts";
import { IC, Icon } from "../ui/icons.tsx";
import type { MachineCard } from "../machines/machine-state.ts";
import type { HomeView } from "./home-state.ts";
import { createElement, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { placeWindowParts } from "./window-root.ts";
import { windowHost } from "./window-shell.tsx";
import { SidebarShell, SidebarItem, SidebarGroup as SidebarGroupView, type SideFooter, type SideLink } from "./sidebar-shell.tsx";
import { SidebarProjectRow } from "./sidebar-rows.tsx";
import { single } from "../ui/active.ts";

export function registerSidebarGroup<T>(name: string, group: SidebarGroup<T>) {
  const slot: GroupSlot<T> = { title: "", ...group, items: null, loading: false, again: false };
  groups.set(name, slot);
  reloadGroup(name);
}

/// Loads a group's items again. Never two loads at once: one asked for
/// during a load runs once more after it.
/// The sidebar redraws once the group is in, unless `redraw` is false and the
/// caller draws after loading several. A load asked for during another
/// returns at once, so the one that follows always draws.
export async function reloadGroup(name: string, redraw = true) {
  const g = groups.get(name);
  if (!g) return;
  if (g.loading) { g.again = true; return; }
  g.loading = true;

  try {
    g.items = await g.load();
  } catch (e) {
    console.warn(`loading the ${name} group:`, e);
  } finally {
    g.loading = false;
  }

  if (redraw) drawSide();
  if (g.again) { g.again = false; reloadGroup(name); }
}

/// What the pane shows, for the sidebar's highlight; null off the pane.
export function paneOn(): { view: PaneNow["view"] | null } {
  return currentLocation()?.view === "pane" ? pane.current() : { view: null };
}

export function drawSide() {
  const home = state.home, location = currentLocation()?.view;
  const links: SideLink[] = [
    { svg: IC.find, label: "Search", count: 0, className: "cc-search", open: openPalette, shortcut: WINDOWS ? "Ctrl+K" : "⌘K" },
    { svg: IC.home, label: "Home", count: home?.needs_you.length ?? 0, current: location === "home", hot: true, open: () => go("home") },
  ];
  // The board has every session; the list keeps its own page of them.
  if (home?.running && frontLayout("home") === "list") links.push({ svg: IC.run, label: "Running", count: visibleRunning(home.running).length, current: paneOn().view === "agents", open: () => go("pane", "agents") });
  const terms = allTerms.list(ALL);
  if (home?.machines && terms) links.push({ svg: IC.term, label: "Terminals", count: terms.length, current: location === "terminals", open: () => go("terminals") });
  const side: ReactNode[] = [];

  for (const [name, g] of [...groups].sort((a, b) => a[1].order - b[1].order)) {
    if (g.items != null) side.push(createElement(SidebarGroupView, { key: name, name, group: g }));
  }

  const host = windowHost("side");
  placeWindowParts({ sidebar: createElement(SidebarShell, { host, phone: onPhone(), links, groups: side, footer: footer(home?.machines), addMachine: openAddMachine }) });
  if (currentLocation()?.view === "pane") drawTop();
}

function footer(machines: MachineCard[] | null | undefined): SideFooter {
  const action = machines ? "add" : null;
  if (!state.home) return { dot: stuck() ? "bad" : "off", text: stuck() ? "Not connected" : "Connecting to your machines…", action };
  if (!machines) return { dot: null, text: "Machines unavailable", action };
  if (!machines.length) return { dot: null, text: "No machines linked", action };
  const dialing = machines.filter(m => m.connecting).map(m => m.name);
  if (dialing.length) return { dot: "dial", text: `Connecting to ${dialing[0]}${dialing.length > 1 ? ` and ${dialing.length - 1} more` : ""}…`, title: dialing.join(", "), action };
  return { dot: "on", count: machines.filter(m => m.online).length, text: ` of ${plural(machines.length, "machine", "machines")} online`, action };
}

/// The projects pinned to the top, the rows of the Pinned group.
function pinnedProjects(): Placed<Project>[] {
  const rows = projectRows();
  if (!rows) return [];
  return splitOnTop(rows, state.top).onTop;
}

/// Pins project `name` to the top of the sidebar, or unpins it. Not the
/// file sync pin, which keeps a project on the machines it is on.
export async function setProjectOnTop(name: string, pinned: boolean) {
  const before = state.top;
  state.top = pinned ? [...before.filter(n => n !== name), name] : before.filter(n => n !== name);
  redrawProjectPins();

  try {
    await invoke("pin_project_to_top", { project: name, pinned });
  } catch (e) {
    console.warn("pinning a project:", e);
    state.top = before;
    redrawProjectPins();
  }
}

function redrawProjectPins() {
  reloadGroup("pinned");
  reloadGroup("projects");
  redrawProjectsPage();
}

/// Opens project `name` in a window of its own, or brings up the one it
/// already has.
export function openProjectWindow(name: string) {
  invoke("center_open_project_window", { project: name }).catch(e => console.warn("opening a project window:", e));
}

/// What lets project `name` open in a window of its own: ⌘-click or middle-click on the
/// element given `clicks`, which also reads the project ahead on hover, or "Open in New Window"
/// from a right-click on the one given `menu`. The capture runs before the click opens it here.
export function projectWindowProps(name: string): ProjectWindowProps {
  const open = (e: ReactMouseEvent) => {
    if (!opensNewWindow(e)) return;
    e.preventDefault();
    e.stopPropagation();
    openProjectWindow(name);
  };
  return {
    clicks: { onClickCapture: open, onAuxClick: open, ...prefetchOnHover(name) },
    menu: { onContextMenu: (e: ReactMouseEvent) => openProjectMenu(e.nativeEvent, name) },
  };
}

/// The right-click menu of project `name`: a window of its own, and
/// removing it once it is imported (one from running agents only has
/// nothing to remove).
function openProjectMenu(e: MouseEvent, name: string) {
  const p = projectRows()?.find(x => x.name === name);
  const on = state.top.includes(name);
  const items: MenuItem[] = [...(onPhone() ? [] : [{ label: NEW_WINDOW, on: () => openProjectWindow(name) }]), { label: on ? "Unpin" : "Pin to top", on: () => setProjectOnTop(name, !on) }];
  const folder = p && whereRows(p, state.where.get(name) || null, state.mac?.id || null).find(r => r.mac && r.folder)?.folder;
  if (folder && !onPhone()) items.push({ label: "Open in editor", on: () => void openInEditor(folder) });
  const label = p?.url ? openOnLabel(p.url) : null;
  if (p?.url && label) items.push({ label, on: () => openUrl(p.url!) });
  if (p?.imported) items.push({ label: "Rename…", on: () => renameProject(p) });
  if (p?.imported) items.push({ label: REMOVE_PROJECT, warn: true, on: () => removeProject(name) });
  openMenu(e, name, items);
}

/// Asks, then removes project `name` from Beep. Its files stay on every
/// machine.
export function removeProject(name: string) {
  confirmSheet({
    ...removeProjectSheet(name),
    run: async () => {
      await invoke("remove_project", { project: name });
      state.top = state.top.filter(n => n !== name);
      if (state.home?.projects) state.home = { ...state.home, projects: state.home.projects.filter(p => p.name !== name) };
      state.where.delete(name);

      if (currentLocation()?.view === "projects" && state.picked === name) openProjects();
      else { draw(); redrawProjectsPage(); }

      loadFound();
      refresh();
    },
  });
}

function projectReact(project: Placed<Project>, label: string, pinned: boolean, indented: boolean) {
  return createElement(SidebarProjectRow, { project, label, pinned, indented, conflicts: conflictList(fsync.detail(project.name)).length,
    current: currentLocation()?.view === "projects" && state.picked === project.name,
    open: () => openProjects(project.name), pin: () => { void setProjectOnTop(project.name, !pinned); },
    windowProps: projectWindowProps(project.name) });
}
registerSidebarGroup<Placed<Project>>("pinned", {
  order: 2,
  title: "Pinned",
  load: async () => pinnedProjects(),
  render: p => projectReact(p, projectName(p), true, false),
  key: p => `project:${p.name}`,
});

registerSidebarGroup("views", {
  order: 6,
  title: "Beep",
  load: async () => ["settings" as const],
  key: v => v,
  render: v => createElement(SidebarItem, { svg: IC.gear, label: "Settings", current: false, shortcut: WINDOWS ? "Ctrl+," : "⌘,", open: () => go("pane", v) }),
});

/// Imported projects merged with the folders agents run in.
export function projectRows() {
  const home = state.home;
  if (!home || (!home.running && !home.projects)) return null;
  // Home, the sidebar and the pages all ask on each draw; the answer only
  // changes with the lists it is made from.
  if (merged?.projects !== home.projects || merged.running !== home.running) {
    merged = { projects: home.projects, running: home.running, rows: mergeProjects(home.projects, home.running) };
  }
  return merged.rows;
}
let merged: { projects: HomeView["projects"]; running: HomeView["running"]; rows: Project[] } | undefined;

/// The project headings folded in the sidebar, kept between launches. Only
/// the main window has a sidebar.
const FOLDED_KEY = "beep.sideFolded";
let folded = new Set<string>();
try { folded = parseFolded(localStorage.getItem(FOLDED_KEY)); } catch {}

/// Makes `head` fold the projects under it. Folded, it lights up while it
/// holds the open project.
function toggleFold(p: OrgHeading | GroupHeading) {
  const key = foldKey(p);
  if (!folded.delete(key)) folded.add(key);
  try { localStorage.setItem(FOLDED_KEY, JSON.stringify([...folded])); } catch { /* kept for this launch only */ }
  reloadGroup("projects");
}

function foldItem(p: OrgHeading | GroupHeading): MenuItem {
  return { label: p.folded ? "Unfold" : "Fold", on: () => toggleFold(p) };
}

/// Opens the Add a machine sheet; Home refreshes once it closes.
export function openAddMachine() {
  openAddMachineSheet({ onClose: refresh });
}

/// Opens the Projects page, filtered to `name` when given.
export function openProjects(name?: string | null) {
  return go("projects", { project: name || null });
}

/// Opens a project once it is in: Home and the found folders load again.
function landed(p: { name: string }) {
  state.picked = p.name;
  sessionsDue.add(p.name);
  state.where.delete(p.name);
  loadFound();
  refresh();
  openProjects(p.name);
}

/// Opens the Import a project sheet; the project is selected once it is in.
export function importSheet(repo: string | null = null, machine: string | null = null) {
  openImportProject({
    machines: state.home?.machines || [],
    macId: state.mac?.id || null,
    repo,
    machine,
    onDone: landed,
  });
}

/// Opens the Add a project sheet: found folders, a folder, a clone, or a new project.
export function addProject() {
  openAddProject({
    machines: (state.home?.machines || []).filter(m => m.can_host !== false),
    onImported: landed,
    onChoose: () => openAddFolder({
      machines: state.home?.machines || [],
      macId: state.mac?.id || null,
      onDone: landed,
      onBack: addProject,
    }),
    onClone: repo => importSheet(repo ?? null),
    onNew: () => openNewProject({
      machines: state.home?.machines || [],
      onDone: landed,
      onBack: addProject,
      onClone: repo => importSheet(repo),
    }),
  });
}

/// The checkouts the machines named last, for the Projects page, then each
/// machine's look again as it answers; it redraws each time.
export const loadFound = single(async () => {
  try {
    state.found = await invoke<FoundRow[]>("found_projects");
  } catch (e) {
    console.warn("found projects:", e);
    state.found = state.found || [];
  }

  if (!state.picked) redrawProjects();
  await rescanAll(() => {}, (_, after) => {
    if (!after.rows) return;
    state.found = after.rows;
    if (!state.picked) redrawProjects();
  });
});

/// Imports a found row from the Projects page and opens it.
export async function importFromPage(row: FoundRow) {
  if (state.importing) return;
  state.importing = row.name;
  state.foundError = null;
  drawProjects();

  try {
    const project = await importFound(row);
    state.importing = null;
    landed(project);
  } catch (e) {
    state.importing = null;
    state.foundError = { name: row.name, message: reason(e) };
    redrawProjects();
  }
}

// Imported projects and the folders agents run in. It shows once Home has
// loaded, empty or not.
registerSidebarGroup("projects", {
  order: 5,
  title: "Projects",
  onTitle: () => openProjects(),
  action: { label: "Add a project", run: addProject },
  empty: "No projects yet",
  load: async () => {
    const rows = projectRows();
    // Those pinned to the top show under Pinned instead.
    return rows ? sideProjects(splitOnTop(rows, state.top).rest, state.groups, folded) : null;
  },
  key: p => p.group ? `group:${p.group.id}` : p.org !== undefined ? `org:${p.org}` : `project:${p.name}`,
  render: p => {
    if (!p.group && p.org === undefined) return projectReact(p, p.repo ?? projectName(p), false, !!p.repo);
    const title = p.group ? p.group.name : `${p.org}/`;
    const label = p.group ? createElement("span", { className: "sidebar-icon", style: { display: "contents" } }, createElement(Icon, { name: "folder" })) : null;
    return createElement("button", { type: "button", className: `cc-org${p.group ? " gp" : ""}`,
      title: p.group ? `${title}\n${p.group.paths.join(", ")} on every machine` : title, "aria-label": title,
      "aria-expanded": !p.folded, "aria-current": currentLocation()?.view === "projects" && state.picked && p.folded?.includes(state.picked) ? "true" : undefined,
      onClick: () => toggleFold(p), onContextMenu: (event: React.MouseEvent) => p.group ? groupMenu(event.nativeEvent, p.group, [foldItem(p)]) : openMenu(event.nativeEvent, p.org ?? "", [foldItem(p)]) },
      createElement("span", { className: `twist${p.folded ? "" : " open"}` }, createElement(Icon, { name: "twist" })), label, createElement("span", { className: "tt" }, title));
  },

});

onClosing(() => drawSide());
layoutChanged.add(() => drawSide());
