import { projectName, foldKey, parseFolded, sideProjects, splitOnTop, type GroupHeading, type OrgHeading, type Placed, type Project } from "../../stores/project/projects-state.ts";
import { MACHINES } from "../../stores/machines/machine-store.ts";
import { WINDOWS, onPhone, isMobile } from "../../ui/platform.ts";
// The sidebar: its groups (pinned, views, projects, machines).
import { onClosing } from "../../stores/agents/closing.ts";
import { plural } from "../../ui/format.ts";
import { openPalette } from "./palette.tsx";
import { S, type View } from "../../stores/bar/core.ts";
import { openMenu, type MenuItem } from "../../ui/menu.tsx";
import { ALL } from "../../stores/terminals/terminal-list.ts";
import { state, stuck, type SidebarGroup, groups, listsChanged, sessionsChanged, syncChanged } from "../../stores/center/state.ts";
import { allTerms, termsChanged } from "../../parts/center/allterms.ts";
import { currentLocation, go, drawTop, aloneProject, windowAlone } from "../../parts/center/nav.ts";
import { fsync, syncProjectFollow } from "../../parts/project/sections.ts";
import { conflictList } from "../../parts/project/filesync.ts";
import { projectRail } from "../../screens/project/loads.tsx";
import { railSessions } from "../../parts/project/sessionsload.ts";
import { openAddMachine, openProjects, addProject } from "../../parts/project/addflow.ts";
import { projectRows } from "../../stores/project/projectrows.ts";
import { projectWindowProps, setProjectOnTop } from "../../parts/project/projectops.ts";
import { IC, Icon } from "../../ui/icons.tsx";
import type { MachineCard } from "../../parts/machines/machine-state.ts";
import { type ReactNode } from "react";
import { placeWindowParts } from "../../parts/center/window-root.tsx";
import { type Rail, railProject, setRail, shell } from "../../parts/center/window-shell.tsx";
import { chatRail, chatsAsking } from "../../stores/agents/threads-state.ts";
import { sessions, sessionsRead } from "../../stores/agents/sessions.ts";
import { sessionVisible } from "../../stores/agents/closing.ts";
import { ChatRail } from "../../screens/agents/assistant-page.tsx";
import { SidebarShell, SidebarItem, SidebarGroup as SidebarGroupView, type SideFooter, type SideLink } from "./sidebar-shell.tsx";
import { SidebarProjectRow } from "./sidebar-rows.tsx";
import { AG } from "../../stores/agents/agents-state.ts";
import { projectChanged } from "../../stores/project/barstate.ts";
import { useBuildCapabilities } from "../../ui/tauri.ts";
import { SidebarMachine } from "../../screens/machines/sidebar-machine-view.tsx";
import { jump } from "../../screens/center/home.tsx";
import { thisMacRow, type MacRow } from "../../parts/machines/machine-state.ts";
import { placeOf } from "../../stores/agents/place.ts";
import { newChatTarget } from "../../stores/project/agent.ts";
import { runningKey } from "../../stores/agents/running-state.ts";

function registerSidebarGroup<T>(name: string, group: SidebarGroup<T>) {
  groups.set(name, group as SidebarGroup<unknown>);
}

/// What the pane shows, for the sidebar's highlight; null off the pane.
export function paneOn(): { view: View | null } {
  return { view: currentLocation()?.view === "pane" ? S.view : null };
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
        if (p.group) { const grp = p.group; void import("../../screens/project/groups.tsx").then(m => m.groupMenu(native, grp, [foldItem(p)])); }
        else openMenu(native, p.org ?? "", [foldItem(p)]);
      }}>
      <span className={`twist${p.folded ? "" : " open"}`}><Icon name="twist" /></span>{label}<span className="tt">{title}</span>
    </button>;
  },

});

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

onClosing(() => drawSide());
listsChanged.follow(drawSide);
termsChanged.follow(drawSide);
syncChanged.follow(drawSide);

// This computer's row in the Machines group waits for the build to say it can host.
useBuildCapabilities.subscribe(drawSide);

// The project's sessions beside one of them or beside its page show its card.
sessionsChanged.follow(project => {
  if (project === null || railProject(shell.getState().side.rail) === project) drawRail();
});
projectChanged.follow(() => {
  if (currentLocation()?.view === "pane" || railProject(shell.getState().side.rail)) drawRail();
});

/// A sidebar item's key hint: none on a phone, whose keyboard rarely has the keys.
function shortcut(windows: string, apple: string): string | undefined {
  if (onPhone()) return undefined;

  return WINDOWS ? windows : apple;
}
