// The sidebar: its groups (pinned, chats, views, projects, machines) and
// the sheets that add a machine or a project.
import { plural } from "../ui/format.ts";
import { svgEl as icon } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { splitPinned } from "./pinned.ts";
import { openPalette } from "./palette.ts";
import { pane } from "../bar/pane.ts";
import type { MachineCard } from "./home-state.ts";
import { openAddMachine as openAddMachineSheet } from "../machines/addmachine.ts";
import { openImportProject } from "../addproject/importproject.ts";
import { openAddProject, importFound } from "../addproject/addproject.ts";
import { openNewProject } from "../addproject/newproject.ts";
import { foldKey, mergeProjects, parseFolded, sideProjects, splitOnTop, removeProjectSheet, REMOVE_PROJECT, type FoundRow, type GroupHeading, type OrgHeading, type Placed, type Project } from "../project/projects-state.ts";
import { button, el as h, type Kid } from "../ui/dom.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { NEW_WINDOW, opensNewWindow } from "../project/projectwindow.ts";
import { ALL } from "../terminals/terminals.ts";
import { runningChats, sideThreads, threadLabel, type ThreadChatEntry } from "./sidechats.ts";
import { withoutResearch } from "../research/research-list.ts";
import { RESEARCHES } from "../research/researches.ts";
import { type ChatItem, type ProjectPin, type SideChat, $, IC, state, type GroupSlot, type SidebarGroup, groups } from "./state.ts";
import { projectDot, skeleton } from "./pieces.ts";
import { stuck, problem, draw, jump } from "./home.ts";
import { allTerms } from "../terminals/view.ts";
import { single } from "./openitems.ts";
import { shown, go, drawTop } from "./views.ts";
import { groupHeading } from "../project/groups.ts";
import { loadOverview } from "../project/projects.ts";
import { projectsEl, redrawProjects, redrawProjectsPage } from "../project/sections.ts";
import { prefetchOnHover, sessionsDue } from "../project/loads.ts";
import { drawProjects } from "../project/page.ts";
import { refresh } from "./refresh.ts";
import { threadChats } from "./threadcard.ts";
import { reason } from "../addproject/importproject-state.ts";
import { beepViews, groupShown } from "../settings/subscription.ts";

export function registerSidebarGroup<T>(name: string, group: SidebarGroup<T>) {
  const slot: GroupSlot<T> = { title: "", ...group, items: null, loading: false, again: false };
  groups.set(name, slot);
  reloadGroup(name);
}

/// Loads a group's items again. Never two loads at once: one asked for
/// during a load runs once more after it.
export async function reloadGroup(name: string) {
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
  drawSide();
  if (g.again) { g.again = false; reloadGroup(name); }
}

interface SideOpts {
  cls?: string;
  current?: boolean;
  hot?: boolean;
  kbd?: string;
  on?: () => void;
}

export function sideItem(svg: string | null, label: string, count: number | undefined, opts: SideOpts = {}) {
  const b = h("button", `cc-si${opts.cls ? " " + opts.cls : ""}`, svg ? icon(svg) : null, h("span", "tt", label));
  b.type = "button";
  if (opts.current) b.setAttribute("aria-current", "true");
  if (count) b.append(h("span", `n${opts.hot ? " hot" : ""}`, String(count)));
  if (opts.kbd) b.append(h("kbd", "", opts.kbd));
  if (opts.on) b.addEventListener("click", opts.on);
  return b;
}

/// What the pane shows, for the sidebar's highlight.
export function paneOn(): { view: string | null; session: string | null; group: string | null } {
  return shown === "pane" ? pane.current() : { view: null, session: null, group: null };
}

