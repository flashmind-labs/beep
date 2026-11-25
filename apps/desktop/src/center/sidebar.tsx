import { projectName } from "../project/projects-state.ts";
import { MACHINES } from "../machines/machine-store.ts";
import { WINDOWS, isMobile, onPhone } from "../ui/platform.ts";
// The sidebar: its groups (pinned, views, projects, machines) and the
// sheets that add a machine or a project.
import { onClosing } from "../agents/closing.ts";
import { plural, reason } from "../ui/format.ts";
import { invoke, openUrl } from "../ui/tauri.ts";
import { openPalette } from "./palette.tsx";
import { pane } from "../bar/pane.ts";
import type { PaneNow } from "../bar/core.ts";
import { foldKey, mergeProjects, parseFolded, sideProjects, splitOnTop, removeProjectSheet, REMOVE_PROJECT, type FoundRow, type GroupHeading, type OrgHeading, type Placed, type Project } from "../project/projects-state.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.tsx";
import { openInEditor } from "../project/editors.ts";
import { whereRows, openOnLabel } from "../project/projects-state.ts";
import { NEW_WINDOW, opensNewWindow } from "../project/projectwindow.ts";
import { ALL } from "../terminals/terminal-list.ts";
import { state, pages, whereOf, type SidebarGroup, groups } from "./state.ts";
import { stuck, draw } from "./home.tsx";
import { allTerms } from "../terminals/view.tsx";
import { currentLocation, go, drawTop, aloneProject, windowAlone } from "./views.tsx";
import { fsync, redrawProjects, redrawProjectsPage, syncProjectFollow } from "../project/sections.ts";
import { conflictList } from "../project/filesync.ts";
import { prefetchOnHover, projectRail, railSessions, sessionsDue } from "../project/loads.tsx";
import type { Alone, ProjectWindowProps } from "../project/projectwindow.ts";
import type { AgentRow } from "../agents/agents.ts";
import { forgetProject } from "./refresh.ts";
import { IC, Icon } from "../ui/icons.tsx";
import type { MachineCard } from "../machines/machine-state.ts";
import type { HomeView } from "./home-state.ts";
import { type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { placeWindowParts } from "./window-root.tsx";
import { type Rail, railProject, setRail, shell } from "./window-shell.tsx";
import { chatRail, chatsAsking } from "../agents/threads-state.ts";
import { sessions, sessionsRead } from "../agents/sessions.ts";
import { sessionVisible } from "../agents/closing.ts";
import { ChatRail } from "../agents/assistant-page.tsx";
import { SidebarShell, SidebarItem, SidebarGroup as SidebarGroupView, type SideFooter, type SideLink } from "./sidebar-shell.tsx";
import { SidebarProjectRow } from "./sidebar-rows.tsx";
import { single } from "../ui/active.ts";
import { AG } from "../agents/agents.ts";
import { placeOf } from "../agents/place.ts";
import { newChatTarget } from "../project/agent.ts";
import { runningKey } from "../agents/running-state.ts";

export function registerSidebarGroup<T>(name: string, group: SidebarGroup<T>) {
  groups.set(name, group as SidebarGroup<unknown>);
}

/// What the pane shows, for the sidebar's highlight; null off the pane.
export function paneOn(): { view: PaneNow["view"] | null } {
  return currentLocation()?.view === "pane" ? pane.current() : { view: null };
}

/// The project on screen, its page or a session of it, and the machine
/// its start input opens on; null elsewhere.
export function projectHere() {
  const on = paneOn();
  if (on.view === "agent" && AG.draft) return { project: AG.draft, machine: null };
  // The session on screen, which the pane may have moved to since the window opened it.
  const page = currentLocation();
  const target = page?.view === "pane" ? page.arg : null;
  const open = AG.page?.session ?? (target && typeof target === "object" && "agent" in target ? target.agent : null);
  const agent = on.view === "agent" && open ? { machine_id: open.machine_id, cwd: open.cwd, main: placeOf(open)?.main } : null;
  return newChatTarget({ shown: currentLocation()?.view ?? null, picked: state.picked, view: on.view, agent, mac: MACHINES.mac?.id }, state.home?.projects);
}

shell.subscribe((now, was) => { if (!now.assistant !== !was.assistant) drawSide(); });

/// Draws a project's sessions in place of the sidebar while one of them shows, or the sidebar again. The project's page opened from them keeps
/// them beside it. A project window, which has no sidebar, shows its project's sessions on every
/// page. The phone layout has no column for them. Their reads redraw only this.
export function drawRail() {
  const view = currentLocation()?.view, side = shell.getState().side, shown = railProject(side.rail);
  const project = aloneProject() ?? (windowAlone ? null : view === "pane" ? projectHere()?.project ?? null
    : view === "projects" && shown && !side.railHidden && state.picked === shown ? shown : null);
  // A chat's window lists its threads even before it has any.
  const chat = !project && view === "pane" ? chatHere(windowAlone?.kind === "chat") : null;
  const rail: Rail | null = onPhone() || isMobile() ? null
    : project ? { kind: "project", project } : chat ? { kind: "chat", chat: runningKey(chat.chat) } : null;
  setRail(rail);
  syncProjectFollow();
  placeWindowParts({ rail: rail?.kind === "project" ? projectRail(rail.project, openKey()) : chat && rail ? <ChatRail rail={chat} current={openKey()} /> : null });
}

/// The rail of the assistant chat on screen, or of the chat whose thread is: null when it has
/// no threads, unless `always`.
function chatHere(always: boolean) {
  const open = AG.open;
  if (!open || AG.draft) return null;
  const listed = sessions.filter(sessionVisible), session = listed.find(row => row.machine_id === open.machine_id && row.id === open.session) ?? AG.page?.session;

  return session ? chatRail(listed, session, always) : null;
}

let chatsWaiting = 0;
sessionsRead.add(() => {
  if (currentLocation()?.view === "pane") drawRail();
  // The Assistant link counts the chats waiting on an answer.
  const asking = chatsAsking(sessions.filter(sessionVisible));
  if (asking !== chatsWaiting) { chatsWaiting = asking; drawSide(); }
});

/// The running key of the session on screen; null for a draft or none.
const openKey = () => AG.open && !AG.draft ? runningKey({ machine_id: AG.open.machine_id, id: AG.open.session }) : null;

// ⌃Tab in a project's session opens the next of its running sessions, in the rail's order and
// round to the first; ⌃⇧Tab the one before. A side pane with focus takes it for its tabs first.
document.addEventListener("keydown", e => {
  if (e.key !== "Tab" || !e.ctrlKey || e.altKey || e.metaKey || e.isComposing || currentLocation()?.view !== "pane") return;
  const project = projectHere()?.project, running = project ? railSessions(project)?.running ?? [] : chatHere(false)?.running ?? [];
  if (!running.length) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const at = running.findIndex(row => runningKey(row) === openKey());
  const next = at < 0 ? (e.shiftKey ? running.length - 1 : 0) : (at + (e.shiftKey ? -1 : 1) + running.length) % running.length;
  if (next !== at) go("pane", { agent: running[next] });
}, true);

export function drawSide() {
  const home = state.home, location = currentLocation()?.view;
  const links: SideLink[] = [
    { svg: IC.find, label: "Search", count: 0, className: "cc-search", open: openPalette, shortcut: shortcut("Ctrl+K", "⌘K") },
    { svg: IC.home, label: "Home", count: home?.needs_you.length ?? 0, current: location === "home", hot: true, open: () => go("home") },
    { svg: IC.chat, label: "Assistant", count: chatsWaiting, hot: true, current: !!shell.getState().assistant, open: () => go("pane", { assistant: "chats" }) },
  ];
  const terms = allTerms.list(ALL);
  if (home?.machines && terms) links.push({ svg: IC.term, label: "Terminals", count: terms.length, current: location === "terminals", open: () => go("terminals") });
  const side: ReactNode[] = [];

  for (const [name, g] of [...groups].sort((a, b) => a[1].order - b[1].order)) {
    const items = g.load();
    if (items != null) side.push(<SidebarGroupView key={name} name={name} group={{ title: "", ...g, items }} />);
  }

  drawRail();
  placeWindowParts({ sidebar: <SidebarShell links={links} groups={side} footer={footer(home?.machines)} addMachine={openAddMachine} /> });
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
  drawSide();
  redrawProjectsPage();
}

/// Opens project `name` in a window of its own, or brings up the one it
/// already has.
export function openProjectWindow(name: string) {
  openWindow({ kind: "project", project: name });
}

/// Opens assistant chat `chat` with its threads in a window of its own, or brings up the one it
/// already has.
export function openChatWindow(chat: Pick<AgentRow, "machine_id" | "id">) {
  openWindow({ kind: "chat", machine_id: chat.machine_id, session: chat.id });
}

function openWindow(alone: Alone) {
  invoke("center_open_window", { alone }).catch(e => console.warn("opening a window:", e));
}

/// What lets project `name` open in a window of its own: ⌘-click or middle-click on the
/// element given `clicks`, which also reads the project ahead on hover, or "Open in New Window"
/// from a right-click on the one given `menu`. The capture runs before the click opens it here.
export function projectWindowProps(name: string): ProjectWindowProps {
  const open = (e: ReactMouseEvent) => {
    if (onPhone() || !opensNewWindow(e)) return;
    e.preventDefault();
    e.stopPropagation();
    openProjectWindow(name);
  };
  return {
    clicks: { onClickCapture: open, onAuxClick: open, ...prefetchOnHover(name) },
    menu: { onContextMenu: (e: ReactMouseEvent) => { e.stopPropagation(); openProjectMenu(e.nativeEvent, name); } },
  };
}

/// The right-click menu of project `name`: a window of its own, and
/// removing it once it is imported (one from running agents only has
/// nothing to remove).
function openProjectMenu(e: MouseEvent, name: string) {
  const p = projectRows()?.find(x => x.name === name);
  const on = state.top.includes(name);
  const items: MenuItem[] = [
    ...(onPhone() ? [] : [{ label: NEW_WINDOW, icon: "openWindow" as const, on: () => openProjectWindow(name) }]),
    { label: on ? "Unpin" : "Pin to top", icon: "pin", on: () => setProjectOnTop(name, !on) },
  ];
  const folder = p && whereRows(p, whereOf(name) || null, MACHINES.mac?.id || null).find(r => r.mac && r.folder)?.folder;
  if (folder && !onPhone()) items.push({ label: "Open in editor", icon: "paneText", on: () => void openInEditor(folder) });
  const label = p?.url ? openOnLabel(p.url) : null;
  if (p?.url && label) items.push({ label, icon: "openWindow", on: () => openUrl(p.url!) });
  if (p?.imported) items.push({ label: "Rename", icon: "edit", on: () => void import("../project/rename.tsx").then(m => m.renameProject(p)) });
  if (p?.imported) items.push({ kind: "separator" }, { label: REMOVE_PROJECT, icon: "remove", warn: true, on: () => removeProject(name) });
  openMenu(e, name, items);
}

/// Asks, then removes project `name` from Beep. Its files stay on every
/// machine.
export function removeProject(name: string) {
  confirmSheet({
    ...removeProjectSheet(name),
    run: async () => {
      await invoke("remove_project", { project: name });
      forgetProject(name);

      if (currentLocation()?.view === "projects" && state.picked === name) openProjects();
      else { draw(); redrawProjectsPage(); }

      loadFound();
    },
  });
}

function projectReact(project: Placed<Project>, label: string, pinned: boolean, indented: boolean) {
  return <SidebarProjectRow project={project} label={label} pinned={pinned} indented={indented} conflicts={conflictList(fsync.detail(project.name)).length}
    current={currentLocation()?.view === "projects" && state.picked === project.name}
    open={() => openProjects(project.name)} pin={() => { void setProjectOnTop(project.name, !pinned); }}
    windowProps={projectWindowProps(project.name)} />;
}
registerSidebarGroup<Placed<Project>>("pinned", {
  order: 2,
  title: "Pinned",
  load: pinnedProjects,
  render: p => projectReact(p, projectName(p), true, false),
  key: p => `project:${p.name}`,
});

registerSidebarGroup("views", {
  order: 6,
  title: "Beep",
  load: () => ["settings" as const],
  key: v => v,
  render: v => <SidebarItem svg={IC.gear} label="Settings" shortcut={shortcut("Ctrl+,", "⌘,")} open={() => go("pane", v)} />,
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
  drawSide();
}

function foldItem(p: OrgHeading | GroupHeading): MenuItem {
  return { label: p.folded ? "Unfold" : "Fold", on: () => toggleFold(p) };
}

/// Opens the Add a machine sheet; the machines topic brings the new one.
export function openAddMachine() {
  void import("../machines/addmachine.tsx").then(m => m.openAddMachine());
}

/// Opens the Projects page, filtered to `name` when given.
export function openProjects(name?: string | null) {
  return go("projects", { project: name || null });
}

/// Opens a project once it is in: the projects topic brings Home, and the found folders load again.
function landed(p: { name: string }) {
  state.picked = p.name;
  sessionsDue.add(p.name);
  pages.drop("where", p.name);
  loadFound();
  openProjects(p.name);
}

/// Opens the Import a project sheet; the project is selected once it is in.
export function importSheet(repo: string | null = null, machine: string | null = null) {
  void import("../addproject/importproject.tsx").then(m => m.openImportProject({
    machines: state.home?.machines || [],
    macId: MACHINES.mac?.id || null,
    repo,
    machine,
    onDone: landed,
  }));
}

/// Opens the Add a project sheet: found folders, a folder, a clone, or a new project.
export function addProject() {
  void import("../addproject/addproject.tsx").then(m => m.openAddProject({
    machines: (state.home?.machines || []).filter(mac => mac.can_host !== false),
    onImported: landed,
    onChoose: () => void import("../addproject/addfolder.tsx").then(f => f.openAddFolder({
      machines: state.home?.machines || [],
      macId: MACHINES.mac?.id || null,
      onDone: landed,
      onBack: addProject,
    })),
    onClone: repo => importSheet(repo ?? null),
    onNew: () => void import("../addproject/newproject.tsx").then(n => n.openNewProject({
      machines: state.home?.machines || [],
      onDone: landed,
      onBack: addProject,
      onClone: repo => importSheet(repo),
    })),
  }));
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

  drawFound();
  const { rescanAll } = await import("../addproject/addproject.tsx");
  await rescanAll(() => {}, (_, after) => {
    if (!after.rows) return;
    state.found = after.rows;
    drawFound();
  });
});

/// The Projects overview and a first start Home, which list the found checkouts.
function drawFound() {
  if (!state.picked) redrawProjects();
  draw();
}

/// Imports a found row from the Projects page and opens it.
export async function importFromPage(row: FoundRow) {
  if (state.importing) return;
  state.importing = row.name;
  state.foundError = null;
  redrawProjects();
  draw();

  try {
    const { importFound } = await import("../addproject/addproject.tsx");
    const project = await importFound(row);
    state.importing = null;
    landed(project);
  } catch (e) {
    state.importing = null;
    state.foundError = { name: row.name, message: reason(e) };
    drawFound();
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
  load: () => {
    const rows = projectRows();
    // Those pinned to the top show under Pinned instead.
    return rows ? sideProjects(splitOnTop(rows, state.top).rest, state.groups, folded) : null;
  },
  key: p => p.group ? `group:${p.group.id}` : p.org !== undefined ? `org:${p.org}` : `project:${p.name}`,
  render: p => {
    if (!p.group && p.org === undefined) return projectReact(p, p.repo ?? projectName(p), false, !!p.repo);
    const title = p.group ? p.group.name : `${p.org}/`;
    const label = p.group ? <span className="sidebar-icon" style={{ display: "contents" }}><Icon name="folder" /></span> : null;
    return <button type="button" className={`cc-org${p.group ? " gp" : ""}`}
      title={p.group ? `${title}\n${p.group.paths.join(", ")} on every machine` : title} aria-label={title}
      aria-expanded={!p.folded} aria-current={currentLocation()?.view === "projects" && state.picked && p.folded?.includes(state.picked) ? "true" : undefined}
      onClick={() => toggleFold(p)} onContextMenu={(event: React.MouseEvent) => {
        const native = event.nativeEvent;
        if (p.group) { const grp = p.group; void import("../project/groups.tsx").then(m => m.groupMenu(native, grp, [foldItem(p)])); }
        else openMenu(native, p.org ?? "", [foldItem(p)]);
      }}>
      <span className={`twist${p.folded ? "" : " open"}`}><Icon name="twist" /></span>{label}<span className="tt">{title}</span>
    </button>;
  },

});

onClosing(() => drawSide());

/// A sidebar item's key hint: none on a phone, whose keyboard rarely has the keys.
function shortcut(windows: string, apple: string): string | undefined {
  if (onPhone()) return undefined;

  return WINDOWS ? windows : apple;
}
