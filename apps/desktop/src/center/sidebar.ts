import { WINDOWS, onPhone } from "../ui/platform.ts";
// The sidebar: its groups (pinned, chats, views, projects, machines) and
// the sheets that add a machine or a project.
import { onClosing, visibleRunning } from "../agents/closing.ts";
import { plural, reason } from "../ui/format.ts";
import { writeClipboard } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { openPalette } from "./palette.ts";
import { pane } from "../bar/pane.ts";
import { live, S, type View } from "../bar/core.ts";
import { openAddMachine as openAddMachineSheet } from "../machines/addmachine.ts";
import { openImportProject } from "../addproject/importproject.ts";
import { openAddProject, importFound, rescanAll } from "../addproject/addproject.ts";
import { openAddFolder } from "../addproject/addfolder.ts";
import { openNewProject } from "../addproject/newproject.ts";
import { foldKey, mergeProjects, parseFolded, sideProjects, splitOnTop, removeProjectSheet, REMOVE_PROJECT, type FoundRow, type GroupHeading, type OrgHeading, type Placed, type Project } from "../project/projects-state.ts";
import { confirmSheet, openMenu, renameSheet, type MenuItem } from "../ui/menu.ts";
import { openInWindow } from "../chat/chat.ts";
import { openInEditor } from "../project/editors.ts";
import { whereRows, openOnLabel } from "../project/projects-state.ts";
import { openUrl } from "./openitems.ts";
import { renderGroup, renderInvite } from "../chat/groups.ts";
import { NEW_WINDOW, opensNewWindow } from "../project/projectwindow.ts";
import { ALL } from "../terminals/terminals.ts";
import { runningChats, sideThreads } from "./sidechats.ts";
import { withoutResearch } from "../research/research-list.ts";
import { RESEARCHES } from "../research/researches.ts";
import { type ChatItem, type ProjectPin, type SideChat, state, type GroupSlot, type SidebarGroup, groups } from "./state.ts";
import { stuck, problem, draw, jump } from "./home.ts";
import { allTerms } from "../terminals/view.ts";
import { currentLocation, go, drawTop, openResearch } from "./views.ts";
import { groupMenu } from "../project/groups.ts";
import { loadOverview } from "../project/projects.ts";
import { redrawProjects, redrawProjectsPage } from "../project/sections.ts";
import { prefetchOnHover, sessionsDue } from "../project/loads.ts";
import type { ProjectWindowProps } from "../project/projectwindow.ts";
import { drawProjects } from "../project/page.ts";
import { refresh } from "./refresh.ts";
import { threadChats } from "./threadcard.ts";
import { beepViews, groupShown } from "../settings/subscription.ts";
import { signedOutFooter } from "../settings/server-state.ts";
import { openSettings } from "../settings/sheet.ts";
import { IC } from "../ui/icons.tsx";
import { syncProjects, syncConflicts } from "../project/syncpage.ts";
import type { MachineCard } from "../machines/machine-state.ts";
import type { HomeView } from "./home-state.ts";
import { createElement, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { placeWindowParts } from "./window-root.ts";
import { windowHost } from "./window-shell.tsx";
import { SidebarShell, SidebarItem, SidebarGroup as SidebarGroupView, type SideFooter, type SideLink } from "./sidebar-shell.tsx";
import { SidebarChatRow, SidebarProjectRow } from "./sidebar-rows.tsx";
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

/// What the pane shows, for the sidebar's highlight.
export function paneOn(): { view: View | null; session: string | null; group: string | null } {
  return currentLocation()?.view === "pane" ? pane.current() : { view: null, session: null, group: null };
}

export function drawSide() {
  const home = state.home, location = currentLocation()?.view;
  const links: SideLink[] = [
    { svg: IC.find, label: "Search", count: 0, className: "cc-search", open: openPalette, shortcut: WINDOWS ? "Ctrl+K" : "⌘K" },
    { svg: IC.home, label: "Home", count: home?.needs_you.length ?? 0, current: location === "home", hot: true, open: () => go("home") },
  ];
  if (home?.running) links.push({ svg: IC.run, label: "Running", count: visibleRunning(home.running).length, current: paneOn().view === "agents", open: () => go("pane", "agents") });
  const terms = allTerms.list(ALL);
  if (home?.machines && terms) links.push({ svg: IC.term, label: "Terminals", count: terms.length, current: location === "terminals", open: () => go("terminals") });
  if (syncProjects().length) links.push({ svg: IC.sync, label: "Sync", count: syncConflicts(), current: location === "sync", hot: true, open: () => go("sync") });
  if (home?.coming_up) links.push({ svg: IC.cal, label: "Coming up", count: home.coming_up.length, open: () => jump("sec-coming") });
  const side: ReactNode[] = [];

  for (const [name, g] of [...groups].sort((a, b) => a[1].order - b[1].order)) {
    // Rows that fill in once the server answers, never an empty sidebar.
    if (name === "chats" && g.items == null && !home && groupShown("chats", state.subscribed)) {
      side.push(createElement("div", { className: "cc-grp", key: name }, createElement("h3", null, "Chats"), ...[70, 55, 62].map((width, index) => createElement("div", { className: "cc-skel", style: { width: `${width}%` }, key: index }))));
    }
    if (g.items != null) side.push(createElement(SidebarGroupView, { key: name, name, group: g }));
  }

  const host = windowHost("side");
  placeWindowParts({ sidebar: createElement(SidebarShell, { host, phone: onPhone(), links, groups: side, footer: footer(home?.machines), signIn: () => openSettings("devices"), addMachine: openAddMachine }) });
  if (currentLocation()?.view === "pane") drawTop();
}

function footer(machines: MachineCard[] | null | undefined): SideFooter {
  const out = signedOutFooter(S.server, S.signedOut);
  if (out) return { dot: "off", text: out.text, action: "signin" };
  const action = machines ? "add" : null;
  const p = problem();
  if (p) return { dot: p.kind === "unlinked" || p.kind === "noserver" ? "off" : "bad", text: p.kind === "unlinked" ? "Not linked" : p.kind === "noserver" ? "No server" : p.kind === "revoked" ? "Removed" : "Refused", action };
  if (!state.home) return { dot: stuck() ? "bad" : "off", text: stuck() ? "Not connected" : "Connecting…", action };
  if (!machines) return { dot: null, text: "Machines unavailable", action };
  if (!machines.length) return { dot: null, text: "No machines linked", action };
  return { dot: "on", count: machines.filter(m => m.online).length, text: ` of ${plural(machines.length, "machine", "machines")} online`, action };
}

/// Pinned chats and groups, then the newest of the rest, the open one
/// highlighted.
const SIDE_CHATS = 5;
export let chatList: ChatItem[] = [];
let chatFetch: Promise<ChatItem[]> | null = null;
/// The chats the server last said have a run going on, from any device.
let serverRunning: string[] = [];
let runningReadVersion = 0;

/// One fetch feeds both groups, even when both ask at once.
function fetchChats() {
  if (chatFetch) return chatFetch;
  // The rows come from this Mac's copy; which run elsewhere is the
  // server's to say, so the rows never wait for it.
  const version = ++runningReadVersion;
  pane.activeRuns().then(runs => { if (version === runningReadVersion) { serverRunning = runs; drawSide(); } });
  const mine: Promise<ChatItem[]> = pane.chatRows().then(rows => {
    if (chatFetch === mine) chatList = rows;
    return rows;
  }).finally(() => { if (chatFetch === mine) chatFetch = null; });
  return chatFetch = mine;
}

/// The chats with a reply coming: run here, or on another device.
export function running() {
  return runningChats(pane.running(), serverRunning);
}

export function reloadChats() {
  runningReadVersion++;
  // A fetch that started before this change may miss it: the next one is new.
  chatFetch = null;
  return Promise.all([reloadGroup("pinned"), reloadGroup("chats")]);
}

/// The projects pinned to the top, as rows of the Pinned group shaped like
/// chat rows.
function pinnedProjects(): ProjectPin[] {
  const rows = projectRows();
  if (!rows) return [];
  return splitOnTop(rows, state.top).onTop.map(p => ({ kind: "project", id: p.name, title: p.name, pinned: true, project: p }));
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

/// The pin button of a sidebar row: pins it to the top, or unpins it.
/// Gives the pin button of chat `id` the focus again, unless focus has gone
/// elsewhere meanwhile (a redraw leaves it on the page body).
function restorePinFocus(id: string) {
  const on = document.activeElement;
  if (on && on !== document.body) return;
  document.querySelector<HTMLElement>(`.cc-chat .pa[data-id="${CSS.escape(id)}"]`)?.focus();
}

export async function setPinned(c: SideChat, pinned: boolean) {
  if (c.kind === "project") return setProjectOnTop(c.id, pinned);
  const active = document.activeElement as HTMLElement | null;
  const hadFocus = active?.classList.contains("pa") && active.dataset.id === c.id;

  try {
    await invoke(c.kind === "group" ? "pin_group" : "pin_session", { id: c.id, pinned });
  } catch (e) {
    console.warn("pinning a chat:", e);
  }

  await reloadChats();
  if (hadFocus) restorePinFocus(c.id);
}

function chatMenu(e: MouseEvent, c: ChatItem) {
  openMenu(e, c.title, [
    { label: c.pinned ? "Unpin" : "Pin to top", on: () => setPinned(c, !c.pinned) },
    ...(onPhone() ? [] : [{ label: "Open in window", on: () => void openInWindow(c.id) }]),
    { label: "Rename…", on: () => renameSheet({ title: "Rename chat", value: c.title, run: async title => { await invoke("rename_session", { id: c.id, title }); await reloadChats(); } }) },
    { label: "Copy share link", on: () => void invoke<{ url: string }>("share_session", { id: c.id }).then(link => writeClipboard(link.url), err => console.warn("sharing a chat:", err)) },
    { label: "Stop sharing", on: () => void invoke("unshare_session", { id: c.id }).catch(err => console.warn("unsharing a chat:", err)) },
    { label: "Delete…", warn: true, on: () => deleteChat(c) },
  ]);
}

function groupChatMenu(e: MouseEvent, c: ChatItem) {
  openMenu(e, c.title, [
    { label: c.pinned ? "Unpin" : "Pin to top", on: () => setPinned(c, !c.pinned) },
    { label: "Invite…", on: () => { go("pane", { group: { id: c.id } }); void renderInvite(c.id, c.title); } },
    { label: "Leave group…", warn: true, on: () => { go("pane", { group: { id: c.id } }); void renderGroup(c.id, "leave"); } },
  ]);
}

function researchMenu(e: MouseEvent, c: ChatItem) {
  const items: MenuItem[] = [
    { label: "Open", on: () => openResearch(c.id) },
    { label: c.pinned ? "Unpin" : "Pin to top", on: () => setPinned(c, !c.pinned) },
    { label: "Export PDF", on: () => void invoke<string>("export_research", { session: c.id }).then(path => invoke("reveal_file", { path }), err => console.warn("exporting a research:", err)) },
  ];
  if (running().has(c.id)) items.push({ label: "Stop", warn: true, on: () => void invoke("stop_research", { session: c.id }).catch(err => console.warn("stopping a research:", err)) });
  openMenu(e, c.title, items);
}

/// Asks, then deletes chat `c` as the All chats list does. The open chat
/// sends the pane to that list.
function deleteChat(c: SideChat) {
  confirmSheet({
    title: `Delete ${c.title}?`,
    text: "Its messages are deleted for good.",
    action: "Delete",
    run: async () => {
      await invoke("delete_session", { id: c.id });
      live.delete(c.id);
      const wasOpen = paneOn().session === c.id;
      if (S.session === c.id) S.session = null;
      await reloadChats();
      if (wasOpen) go("pane", "chats");
    },
  });
}

/// The chats about one pull request or issue each project lists under
/// it: the open one, the running ones, then the newest.
const SIDE_THREADS = 3;
function nestedThreads() {
  const on = paneOn();
  const open = on.view === "chat" ? on.session : null;
  return sideThreads(threadChats?.values() ?? [], chatList.map(c => c.id), open, running(), SIDE_THREADS);
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
/// removing it once it is imported (one from running agents only is not
/// the server's to remove).
function openProjectMenu(e: MouseEvent, name: string) {
  const p = projectRows()?.find(x => x.name === name);
  const on = state.top.includes(name);
  const items: MenuItem[] = [...(onPhone() ? [] : [{ label: NEW_WINDOW, on: () => openProjectWindow(name) }]), { label: on ? "Unpin" : "Pin to top", on: () => setProjectOnTop(name, !on) }];
  const folder = p && whereRows(p, state.where.get(name) || null, state.mac?.id || null).find(r => r.mac && r.folder)?.folder;
  if (folder && !onPhone()) items.push({ label: "Open in editor", on: () => void openInEditor(folder) });
  const label = p?.url ? openOnLabel(p.url) : null;
  if (p?.url && label) items.push({ label, on: () => openUrl(p.url!) });
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
      state.pview.delete(name);

      if (currentLocation()?.view === "projects" && state.picked === name) openProjects();
      else { draw(); redrawProjectsPage(); }

      loadFound();
      loadOverview();
      refresh();
    },
  });
}