export function drawSide() {
  const home = state.home;
  const head = h("div", "cc-head", "");
  head.setAttribute("data-tauri-drag-region", "");
  const top = h("div", "cc-grp",
    sideItem(IC.find, "Search", 0, { cls: "cc-search", on: openPalette, kbd: "⌘K" }),
    sideItem(IC.home, "Home", home?.needs_you.length, { current: shown === "home", hot: true, on: () => go("home") }));
  // Running opens the coding agents' list, the full view of Home's section.
  if (home?.running) top.append(sideItem(IC.run, "Running", home.running.length, { current: paneOn().view === "agents", on: () => go("pane", "agents") }));
  // Terminals on every machine, once the server lists them.
  const termList = allTerms.list(ALL);
  if (home?.machines && termList) top.append(sideItem(IC.term, "Terminals", termList.length, { current: shown === "terminals", on: () => go("terminals") }));
  if (home?.coming_up) top.append(sideItem(IC.cal, "Coming up", home.coming_up.length, { on: () => jump("sec-coming") }));
  const side = [h("div", "cc-brand", h("b", "", "Beep")), top];
  for (const [name, g] of [...groups].sort((a, b) => a[1].order - b[1].order)) {
    // Rows that fill in once the server answers, never an empty sidebar.
    if (name === "chats" && g.items == null && !home) side.push(h("div", "cc-grp", h("h3", "", "Chats"), ...skeleton([70, 55, 62])));
    if (g.items == null) continue;
    const grp = h("div", `cc-grp ${name}`);
    if (g.title) {
      const h3 = h("h3", "", g.onTitle ? button(g.title, "cc-gt", g.onTitle) : g.title);
      if (g.action) h3.append(g.action());
      grp.append(h3);
    }
    const none = typeof g.empty === "function" ? g.empty() : g.empty;
    if (none && !g.items.length) grp.append(typeof none === "string" ? h("span", "cc-si none", none) : none);
    for (const item of g.items) {
      const el = g.row(item);
      if (g.onPick) el.addEventListener("click", () => g.onPick!(item));
      grp.append(el);
    }
    if (g.foot) grp.append(...g.foot());
    if (grp.childElementCount > (g.title ? 1 : 0)) side.push(grp);
  }
  const foot = h("div", "cc-foot", ...footer(home?.machines).map(f => h("span", "", f)));
  if (home?.machines) {
    const add = plusButton("Add a machine", openAddMachine);
    add.classList.add("dbtn", "quiet");
    foot.append(add);
  }
  side.push(foot);
  // The head stays put under the window buttons; only the list below it
  // scrolls, and a redraw keeps where it was.
  const el = $("#side");
  let list = el.querySelector<HTMLElement>(":scope > .cc-side-list");
  if (!list) {
    list = h("div", "cc-side-list");
    el.replaceChildren(head, list);
  }
  list.replaceChildren(...side);
  // A chat's title comes from the list, which may just have loaded.
  if (shown === "pane") drawTop();
}

function footer(machines: MachineCard[] | null | undefined): Kid[] {
  const p = problem();
  if (p) return [h("span", "", h("span", `ddot ${p.kind === "unlinked" || p.kind === "noserver" ? "off" : "bad"}`), ` ${p.kind === "unlinked" ? "Not linked" : p.kind === "noserver" ? "No server" : p.kind === "revoked" ? "Removed" : "Refused"}`)];
  if (!state.home) return [h("span", "", h("span", `ddot ${stuck() ? "bad" : "off"}`), stuck() ? " Not connected" : " Connecting…")];
  if (!machines) return ["Machines unavailable"];
  if (!machines.length) return ["No machines linked"];
  const on = machines.filter(m => m.online).length;
  return [h("span", "", h("span", "ddot"), " ", h("b", "", String(on)), ` of ${plural(machines.length, "machine", "machines")} online`)];
}

/// Pinned chats and groups, then the newest of the rest, the open one
/// highlighted.
const SIDE_CHATS = 5;
export let chatList: ChatItem[] = [];
let chatFetch: Promise<ChatItem[]> | null = null;
/// The chats the server last said have a run going on, from any device.
let serverRunning: string[] = [];