function projectReact(project: Placed<Project>, label: string, pinned: boolean, indented: boolean) {
  const on = paneOn();
  return createElement(SidebarProjectRow, { project, label, pinned, indented,
    current: currentLocation()?.view === "projects" && state.picked === project.name,
    threads: nestedThreads().get(project.name) ?? [], activeChat: on.view === "chat" ? on.session : null,
    running: running(), open: () => openProjects(project.name), pin: () => { void setProjectOnTop(project.name, !pinned); },
    windowProps: projectWindowProps(project.name), openThread: id => { go("pane", { chat: { session: id } }); } });
}
function chatReact(c: SideChat) {
  if (c.kind === "project") return projectReact(c.project, c.title, true, false);
  const on = paneOn(), current = c.kind === "group" ? on.group === c.id : on.view === "chat" && on.session === c.id;
  return createElement(SidebarChatRow, { chat: c, current, answering: c.kind === "chat" && running().has(c.id),
    open: () => pickChat(c), pin: () => { void setPinned(c, !c.pinned); },
    menu: event => c.kind === "group" ? groupChatMenu(event, c) : c.research ? researchMenu(event, c) : chatMenu(event, c) });
}

function pickChat(c: SideChat) {
  if (c.kind === "project") return openProjects(c.id);
  if (c.kind === "group") return go("pane", { group: { id: c.id } });
  if (c.research) return openResearch(c.id);
  go("pane", { chat: { session: c.id } });
}

/// Pinned chats and groups go to the Pinned group; the newest of the rest stay
/// under Chats. `rows` are newest first, as the pane lists them.
function splitPinned<T extends { pinned: boolean }>(rows: T[], limit: number) {
  return {
    pinned: rows.filter(c => c.pinned),
    recent: rows.filter(c => !c.pinned).slice(0, limit),
  };
}

registerSidebarGroup<SideChat>("pinned", {
  order: 2,
  title: "Pinned",
  async load() {
    const chats = state.subscribed ? splitPinned(await fetchChats(), SIDE_CHATS).pinned : [];
    return [...chats, ...pinnedProjects()];
  },
  render: chatReact,
  key: c => `${c.kind}:${c.id}`,
});

registerSidebarGroup<SideChat>("chats", {
  order: 3,
  title: "Chats",
  onTitle: () => go("pane", "chats"),
  empty: () => (chatList.length ? "" : "No chats yet"),
  action: { label: "New chat", title: false, run: () => { go("pane", { chat: { session: null } }); } },
  async load() {
    if (!groupShown("chats", state.subscribed)) return null;
    // Chats about one thread are listed under their project, researches
    // under Research.
    const rows = withoutResearch(await fetchChats(), RESEARCHES).filter(c => c.pinned || !threadChats?.has(c.id));
    return splitPinned(rows, SIDE_CHATS).recent;
  },
  render: chatReact,
  key: c => `${c.kind}:${c.id}`,
});