/// One fetch feeds both groups, even when both ask at once.
function fetchChats() {
  if (chatFetch) return chatFetch;
  // The rows come from this Mac's copy; which run elsewhere is the
  // server's to say, so the rows never wait for it.
  pane.activeRuns().then(runs => { serverRunning = runs; drawSide(); });
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
function pinToggle(id: string, pinned: boolean, on: () => void) {
  const toggle = button(icon(IC.pin)!, "pa", e => { e.stopPropagation(); on(); });
  toggle.dataset.id = id;
  toggle.setAttribute("aria-label", pinned ? "Unpin" : "Pin to top");
  return toggle;
}

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

function chatRow(c: SideChat) {
  if (c.kind === "project") return withThreads(projectRow(c.project, c.title, true), c.project.name);
  const on = paneOn();
  const open = c.kind === "group" ? on.group === c.id : on.view === "chat" && on.session === c.id;
  const mark = c.kind === "group" ? icon(IC.group) : icon(c.research ? IC.doc : IC.chat);
  // A chat still answering shows a spinner where its pin button was.
  const end = c.kind === "chat" && running().has(c.id) ? runSpin() : pinToggle(c.id, c.pinned, () => setPinned(c, !c.pinned));
  // Two real buttons in one row, never one inside the other.
  const openBtn = h("button", "open", mark, h("span", "tt", c.title));
  openBtn.type = "button";
  openBtn.title = c.title;
  if (open) openBtn.setAttribute("aria-current", "true");
  return h("div", "cc-si cc-chat", openBtn, end);
}

function runSpin() {
  const spin = h("span", "dspin cc-run");
  spin.title = "Answering";
  spin.setAttribute("aria-label", "Answering");
  return spin;
}

/// The chats about one pull request or issue each project lists under
/// it: the open one, the running ones, then the newest.
const SIDE_THREADS = 3;
function nestedThreads() {
  const on = paneOn();
  const open = on.view === "chat" ? on.session : null;
  return sideThreads(threadChats?.values() ?? [], chatList.map(c => c.id), open, running(), SIDE_THREADS);
}

/// Project `name`'s sidebar row `row`, with its thread chats under it.
function withThreads(row: HTMLElement, name: string) {
  const threads = nestedThreads().get(name);
  if (!threads?.length) return row;
  return h("div", "cc-pjthreads", row, ...threads.map(threadRow));
}

/// A chat about one pull request or issue, under its project.
function threadRow(t: ThreadChatEntry) {
  const on = paneOn();
  const label = t.title || threadLabel(t.kind, t.number);
  const openBtn = h("button", "open", h("span", "tt", label));
  openBtn.type = "button";
  openBtn.title = `${label} (${t.project})`;
  if (on.view === "chat" && on.session === t.chat_id) openBtn.setAttribute("aria-current", "true");
  // The project row's own click opens the project; this one opens the chat.
  openBtn.addEventListener("click", e => { e.stopPropagation(); go("pane", { chat: { session: t.chat_id } }); });
  const row = h("div", "cc-si cc-chat cc-thread", openBtn);
  if (running().has(t.chat_id)) row.append(runSpin());
  return row;
}

/// A project's sidebar row, `label` beside its colour, with its pin button.
function projectRow(p: Placed<Project>, label: string, pinned: boolean) {
  const openBtn = h("button", "open", projectDot(p.i), h("span", "tt", label));
  openBtn.type = "button";
  openBtn.title = p.machines.map(m => `${m.name}: ${m.folder}`).join("\n") || p.name;
  if (shown === "projects" && state.picked === p.name) openBtn.setAttribute("aria-current", "true");
  if (p.agents) openBtn.append(h("span", "n", String(p.agents)));
  const toggle = pinToggle(p.name, pinned, () => setProjectOnTop(p.name, !pinned));
  const row = h("div", `cc-si cc-chat${pinned ? "" : " pj-repo"}`, openBtn, toggle);
  newWindowOn(openBtn, row, p.name);
  return row;
}

/// Opens project `name` in a window of its own, or brings up the one it
/// already has.
export function openProjectWindow(name: string) {
  invoke("center_open_project_window", { project: name }).catch(e => console.warn("opening a project window:", e));
}

/// Lets project `name` open in a window of its own: ⌘-click or middle-click on
/// `clicks`, or "Open in New Window" from a right-click on `menu`.
export function newWindowOn(clicks: HTMLElement, menu: HTMLElement, name: string) {
  const open = (e: MouseEvent) => {
    if (!opensNewWindow(e)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    openProjectWindow(name);
  };
  clicks.addEventListener("click", open, true);
  clicks.addEventListener("auxclick", open);
  menu.addEventListener("contextmenu", e => openProjectMenu(e, name));
  prefetchOnHover(clicks, name);
}

/// The right-click menu of project `name`: a window of its own, and
/// removing it once it is imported (one from running agents only is not
/// the server's to remove).
function openProjectMenu(e: MouseEvent, name: string) {
  const items: MenuItem[] = [{ label: NEW_WINDOW, on: () => openProjectWindow(name) }];
  if (projectRows()?.some(p => p.name === name && p.imported)) items.push({ label: REMOVE_PROJECT, warn: true, on: () => removeProject(name) });
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
      if (shown === "projects" && state.picked === name) openProjects();
      else { draw(); redrawProjectsPage(); }
      loadFound();
      loadOverview();
      refresh();
    },
  });
}

function pickChat(c: SideChat) {
  if (c.kind === "project") return openProjects(c.id);
  if (c.kind === "group") return go("pane", { group: { id: c.id } });
  // A research opens in its own window, as from the bar's list.
  if (c.research) return invoke("open_research", { session: c.id }).catch(() => go("pane", { chat: { session: c.id } }));
  go("pane", { chat: { session: c.id } });
}

registerSidebarGroup<SideChat>("pinned", {
  order: 2,
  title: "Pinned",
  async load() {
    const chats = state.subscribed ? splitPinned(await fetchChats(), SIDE_CHATS).pinned : [];
    return [...chats, ...pinnedProjects()];
  },
  row: chatRow,
  onPick: pickChat,
});

registerSidebarGroup<SideChat>("chats", {
  order: 3,
  title: "Chats",
  onTitle: () => go("pane", "chats"),
  empty: () => (chatList.length ? "" : "No chats yet"),
  action() {
    const b = button(icon(IC.plus)!, "", () => go("pane", { chat: { session: null } }));
    b.setAttribute("aria-label", "New chat");
    return b;
  },
  async load() {
    if (!groupShown("chats", state.subscribed)) return null;
    // Chats about one thread are listed under their project, researches
    // under Research.
    const rows = withoutResearch(await fetchChats(), RESEARCHES).filter(c => c.pinned || !threadChats?.has(c.id));
    return splitPinned(rows, SIDE_CHATS).recent;
  },
  row: chatRow,
  onPick: pickChat,
});