registerSidebarGroup("views", {
  order: 6,
  title: "Beep",
  load: async () => beepViews([
    { view: "memories" as const, label: "Memories", svg: IC.mem },
    { view: "routines" as const, label: "Routines", svg: IC.loop },
    { view: "settings" as const, label: "Settings", svg: IC.gear },
  ], state.subscribed),
  key: v => v.view,
  render: v => createElement(SidebarItem, { svg: v.svg, label: v.label, current: v.view !== "settings" && paneOn().view === v.view, shortcut: v.view === "settings" ? (WINDOWS ? "Ctrl+," : "⌘,") : "", open: () => go("pane", v.view) }),
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
    machines: (state.home?.machines || []).filter(m => m.can_host !== false),
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
      machines: (state.home?.machines || []).filter(m => m.can_host !== false),
      macId: state.mac?.id || null,
      onDone: landed,
      onBack: addProject,
    }),
    onClone: repo => importSheet(repo ?? null),
    onNew: () => openNewProject({
      machines: (state.home?.machines || []).filter(m => m.can_host !== false),
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
    if (!p.group && p.org === undefined) return projectReact(p, p.repo ?? p.name, false, !!p.repo);
    const title = p.group ? p.group.name : `${p.org}/`;
    const label = p.group ? createElement("span", { className: "sidebar-icon", style: { display: "contents" }, dangerouslySetInnerHTML: { __html: IC.folder } }) : null;
    return createElement("button", { type: "button", className: `cc-org${p.group ? " gp" : ""}`,
      title: p.group ? `${title}\n${p.group.paths.join(", ")} on every machine` : title, "aria-label": title,
      "aria-expanded": !p.folded, "aria-current": currentLocation()?.view === "projects" && state.picked && p.folded?.includes(state.picked) ? "true" : undefined,
      onClick: () => toggleFold(p), onContextMenu: (event: React.MouseEvent) => p.group ? groupMenu(event.nativeEvent, p.group, [foldItem(p)]) : openMenu(event.nativeEvent, p.org ?? "", [foldItem(p)]) },
      createElement("span", { className: `twist${p.folded ? "" : " open"}`, dangerouslySetInnerHTML: { __html: IC.twist } }), label, createElement("span", { className: "tt" }, title));
  },

});

onClosing(() => drawSide());