registerSidebarGroup("views", {
  order: 6,
  title: "Beep",
  load: async () => beepViews([
    { view: "memories", label: "Memories", svg: IC.mem },
    { view: "routines", label: "Routines", svg: IC.loop },
    { view: "settings", label: "Settings", svg: IC.gear },
  ], state.subscribed),
  row: v => sideItem(v.svg, v.label, 0, { current: v.view !== "settings" && paneOn().view === v.view, kbd: v.view === "settings" ? "⌘," : "" }),
  onPick: v => go("pane", v.view),
});

/// Imported projects merged with the folders agents run in.
export function projectRows() {
  const home = state.home;
  if (!home || (!home.running && !home.projects)) return null;
  return mergeProjects(home.projects, home.running);
}

/// The project headings folded in the sidebar, kept between launches. Only
/// the main window has a sidebar.
const FOLDED_KEY = "beep.sideFolded";
let folded = new Set<string>();
try { folded = parseFolded(localStorage.getItem(FOLDED_KEY)); } catch {}

/// Makes `head` fold the projects under it. Folded, it lights up while it
/// holds the open project.
function foldable(head: HTMLButtonElement, p: OrgHeading | GroupHeading) {
  head.type = "button";
  head.prepend(h("span", `twist${p.folded ? "" : " open"}`, icon(IC.twist)));
  head.setAttribute("aria-expanded", String(!p.folded));
  if (shown === "projects" && state.picked && p.folded?.includes(state.picked)) head.setAttribute("aria-current", "true");
  head.addEventListener("click", () => {
    const key = foldKey(p);
    if (!folded.delete(key)) folded.add(key);
    try { localStorage.setItem(FOLDED_KEY, JSON.stringify([...folded])); } catch { /* kept for this launch only */ }
    reloadGroup("projects");
  });
  return head;
}

/// A small + button for a group's heading or the footer.
export function plusButton(label: string, on: () => void) {
  const b = button(icon(IC.plus)!, "", e => { e.stopPropagation(); on(); });
  b.title = label;
  b.setAttribute("aria-label", label);
  return b;
}

/// An empty group's row: what is missing. The group's + adds one.
export function emptyRow(text: string) {
  return h("div", "cc-si none", text);
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

/// Opens the Import a project sheet, at the folder choice with `choose`; the
/// project is selected once it is in.
export function importSheet(choose = false, repo: string | null = null, machine: string | null = null) {
  openImportProject({
    machines: state.home?.machines || [],
    macId: state.mac?.id || null,
    choose,
    repo,
    machine,
    onDone: landed,
  });
}

/// Opens the Add a project sheet: found folders, a folder, a clone, or a new project.
export function addProject() {
  openAddProject({
    machines: state.home?.machines || [],
    onImported: landed,
    onChoose: () => importSheet(true),
    onClone: repo => importSheet(false, repo ?? null),
    onNew: () => openNewProject({
      machines: state.home?.machines || [],
      onDone: landed,
      onBack: addProject,
      onClone: repo => importSheet(false, repo),
    }),
  });
}

/// Asks the machines for the checkouts their agents worked in, for the
/// Projects page; it redraws once they answer.
export const loadFound = single(async () => {
  try {
    state.found = await invoke<FoundRow[]>("found_projects");
  } catch (e) {
    console.warn("found projects:", e);
    state.found = state.found || [];
  }
  if (!state.picked) redrawProjects();
});

/// Imports a found row from the Projects page and opens it.
export async function importFromPage(row: FoundRow) {
  if (state.importing) return;
  state.importing = row.name;
  state.foundError = null;
  drawProjects(projectsEl());
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
  action: () => plusButton("Add a project", addProject),
  empty: () => emptyRow("No projects yet"),
  load: async () => {
    const rows = projectRows();
    // Those pinned to the top show under Pinned instead.
    return rows ? sideProjects(splitOnTop(rows, state.top).rest, state.groups, folded) : null;
  },
  row(p) {
    // A group's heading, under it the projects' whole names; an org's,
    // under it only the repo names.
    if (p.group) return foldable(groupHeading(p.group), p);
    if (p.org !== undefined) return foldable(h("button", "cc-org", `${p.org}/`), p);
    const row = projectRow(p, p.repo ?? p.name, false);
    if (!p.repo) row.classList.remove("pj-repo");
    return withThreads(row, p.name);
  },
  onPick: p => { if (!p.org && !p.group) openProjects(p.name); },
});
