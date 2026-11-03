// The command center window: a sidebar, Home, and the bar's page as a pane
// (src/bar, loaded with this module) for chats, memories and routines.
import { harnessEl } from "./harnessicon.ts";
import { ago, plural, since } from "./components/format.ts";
import { dbtn as button, emptyPanel, errorNote, note, segmented, stateChip, svgEl as icon } from "./components/parts.ts";
import { invoke, Channel, listen } from "./tauri.ts";
import { splitPinned } from "./pinned.ts";
import { initPalette, openPalette } from "./palette.ts";
import { initSideResize } from "./sidewidth.ts";
import { connectProblem } from "./connect.ts";
import { pane, type PaneTarget } from "./bar/pane.ts";
import type { BackTo, PaneNow } from "./bar/core.ts";
import { withoutDismissed } from "./updates.ts";
import { seed, type CachedViews } from "./cached.ts";
import { WIDE_HOME_PX, base, homeColumns, homeRows, machineCaps, machineDisk, machineMemory, machineNow, machineOs, machineSync, needsLine, runningKey, runningAt, runningLine, byRecent, thisMacRow } from "./home.ts";
import type { HomeSection, HomeView, MacControl, MachineCard, MacRow, MachineSyncing, NeedsYou, RunningAgent, RunningRow, SyncRow, UpdateRow, CronRow } from "./home.ts";
import { openAddMachine as openAddMachineSheet } from "./addmachine.ts";
import { keepLastImport, lastImport, openImportProject } from "./importproject.ts";
import { openAddProject, foundRow, importFound, injectStyle as injectAddProjectStyle } from "./addproject.ts";
import { withSyncthing, mergeProjects, filterProjects, agentLabel, pastSessions, nextSessions, whereRows, whereCount, repoFromRemote, openOnLabel, projectLine, sideProjects, splitOnTop, overviewGroups, barSegments, countsLine, latestLine, workersLabel, overviewSub, groupMatches, removeProjectSheet, REMOVE_PROJECT, folderUnder, reason, el as h } from "./projects-state.ts";
import type { FoundRow, Kid, OverviewCard, OverviewCardRow, OverviewGroup, Placed, Project, ProjectGroup, ProjectRow, ProjectMachineRow, SessionRow, SyncthingPlace, WhereRow } from "./projects-state.ts";
import { openGroupSheet } from "./groupsheet.ts";
import { confirmSheet, openMenu, type MenuItem } from "./menu.ts";
import { NEW_WINDOW, opensNewWindow, startView, windowTitle } from "./projectwindow.ts";
import { ITEMS_SHOWN, itemChips, openTitle, projectsWithOpen, replyPrompt, reviewStart, repoUrl } from "./openitems.ts";
import { outsideThreads } from "./branchwork.ts";
import { agentFor, tasksSection, workersSection, cardsSection, chatProject, shortName, newChatProject, workerSession } from "./project.ts";
import { BAR_NOTE, type BarTarget, barCheckout, barPlace, barPrompt, sessionIn, targetCheckout, targetFolders, targetPlan } from "./projectbar.ts";
import { keepMachine, lastMachine } from "./bar/agentnew.ts";
import { type BaseBranches, baseFor, type CheckoutPick, DEFAULT_CHECKOUT, harnessFor, keepEffort, knownWorktrees, startPlace } from "./bar/agentpick.ts";
import { closedPicker, drawPicker, focusSearch, modelsOn, type Picked, pillInner, preloadModels } from "./bar/agentpicker.ts";
import { AGENT_ORDER, type AgentHost, type AgentSession, byPreference } from "./bar/agents.ts";
import { registerInstructions } from "./instructions.ts";
import { registerProviders } from "./providers.ts";
import { ALL, byMachine, makeTerminals, terminalNote, terminalTile, terminalWhere, terminalsCount } from "./terminals.ts";
import type { MachineTerminals, TerminalRow } from "./terminals.ts";
import { mountUsage, type UsageView } from "./usage.ts";
import { makeScreen } from "./termview.ts";
import { makeReviews, branchTile, mergeBlock } from "./review.ts";
import type { ReviewHead } from "./review.ts";
import { tile, backRow, taskTile, prTile, machineTile, agentTile, askingSessions } from "./board.ts";
import { makeFileSync, fileSyncTile } from "./filesync.ts";
import { makeThreads, issueTile, threadTile, issueBranch, issuePrompt, prPrompt } from "./threads.ts";
import type { IssueRow, PullRequestHead, PullRequestRow, RepoActs, ThreadChatRow } from "./threads.ts";
import { openLabel, runningChats, sideThreads, threadLabel, type ThreadChatEntry } from "./sidechats.ts";
import type { GitItemRow, GitItemsView } from "./openitems.ts";
import type { ProjectAgent, ProjectView, TaskRow, WorkerRow } from "./project.ts";

// ---------- What the app gives ----------

/// What `agent_resume` gives.
interface Started {
  session: SessionRow;
  why: string;
}

/// What the bar or tray asked the window to show (CenterTarget).
type CenterTarget = "home" | "chats" | "memories" | "routines" | "skills" | "apps" | "settings" | "instructions" | "providers"
  | { chat: { session: string | null } } | { group: { id: string } } | { agent: RunningAgent };

// ---------- Window state ----------

/// What a view's mount gets: a pane target, or the Projects page's pick.
type ViewArg = PaneTarget | ProjectsArg | undefined;
interface ProjectsArg { project: string | null }

/// A view's header: its title and line, buttons, and an element beside the title.
interface ViewHead { title: string; sub?: string; tools?: Node[]; scope?: Node }

export interface CenterView {
  el?: HTMLElement;
  title?: string | (() => ViewHead);
  mount(el: HTMLElement, arg: ViewArg): void;
  unmount?(): void;
}

/// How a Needs you card or a project card was answered.
interface Outcome { said?: string; error?: string; final?: boolean }

/// A chat or group in the sidebar, or a project pinned among them.
type ChatItem = Awaited<ReturnType<typeof pane.chatRows>>[number];
interface ProjectPin { kind: "project"; id: string; title: string; pinned: true; project: Placed<Project> }
type SideChat = ChatItem | ProjectPin;

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;


const IC = {
  home: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 11l9-7 9 7v9a1 1 0 01-1 1h-5v-6H9v6H4a1 1 0 01-1-1z"/></svg>',
  run: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M10 8l6 4-6 4z" fill="currentColor"/></svg>',
  cal: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/></svg>',
  chat: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>',
  mem: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z"/></svg>',
  find: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg>',
  loop: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M17 2l4 4-4 4"/><path d="M3 11V9a3 3 0 013-3h15M7 22l-4-4 4-4"/><path d="M21 13v2a3 3 0 01-3 3H3"/></svg>',
  plus: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
  gear: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"/></svg>',
  x: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  pin: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  group: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0113 0"/><circle cx="17" cy="9" r="2.5"/><path d="M16 14a5 5 0 016 5"/></svg>',
  doc: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>',
  pr: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="19" r="2.2"/><path d="M6 7.2v9.6M18 16.8V10a3 3 0 00-3-3h-4M13 4.5L10.5 7 13 9.5"/></svg>',
  issue: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/></svg>',
  rout: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>',
  term: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>',
  folder: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H10l2 2.5h7.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z"/></svg>',
};

const PROJECT_COLORS = ["#F07A6A", "#6FA8DC", "#B18CE0", "#E9B949", "#3B9B7A"];
const OFFLINE_NOTE_MS = 60 * 60 * 1000;
/// Before Home first loads: how long "Connecting" shows before "Still
/// connecting", and how soon a failed try is made again.
const CONNECT_STUCK_MS = 15000;
const CONNECT_RETRY_MS = 3000;

/// `since` is when the window started waiting for its first Home.
/// `picked` is the project the Projects page shows, null for all.
/// `found` is what `found_projects` last gave (null before), `importing` the
/// found row being imported and `foundError` why one failed, and
/// `sessions` each project's past sessions once read, by name.
/// `agents` lists the project agents (undefined before, null from a server
/// without them) and `pview` each one's tasks, workers and chat, by name.
/// `git` is what `git_items` last gave (null before, or from an old
/// server), and `gitAll` the projects whose open items show in full.
/// `groups` are the project groups (undefined before, null from a server
/// without them), `renaming` the group whose name is being edited, with
/// what is typed so far.
const state = {
  home: null as HomeView | null,
  mac: null as MacControl | null,
  error: null as string | null,
  outcomes: new Map<string, Outcome>(),
  since: Date.now(),
  picked: null as string | null,
  found: null as FoundRow[] | null,
  importing: null as string | null,
  foundError: null as { name: string; message: string } | null,
  sessions: new Map<string, SessionRow[]>(),
  where: new Map<string, ProjectMachineRow[] | null>(),
  agents: undefined as ProjectAgent[] | null | undefined,
  pview: new Map<string, { view: ProjectView | null; error: string | null }>(),
  sync: [] as SyncRow[],
  git: null as GitItemsView | null,
  gitAll: new Set<string>(),
  top: [] as string[],
  overview: undefined as OverviewCard[] | null | undefined,
  groups: undefined as ProjectGroup[] | null | undefined,
  renaming: null as { id: string; draft: string } | null,
};
/// Whether Home was last drawn in two columns.
let drawnWide = false;
/// The server answered for Home in this window; until then Home may be
/// the one the app kept from before.
let fresh = false;

// ---------- Formatting ----------

function whenOf(iso: string) {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return { time: "", day: "" };
  const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  const days = Math.round((new Date(at).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 86400000);
  const day = days <= 0 ? "today" : days === 1 ? "tomorrow" : at.toLocaleDateString([], { weekday: "short" });
  return { time, day };
}
/// A routine's schedule in words. The row already shows when it runs next,
/// so a one-off is just "Once".
function scheduleText(s: string) {
  if (/^\d{4}-\d\d-\d\dT/.test(s)) return "Once";
  return pane.when(s) ?? s;
}

/// Why a call failed, cut short; a conflict means another answer won.
function failure(e: unknown) {
  const text = String(e);
  return conflict(e) ? "Already answered" : text.length > 90 ? text.slice(0, 90) + "..." : text;
}
function conflict(e: unknown) { return /^conflict/i.test(String(e)); }
/// A refusal without its "bad request:" or "error:" prefix.
function refusal(e: unknown) { return String(e).replace(/^(bad request|error):\s*/i, ""); }

// ---------- Pieces ----------

/// Green online, yellow with agents running there, grey offline.
function dot(m: MachineCard) {
  const el = h("span", `ddot${!m.online ? " off" : m.agents ? " busy" : ""}`);
  el.title = !m.online ? "Offline" : m.agents ? `Online · ${plural(m.agents, "agent", "agents")} running` : "Online";
  return el;
}

/// A project's colour dot; `i` is its place in the project list, none
/// leaves it plain.
function projectDot(i?: number) {
  const d = h("span", "cc-pdot");
  if (i !== undefined) d.style.background = PROJECT_COLORS[i % PROJECT_COLORS.length];
  return d;
}

/// A Home section: its title and count, an optional button on the right,
/// then its body. `notes` follow the count in the title.
function section(id: string, title: string, count: number | string, action: Kid, body: Node, ...notes: Kid[]) {
  const el = h("div");
  el.id = id;
  const head = h("h4", "", title);
  if (count) head.append(h("span", "n", String(count)));
  for (const n of notes) if (n) head.append(n);
  el.append(h("div", "cc-sec", head, action), body);
  return el;
}
function avatar(cls: string, ini: Kid) { return h("span", `cc-ag ${cls}`, ini); }
function tag(cls: string, ...kids: Kid[]) { return h("span", `cc-tag${cls ? " " + cls : ""}`, ...kids); }


/// A button with a + before `label`.
function plusLabel(label: string, cls: string, on: () => void) {
  const b = button("", cls, on);
  b.append(icon(IC.plus)!, ` ${label}`);
  return b;
}

/// A button inside a row that does its own thing, not the row's.
function rowButton(label: string | Node, cls: string, on: () => void) {
  return button(label, cls, e => { e.stopPropagation(); on(); });
}

/// Placeholder rows of these widths, in percent, while a list loads.
function skeleton(widths: number[]) {
  return widths.map(w => {
    const row = h("div", "cc-skel");
    row.style.width = `${w}%`;
    return row;
  });
}

/// One row of Running now, Needs you, Updates or Coming up: a title and one
/// muted line, opening `open` on a click, with `right` beside it.
/// A row with no tile before its title.
function bare(row: HTMLElement) { row.classList.add("bare"); return row; }

function line(lead: Node | null, title: string, sub: string, right: Kid[], open: () => void) {
  const t = h("button", "t", h("b", "", title), h("span", "", sub));
  t.type = "button";
  t.title = title;
  const row = h("div", "cc-row cc-line", lead, t, h("span", "r", ...right));
  row.addEventListener("click", open);
  return row;
}

// ---------- Machines ----------

const addMachineButton = (cls: string) => plusLabel("Add a machine", cls, openAddMachine);

/// One machine per row: name and OS, agents, free disk, memory in use,
/// what it can run and what it is doing now.
function machineTable(machines: MachineCard[]) {
  const table = h("div", "cc-mt", ...["Machine", "Agents", "Disk", "Memory", "Can run", "Now"].map(t => h("span", "th", t)));
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
    table.append(name, agents, disk, memory, can, now);
    machineMenu(m, name, agents, disk, memory, can, now);
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
  return [name, st, now];
}

/// Turns on Let Beep use this Mac, so it joins as a machine.
async function useThisMac() {
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

/// Right-clicking `els` opens the menu for machine `m`, offline or not.
function machineMenu(m: Removable, ...els: Element[]) {
  for (const el of els) el.addEventListener("contextmenu", e => openMachineMenu(e as MouseEvent, m));
}

/// The menu for machine `m` where `e` was clicked; none for this Mac.
function openMachineMenu(e: MouseEvent, m: Removable) {
  if (!m.id || m.id === state.mac?.id) return;
  openMenu(e, m.name, [{ label: "Remove…", warn: true, on: () => removeMachine(m) }]);
}

/// Asks, then unlinks `m` from the account.
function removeMachine(m: Removable) {
  confirmSheet({
    title: `Remove ${m.name}?`,
    text: m.online === false
      ? "It is unlinked from your account. If it comes back online, it is turned away."
      : "It is unlinked from your account and stops taking work. Agents running there stop with it.",
    note: "Its files stay on the machine. To use it again, add it with a new setup command.",
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

function machinesSection(machines: MachineCard[]) {
  if (!machines.length && !thisMacRow(state.mac, machines)) {
    return section("sec-machines", "Machines", "", addMachineButton("small"),
      emptyPanel(h("b", "", "No machines linked yet"), addMachineButton("pri")));
  }
  const online = machines.filter(m => m.online).length;
  const count = machines.length ? `${online} of ${machines.length} online` : "";
  return section("sec-machines", "Machines", count, addMachineButton("small"), machineTable(machines));
}

function offlineNote(machines: MachineCard[]) {
  const gone = machines.filter(m => !m.online && Date.now() - m.seen_at < OFFLINE_NOTE_MS);
  if (!gone.length) return null;
  const names = gone.map(m => m.name).join(", ");
  return note("bad", h("b", "", `${names} ${gone.length === 1 ? "is" : "are"} offline.`), " Their sessions left Running now. They come back when beep-node runs there again.");
}

// ---------- Running now ----------

function runningRow(r: RunningRow) {
  const { title, sub } = runningLine(r, state.home?.projects || []);
  const right = [r.status === "asking" ? tag("wait", "needs you") : ago(runningAt(r))];
  const row = r.kind === "research"
    ? bare(line(null, title, sub, right, () => invoke("open_research", { session: r.session }).catch(() => {})))
    // The pane's agent view follows and answers it.
    : bare(line(null, title, sub, right, () => go("pane", { agent: r })));
  row.dataset.key = runningKey(r);
  return row;
}

/// Where each Running now row sat in its panel when last drawn, by key.
const runningTops = new Map<string, number>();
/// How long a row takes to slide to its new place.
const SLIDE_MS = 200;

/// Slides each row of a just drawn panel from where it sat last time to
/// where it sits now, so a session that moves to the top is seen moving.
/// Nothing slides when the system asks for less motion.
function slideRows(panel: HTMLElement) {
  requestAnimationFrame(() => {
    // A hidden panel (Home behind a project page) measures every row at 0
    // and would spoil the places the shown panel slides from.
    if (!panel.isConnected || !panel.getClientRects().length) return;
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const top = panel.getBoundingClientRect().top, seen = new Set<string>();
    for (const row of panel.querySelectorAll<HTMLElement>(":scope > [data-key]")) {
      const key = row.dataset.key!, now = row.getBoundingClientRect().top - top, was = runningTops.get(key);
      seen.add(key);
      runningTops.set(key, now);
      if (still || was === undefined || was === now) continue;
      row.animate([{ transform: `translateY(${was - now}px)` }, { transform: "none" }], { duration: SLIDE_MS, easing: "ease-out" });
    }
    for (const key of runningTops.keys()) if (!seen.has(key)) runningTops.delete(key);
  });
}

/// A panel of Running now rows, the one that moved last first.
function runningPanel(running: RunningRow[]) {
  const panel = h("div", "cc-panel", ...byRecent(running).map(runningRow));
  slideRows(panel);
  return panel;
}

function runningSection(running: RunningRow[]) {
  return section("sec-running", "Running now", running.length, null, runningPanel(running));
}

// ---------- Terminals ----------

/// Every terminal on every machine: the Terminals page, and Home's section.
const allTerms = makeTerminals({
  invoke, Channel, makeScreen, h, button, ago,
  redraw: () => {
    if (shown === "terminals") { drawTerminals(); drawTop(); }
    else if (shown === "home") draw();
    drawSide();
  },
});

/// The machines terminals are listed under, as Home has them.
const termMachines = () => (state.home?.machines || []).map(m => ({ id: m.id, name: m.name, online: m.online }));

/// Opens the Terminals page with terminal `id`, or the New terminal form on
/// machine `newOn`.
function openTerminals(to: { id: string } | { newOn: string | null } | null) {
  go("terminals");
  if (to && "id" in to) allTerms.show(to.id);
  else if (to) allTerms.newOn(to.newOn);
}

/// One machine's panel on Home: its name, how many run, New, then a row per
/// terminal.
function terminalsPanel(m: MachineTerminals<TerminalRow>) {
  const running = m.terms.filter(t => t.state === "running").length;
  const add = button("New", "small", e => { e.stopPropagation(); openTerminals({ newOn: m.id }); });
  add.disabled = !m.online;
  const head = h("div", "cc-row tl-mh", h("span", "t", h("b", "", m.name),
    h("span", "", !m.online ? "Offline" : m.terms.length ? `${running} running` : "No terminals")), h("span", "r", add));
  const rows = m.terms.map(t => {
    const row = h("button", "cc-row tl-row", h("span", `ddot${t.state === "running" && m.online ? "" : " off"}`),
      h("span", "t", h("b", "", t.name), h("span", "", terminalWhere(t))), h("span", "r", terminalNote(t, ago)));
    row.type = "button";
    row.title = `${t.machine_name}: ${t.command} in ${t.cwd}`;
    row.addEventListener("click", () => openTerminals({ id: t.id }));
    return row;
  });
  return h("div", "cc-panel tl-mp", head, ...rows);
}

/// Home's Terminals: a panel per machine that is online or has terminals.
function terminalsSection(list: TerminalRow[]) {
  const groups = byMachine(termMachines(), list).filter(m => m.online || m.terms.length);
  const all = button("All terminals", "quiet small", () => openTerminals(null));
  return section("sec-terminals", "Terminals", list.length ? terminalsCount(groups) : "", all,
    h("div", "cc-cols tl-home", ...groups.map(terminalsPanel)));
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

// ---------- Needs you ----------

type ChatNeed = Extract<NeedsYou, { kind: "chat" }>;
type AgentNeed = Extract<NeedsYou, { kind: "agent" }>;

function cardKey(n: NeedsYou) { return n.kind === "chat" ? `chat:${n.pending.query_id}` : `agent:${n.machine_id}:${n.session}:${n.question.id}`; }

function setOutcome(key: string, outcome: Outcome) { state.outcomes.set(key, outcome); draw(); }

function answerChat(n: ChatNeed, decision: string, said: string) {
  const key = cardKey(n);
  setOutcome(key, { said });
  // The call returns when the run it resumes ends, so the outcome shows now.
  invoke("answer_approval", { session: n.session_id, queryId: n.pending.query_id, decision, onEvent: new Channel() })
    .then(() => refresh())
    .catch(e => setOutcome(key, { error: failure(e), final: conflict(e) }));
}

/// Opens the session an agent's question came from, where it is answered.
function openAsking(n: AgentNeed) {
  const r = state.home?.running?.find((r): r is RunningAgent => r.kind === "agent" && r.machine_id === n.machine_id && r.id === n.session);
  go("pane", { agent: r || { kind: "agent", machine_id: n.machine_id, machine_name: n.machine_name, id: n.session, agent: n.agent, title: n.title, cwd: "", status: "asking", external: false } });
}

/// Allow answers a chat's approval from the row; the chat itself has the
/// details, Always allow and Deny. Answer opens the agent's session.
function needsRow(n: NeedsYou) {
  const out = state.outcomes.get(cardKey(n));
  const { title, sub, button: label } = needsLine(n);
  let lead, open, act;
  if (n.kind === "chat") {
    lead = avatar("beep", icon(IC.chat));
    open = () => go("pane", { chat: { session: n.session_id } });
    act = () => answerChat(n, "allow", "Allowed");
  } else {
    lead = harnessEl(n.agent);
    open = act = () => openAsking(n);
  }
  let right;
  if (out?.said) right = [h("span", "ok", out.said)];
  else if (out?.final) right = [h("span", "err", out.error)];
  else right = [rowButton(label, "pri small", act)];
  const row = line(lead, title, sub, right, open);
  if (out?.error && !out.final) row.querySelector(".t > span")!.replaceChildren(h("span", "err", out.error));
  if (out?.said) row.style.opacity = ".55";
  return row;
}

function needsSection(needs: NeedsYou[]) {
  return section("sec-needs", "Needs you", needs.length, null, h("div", "cc-panel", ...needs.map(needsRow)));
}

// ---------- Coming up ----------

function comingSection(rows: CronRow[]) {
  return section("sec-coming", "Coming up", rows.length, null, h("div", "cc-panel", ...rows.map(c => {
    const w = whenOf(c.next_run_at);
    return line(h("span", "cc-when", h("b", "", w.time), w.day), c.label.trim() || c.prompt.trim() || "Routine", scheduleText(c.schedule), [], () => go("pane", "routines"));
  })));
}

// ---------- Updates ----------

/// When an update came: "09:41" today, then "Yesterday", "Thu" this week, "3 Oct".
function updateTime(ms: number) {
  const at = new Date(ms);
  const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(ms).setHours(0, 0, 0, 0)) / 86400000);
  if (days <= 0) return at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  if (days === 1) return "Yesterday";
  if (days < 7) return at.toLocaleDateString([], { weekday: "short" });
  return at.toLocaleDateString([], { day: "numeric", month: "short" });
}

/// When a chat started, for its title line: "09:12" today, then as an
/// update's time reads.
function startedWhen(ms: number) {
  const t = updateTime(ms);
  return t === "Yesterday" ? "yesterday" : t;
}

/// Updates dismissed here that a list may still carry, by id.
const dismissedUpdates = new Set<number>();

/// Takes updates off Home at once and tells the server; a failed dismiss
/// brings them back on the next refresh.
function dismissUpdates(ids: number[]) {
  if (!ids.length) return;
  for (const id of ids) dismissedUpdates.add(id);
  if (state.home?.updates) state.home.updates = state.home.updates.filter(u => !ids.includes(u.id));
  draw();
  invoke("center_dismiss_updates", { ids }).catch(e => {
    console.warn("dismissing updates:", e);
    for (const id of ids) dismissedUpdates.delete(id);
    refresh();
  });
}

/// Starts a new chat with the update's summary in the composer, not sent.
async function openUpdate(u: UpdateRow) {
  dismissUpdates([u.id]);
  go("pane", { chat: { session: null } });
  try {
    await pane.draft(u.summary);
  } catch (e) {
    console.warn("opening an update:", e);
  }
}

function updateRow(u: UpdateRow) {
  const letter = (u.app_name || "").slice(0, 1).toUpperCase();
  const close = rowButton(icon(IC.x)!, "quiet", () => dismissUpdates([u.id]));
  close.setAttribute("aria-label", "Dismiss");
  close.style.padding = "2px 6px";
  const sub = u.app_name ? `${u.app_name} · ${updateTime(u.at)}` : updateTime(u.at);
  const row = line(avatar("beep", letter || icon(IC.chat)), u.summary, sub, [close], () => openUpdate(u));
  row.querySelector<HTMLElement>(".t")!.title = "Ask Beep about this";
  return row;
}

/// What the event triage agent found in the user's apps, apart from any
/// chat. Hidden while there is nothing.
function updatesSection(updates: UpdateRow[]) {
  const clear = button("Clear", "quiet small", () => dismissUpdates(updates.map(u => u.id)));
  return section("sec-updates", "Updates", updates.length, clear, h("div", "cc-panel", ...updates.map(updateRow)));
}

// ---------- Open PRs and issues ----------

function openUrl(url: string) {
  invoke("open_url", { url }).catch(e => console.warn("opening a link:", e));
}

/// What Review needs of a project: where it is checked out.
type ReviewProject = Parameters<typeof reviewStart>[1];

/// Review prefills Start a coding agent: in the project's folder on an
/// online machine when it is imported, else on Auto.
function reviewItem(item: GitItemRow, project: ReviewProject) {
  const start = reviewStart(item, project, state.home?.machines);
  go("pane", { agentnew: start });
}

/// Reply opens a new chat with Beep that asks for help with the item; Beep
/// reads it with its GitHub tools, and posting asks first.
async function replyItem(item: GitItemRow) {
  go("pane", { chat: { session: null } });
  try {
    await pane.compose(replyPrompt(item));
  } catch (e) {
    console.warn("starting a reply:", e);
  }
  reloadChats();
}

function openItemRow(item: GitItemRow, project: ReviewProject) {
  const kindIcon = icon(item.kind === "pull_request" ? IC.pr : IC.issue);
  const title = h("button", "t", h("span", "num", `#${item.number}`), h("b", "", item.title));
  title.type = "button";
  title.title = item.url;
  const right = [
    ...itemChips(item).map(c => tag(c.cls, c.text)),
    h("span", "age", ago(item.updated_at)),
    item.kind === "pull_request" ? rowButton("Review", "small", () => reviewItem(item, project)) : null,
    rowButton("Reply", "small", () => replyItem(item)),
  ];
  const row = h("div", "cc-row cc-line cc-gr it", h("span", "ic", kindIcon), title, h("span", "r", ...right));
  row.addEventListener("click", () => openUrl(item.url));
  return row;
}

/// A repository's head row among open items.
function repoLine(i: number | undefined, repo: string, sub: string, right: Kid[], open: () => void) {
  const head = line(projectDot(i), repo, sub, right, open);
  head.classList.add("cc-gr");
  return head;
}

type OpenRepo = ReturnType<typeof projectsWithOpen>[number];

/// One project's row, its counts and its newest items.
function openProjectRows(p: OpenRepo, i: number) {
  const counts = [p.prs ? plural(p.prs, "PR", "PRs") : null, p.issues ? plural(p.issues, "issue", "issues") : null]
    .filter(Boolean).map(t => h("span", "", t));
  const right = [...counts, p.waiting ? tag("wait", `${p.waiting} waiting on you`) : null, h("span", "age", ago(p.last))];
  const sub = p.project ? p.host : `${p.host} · not imported`;
  if (!p.project) right.push(rowImportButton(p));
  const head = repoLine(i, p.repo, sub, right, () => (p.project ? openProjects(p.project.name) : openUrl(p.url)));
  if (!p.project) {
    head.addEventListener("contextmenu", e => openMenu(e, p.repo, [
      { label: "Import…", on: () => quickImport(p) },
      { label: "Import with settings…", on: () => importWithSettings(p) },
    ]));
  }
  const all = state.gitAll.has(p.key);
  const rows = [head, ...(all ? p.items : p.items.slice(0, ITEMS_SHOWN)).map(item => openItemRow(item, p.project))];
  if (p.items.length > ITEMS_SHOWN) {
    const more = button(all ? "Show fewer" : `Show all ${p.items.length}`, "quiet small", () => {
      if (all) state.gitAll.delete(p.key); else state.gitAll.add(p.key);
      draw();
    });
    rows.push(h("div", "cc-gr it more", more));
  }
  return rows;
}

/// Repositories being imported from their row, by key, and why one failed.
const rowImports = new Map<string, "busy" | { error: string }>();

/// What `import_project` takes for an open-items repository: `org/repo`
/// on GitHub, its URL elsewhere.
function importRepo(p: OpenRepo) {
  return p.host === "github.com" ? p.repo : p.url;
}

/// The row's Import button, or how its import stands.
function rowImportButton(p: OpenRepo): HTMLElement {
  const now = rowImports.get(p.key);
  if (now === "busy") return h("span", "age", h("span", "dspin"), " Importing…");
  const b = rowButton("Import", "small", () => quickImport(p));
  b.title = importTitle(p);
  if (now) {
    const failed = tag("bad", "Import failed");
    failed.title = now.error;
    return h("span", "", failed, " ", b);
  }
  return b;
}

/// Where Import puts it: the last machine and folder used, else the server's
/// default on the first online machine.
function importPlace(p: OpenRepo) {
  const machines = (state.home?.machines || []).filter(m => m.online);
  const last = lastImport();
  const machine = machines.find(m => m.id === last?.machine) || machines[0] || null;
  const folder = machine && last?.root && machine.id === last.machine ? folderUnder(last.root, importRepo(p)) : null;
  return { machine, folder };
}

function importTitle(p: OpenRepo) {
  const { machine, folder } = importPlace(p);
  if (!machine) return "No machine is online; Import opens the sheet";
  return `Imports on ${machine.name}${folder ? `, into ${folder}` : ""}. Right-click for Import with settings…`;
}

/// Imports from the row with the last machine and folder; the row turns
/// into the project once it is in. With no machine online, the sheet.
async function quickImport(p: OpenRepo) {
  if (rowImports.get(p.key) === "busy") return;
  const { machine, folder } = importPlace(p);
  if (!machine) return importWithSettings(p);
  rowImports.set(p.key, "busy");
  draw();
  try {
    const project = await invoke<ProjectRow>("import_project", { repo: importRepo(p), machineId: machine.id, folder, worktrees: [] });
    keepLastImport(project);
    rowImports.delete(p.key);
    await refresh();
  } catch (e) {
    const error = reason(e);
    rowImports.set(p.key, { error });
    console.warn(`importing ${p.repo}:`, error);
  }
  draw();
}

/// The Import a project sheet for the row's repository, on the last
/// machine and folder used.
function importWithSettings(p: OpenRepo) {
  const { machine, folder } = importPlace(p);
  openImportProject({
    machines: state.home?.machines || [],
    macId: state.mac?.id || null,
    repo: importRepo(p),
    machine: machine?.id ?? null,
    folder,
    onDone: () => refresh(),
  });
}

/// Open PRs and issues on the user's git hosts, one row per project with
/// its items under it. A host that failed says why above the rows.
function gitSection(view: GitItemsView) {
  const projects = projectsWithOpen(view, projectRows() || []);
  const notes = view.hosts.filter(x => x.error).map(x =>
    note("bad", x.error));
  const body = projects.length
    ? projects.flatMap(openProjectRows)
    : [h("div", "cc-empty", h("span", "", notes.length ? "Nothing to show from the hosts that answered." : "No open pull requests or issues."))];
  const total = projects.reduce((n, p) => n + p.items.length, 0);
  const count = projects.length ? `${total} in ${plural(projects.length, "project", "projects")}` : "";
  return section("sec-git", openTitle(view.hosts), count, null, h("div", "cc-panel cc-open", ...notes, ...body));
}

/// `load` run so that a call while one runs is dropped.
function single(load: () => Promise<void>) {
  let running = false;
  return async () => {
    if (running) return;
    running = true;
    try {
      await load();
    } finally {
      running = false;
    }
  };
}

/// Asks the server for the open items; it keeps them a few minutes, so
/// this runs less often than Home's refresh.
const refreshGit = single(async () => {
  try {
    state.git = await invoke<GitItemsView | null>("git_items");
  } catch (e) {
    console.warn("open PRs and issues:", e);
  }
  if (shown === "home") draw();
});

// ---------- Views ----------

const views = new Map<string, CenterView & { el: HTMLElement }>();
let shown: string | null = null;
/// The argument `shown` was opened with, so Esc from an agent view can return to it.
let shownArg: ViewArg;

/// Adds a view the window can show. `mount(el, arg)` draws it into its own
/// element, `unmount()` runs when another view takes over. `title` is a
/// string or a function giving `{ title, sub, tools, scope }`, `scope` an
/// element shown beside the title.
export function registerView(name: string, view: CenterView) {
  let el = view.el;
  if (!el) {
    el = h("div", "cc-box");
    el.hidden = true;
    $("#main").append(el);
  }
  views.set(name, { ...view, el });
}

/// A pane target that opens an agent view, which can go back.
function opensAgent(arg: ViewArg): arg is Extract<PaneTarget, { agent: unknown } | { agentnew: unknown }> {
  return !!arg && typeof arg === "object" && ("agent" in arg || "agentnew" in arg);
}

/// Shows view `name`, passing `arg` to its mount. The others are hidden,
/// not destroyed, so a reply streaming in the pane keeps going under Home.
export function go(name: string, arg?: ViewArg) {
  // Settings open over whatever shows, which stays as it is.
  if (name === "pane" && arg === "settings") {
    if (!shown) go("home");
    return openSettings();
  }
  // An agent view opened from another view goes back there on Esc.
  if (name === "pane" && opensAgent(arg) && !arg.from && shown && shown !== "pane") {
    arg = { ...arg, from: { view: shown, arg: shown === "projects" ? { project: state.picked } : shownArg } };
  }
  shownArg = arg;
  const view = views.get(name);
  if (!view) return;
  if (shown && shown !== name) views.get(shown)!.unmount?.();
  shown = name;
  for (const [n, v] of views) v.el.hidden = n !== name;
  view.mount(view.el, arg);
  drawTop();
  drawSide();
}

function drawTop() {
  const view = shown ? views.get(shown) : undefined;
  const head: ViewHead = typeof view?.title === "function" ? view.title() : { title: view?.title ?? "Beep" };
  $("#title").textContent = head.title || "Beep";
  $("#scope").replaceChildren(...(head.scope ? [head.scope] : []));
  $("#date").textContent = head.sub || "";
  $("#tools").replaceChildren(...(head.tools || []));
}

// ---------- Sidebar ----------

/// A sidebar group. `load()` gives its items, or null to hide the group;
/// `row(item)` draws one; `onPick(item)` runs on a click; `onTitle()`,
/// when given, makes the title a button; `action()` adds a button beside
/// the title; `empty` is what an empty group says; `foot()` adds rows
/// after the items. Groups sort by `order`.
interface SidebarGroup<T> {
  order: number;
  title?: string;
  load(): Promise<T[] | null>;
  row(item: T): HTMLElement;
  onPick?(item: T): void;
  onTitle?(): void;
  action?(): HTMLElement;
  empty?: string | (() => string | HTMLElement);
  foot?(): HTMLElement[];
}

interface GroupSlot<T> extends SidebarGroup<T> {
  title: string;
  items: T[] | null;
  loading: boolean;
  again: boolean;
}

const groups = new Map<string, GroupSlot<unknown>>();

export function registerSidebarGroup<T>(name: string, group: SidebarGroup<T>) {
  const slot: GroupSlot<T> = { title: "", ...group, items: null, loading: false, again: false };
  groups.set(name, slot);
  reloadGroup(name);
}

/// Loads a group's items again. Never two loads at once: one asked for
/// during a load runs once more after it.
async function reloadGroup(name: string) {
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

function sideItem(svg: string | null, label: string, count: number | undefined, opts: SideOpts = {}) {
  const b = h("button", `cc-si${opts.cls ? " " + opts.cls : ""}`, svg ? icon(svg) : null, h("span", "tt", label));
  b.type = "button";
  if (opts.current) b.setAttribute("aria-current", "true");
  if (count) b.append(h("span", `n${opts.hot ? " hot" : ""}`, String(count)));
  if (opts.kbd) b.append(h("kbd", "", opts.kbd));
  if (opts.on) b.addEventListener("click", opts.on);
  return b;
}

/// Element Home scrolls to once it has drawn.
let scrollTarget: string | null = null;

function jump(id?: string) {
  go("home");
  const view = $("#view");
  if (!id) return view.scrollTo({ top: 0 });
  scrollTarget = id;
  // Home redraws when it mounts, so the target is also applied after that draw.
  applyScroll();
}

function applyScroll() {
  if (!scrollTarget) return;
  const el = document.getElementById(scrollTarget);
  if (el) { el.scrollIntoView({ block: "start" }); scrollTarget = null; }
}

/// What the pane shows, for the sidebar's highlight.
function paneOn(): { view: string | null; session: string | null; group: string | null } {
  return shown === "pane" ? pane.current() : { view: null, session: null, group: null };
}

function drawSide() {
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
      const h3 = h("h3", "", g.onTitle ? titleButton(g.title, g.onTitle) : g.title);
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

/// A group title that opens what the group lists.
function titleButton(title: string, on: () => void) {
  const b = h("button", "cc-gt", title);
  b.type = "button";
  b.addEventListener("click", on);
  return b;
}

function footer(machines: MachineCard[] | null | undefined): Kid[] {
  const p = problem();
  if (p) return [h("span", "", h("span", `ddot ${p.kind === "unlinked" ? "off" : "bad"}`), ` ${p.kind === "unlinked" ? "Not linked" : p.kind === "revoked" ? "Removed" : "Refused"}`)];
  if (!state.home) return [h("span", "", h("span", `ddot ${stuck() ? "bad" : "off"}`), stuck() ? " Not connected" : " Connecting…")];
  if (!machines) return ["Machines unavailable"];
  if (!machines.length) return ["No machines linked"];
  const on = machines.filter(m => m.online).length;
  return [h("span", "", h("span", "ddot"), " ", h("b", "", String(on)), ` of ${plural(machines.length, "machine", "machines")} online`)];
}

/// Pinned chats and groups, then the newest of the rest, the open one
/// highlighted.
const SIDE_CHATS = 5;
let chatList: ChatItem[] = [];
let chatFetch: Promise<ChatItem[]> | null = null;
/// The chats the server last said have a run going on, from any device.
let serverRunning: string[] = [];
/// The chats about one pull request or issue, by chat id, read for each
/// project; null until first read.
let threadChats: Map<string, ThreadChatEntry> | null = null;
let threadsLoading: Promise<void> | null = null;

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
function running() {
  return runningChats(pane.running(), serverRunning);
}

function reloadChats() {
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
async function setProjectOnTop(name: string, pinned: boolean) {
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
  const toggle = h("button", "pa", icon(IC.pin));
  toggle.type = "button";
  toggle.dataset.id = id;
  toggle.setAttribute("aria-label", pinned ? "Unpin" : "Pin to top");
  toggle.addEventListener("click", e => { e.stopPropagation(); on(); });
  return toggle;
}

/// Gives the pin button of chat `id` the focus again, unless focus has gone
/// elsewhere meanwhile (a redraw leaves it on the page body).
function restorePinFocus(id: string) {
  const on = document.activeElement;
  if (on && on !== document.body) return;
  document.querySelector<HTMLElement>(`.cc-chat .pa[data-id="${CSS.escape(id)}"]`)?.focus();
}

async function setPinned(c: SideChat, pinned: boolean) {
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
function openProjectWindow(name: string) {
  invoke("center_open_project_window", { project: name }).catch(e => console.warn("opening a project window:", e));
}

/// Lets project `name` open in a window of its own: ⌘-click or
/// middle-click on `clicks`, or "Open in New Window" from a right-click
/// on `menu`. A ⌘-click is caught first, so the row's own click does not
/// also run.
function newWindowOn(clicks: HTMLElement, menu: HTMLElement, name: string) {
  const open = (e: MouseEvent) => {
    if (!opensNewWindow(e)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    openProjectWindow(name);
  };
  clicks.addEventListener("click", open, true);
  clicks.addEventListener("auxclick", open);
  menu.addEventListener("contextmenu", e => openProjectMenu(e, name));
}

/// The right-click menu of project `name`: a window of its own, and
/// removing it once it is imported (one from running agents only is not
/// the server's to remove).
function openProjectMenu(e: MouseEvent, name: string) {
  const items: MenuItem[] = [{ label: NEW_WINDOW, on: () => openProjectWindow(name) }];
  if (isImported(name)) items.push({ label: REMOVE_PROJECT, warn: true, on: () => removeProject(name) });
  openMenu(e, name, items);
}

function isImported(name: string) {
  return !!projectRows()?.some(p => p.name === name && p.imported);
}

/// Asks, then removes project `name` from Beep. Its files stay on every
/// machine.
function removeProject(name: string) {
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
    return [...splitPinned(await fetchChats(), SIDE_CHATS).pinned, ...pinnedProjects()];
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
    const b = h("button", "", icon(IC.plus));
    b.type = "button";
    b.setAttribute("aria-label", "New chat");
    b.addEventListener("click", () => go("pane", { chat: { session: null } }));
    return b;
  },
  async load() {
    // Chats about one thread are listed under their project.
    const rows = (await fetchChats()).filter(c => c.pinned || !threadChats?.has(c.id));
    return splitPinned(rows, SIDE_CHATS).recent;
  },
  row: chatRow,
  onPick: pickChat,
});

registerSidebarGroup("views", {
  order: 6,
  title: "Beep",
  load: async () => [
    { view: "memories", label: "Memories", svg: IC.mem },
    { view: "routines", label: "Routines", svg: IC.loop },
    { view: "settings", label: "Settings", svg: IC.gear },
  ],
  row: v => sideItem(v.svg, v.label, 0, { current: v.view !== "settings" && paneOn().view === v.view, kbd: v.view === "settings" ? "⌘," : "" }),
  onPick: v => go("pane", v.view),
});

/// Imported projects merged with the folders agents run in.
function projectRows() {
  const home = state.home;
  if (!home || (!home.running && !home.projects)) return null;
  return mergeProjects(home.projects, home.running);
}

/// A small + button for a group's heading or the footer.
function plusButton(label: string, on: () => void) {
  const b = h("button", "", icon(IC.plus));
  b.type = "button";
  b.title = label;
  b.setAttribute("aria-label", label);
  b.addEventListener("click", e => { e.stopPropagation(); on(); });
  return b;
}

/// An empty group's row: what is missing. The group's + adds one.
function emptyRow(text: string) {
  return h("div", "cc-si none", text);
}

/// Opens the Add a machine sheet; Home refreshes once it closes.
export function openAddMachine() {
  openAddMachineSheet({ onClose: refresh });
}

/// Opens the Projects page, filtered to `name` when given.
export function openProjects(name?: string | null) {
  go("projects", { project: name || null });
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

/// Opens the Import a project sheet, at the folder choice with `choose`;
/// the project is selected once it is in. `repo` and `machine` fill it in,
/// as Clone here does.
function importSheet(choose = false, repo: string | null = null, machine: string | null = null) {
  openImportProject({
    machines: state.home?.machines || [],
    macId: state.mac?.id || null,
    choose,
    repo,
    machine,
    onDone: landed,
  });
}

/// Opens the Add a project sheet: found folders, a folder, or a clone.
export function addProject() {
  openAddProject({
    machines: state.home?.machines || [],
    onImported: landed,
    onChoose: () => importSheet(true),
    onClone: () => importSheet(false),
  });
}

/// Asks the machines for the checkouts their agents worked in, for the
/// Projects page; it redraws once they answer.
const loadFound = single(async () => {
  try {
    state.found = await invoke<FoundRow[]>("found_projects");
  } catch (e) {
    console.warn("found projects:", e);
    state.found = state.found || [];
  }
  if (!state.picked) redrawProjects();
});

/// Imports a found row from the Projects page and opens it.
async function importFromPage(row: FoundRow) {
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
    state.foundError = { name: row.name, message: refusal(e) };
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
    return rows ? sideProjects(splitOnTop(rows, state.top).rest, state.groups) : null;
  },
  row(p) {
    // A group's heading, under it the projects' whole names; an org's,
    // under it only the repo names.
    if (p.group) return groupHeading(p.group);
    if (p.org !== undefined) return h("div", "cc-org", `${p.org}/`);
    const row = projectRow(p, p.repo ?? p.name, false);
    if (!p.repo) row.classList.remove("pj-repo");
    return withThreads(row, p.name);
  },
  onPick: p => { if (!p.org && !p.group) openProjects(p.name); },
});

// ---------- Project groups ----------

/// A group's heading in the sidebar; right-click for its menu.
function groupHeading(g: ProjectGroup) {
  const head = h("div", "cc-org gp", icon(IC.folder), g.name);
  head.title = `${g.paths.join(", ")} on every machine`;
  head.addEventListener("contextmenu", e => groupMenu(e, g));
  return head;
}

function groupMenu(e: MouseEvent, g: ProjectGroup) {
  openMenu(e, g.name, [
    { label: "Rename…", on: () => startRename(g) },
    { label: "Folders…", on: () => editGroup(g) },
    { label: "Remove group", warn: true, on: () => removeGroup(g) },
  ]);
}

/// Opens the group sheet: a new group, or `g`'s folders.
function editGroup(g: ProjectGroup | null = null) {
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
function startRename(g: ProjectGroup) {
  state.renaming = { id: g.id, draft: g.name };
  if (shown !== "projects" || state.picked) openProjects();
  else redrawProjects();
}

async function renameGroup(g: ProjectGroup, name: string) {
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

async function removeGroup(g: ProjectGroup) {
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

// ---------- Home ----------

/// Machines across the top, then Running now beside Needs you and Updates
/// beside Coming up. On a wide window, two columns instead: Needs you,
/// Running now and Updates, beside Machines and Coming up. A section with
/// nothing in it is left out; Machines stays to offer Add a machine.
function page(home: HomeView, wide: boolean) {
  const { machines, running, coming_up: coming, needs_you: needs } = home;
  const out: HTMLElement[] = [];
  if (state.error) out.push(errorNote(`Could not refresh: ${state.error}`));
  if (!machines && !running) {
    out.push(note("off", "Machines and running sessions aren't available for this account or server."));
  } else if (machines) {
    const note = offlineNote(machines);
    if (note) out.push(note);
  }
  // Each is made only when its list is there to show.
  const make: Record<HomeSection, () => HTMLElement> = {
    machines: () => machinesSection(machines!),
    running: () => runningSection(running!),
    needs: () => needsSection(needs),
    updates: () => updatesSection(home.updates!),
    coming: () => comingSection(coming!),
    projects: () => projectsSection(projectRows() || []),
    git: () => gitSection(state.git!),
    terminals: () => terminalsSection(allTerms.list(ALL) || []),
  };
  const laid = { ...home, git: state.git, terminals: allTerms.list(ALL) };
  const cols = wide && homeColumns(laid);
  if (cols) {
    const col = (keys: HomeSection[]) => h("div", "", ...keys.map(k => make[k]()));
    out.push(h("div", "cc-wide", col(cols.left), col(cols.right)));
    return out;
  }
  if (machines) out.push(make.machines());
  for (const row of homeRows(laid)) {
    out.push(row.length === 2 ? h("div", "cc-cols", ...row.map(k => make[k]())) : make[row[0]]());
  }
  return out;
}

// ---------- Connecting ----------

/// True once the first Home has been waited for too long.
function stuck() { return !state.home && Date.now() - state.since >= CONNECT_STUCK_MS; }

/// Before Home first loads, a failure waiting will not fix, or null.
function problem() { return fresh ? null : connectProblem(state.error); }

const logButton = () => button("Show log", "", () => invoke("open_log").catch(e => console.warn("opening the log:", e)));

/// A button that asks for Home now, and stays off until the answer.
function retryButton(label: string, cls: string) {
  const retry = button(label, cls, () => { refresh(); retry.disabled = true; });
  retry.disabled = loading;
  return retry;
}

/// What "Still connecting" says, with how long the server has been silent.
function waitText() {
  const secs = Math.round((Date.now() - state.since) / 1000);
  return `The server has not answered for ${secs} seconds. Beep keeps trying every few seconds; your chats show up as soon as it answers.`;
}

/// What Home shows before the server first answers: pulsing dots, never a
/// blank page. Once stuck it says so and offers Retry; retries go on anyway.
/// A failure waiting will not fix is said plainly, with the way to fix it
/// and a Retry.
function connecting() {
  const p = problem();
  if (p) {
    const acts = h("div", "acts");
    if (p.action) acts.append(button(p.action, "pri", () => go("pane", p.view)));
    const retry = retryButton("Try again", p.action ? "" : "pri");
    retry.classList.add("retry");
    acts.append(retry, logButton());
    const box = h("div", "cc-conn stuck", h("span", `ddot ${p.kind === "unlinked" ? "off" : "bad"}`), h("b", "", p.title), h("span", "why", p.text), acts);
    box.dataset.problem = `${p.kind}:${p.text}`;
    return box;
  }
  const pulse = h("div", "cc-pulse", h("i"), h("i"), h("i"));
  if (!stuck()) {
    return h("div", "cc-conn", pulse, h("b", "", "Connecting to Beep…"), h("span", "", "Your chats, machines and approvals load once the server answers."));
  }
  return h("div", "cc-conn stuck", pulse, h("b", "", "Still connecting to Beep"), h("span", "why", waitText()), h("div", "acts", retryButton("Retry now", "pri"), logButton()));
}

/// Draws the waiting view, or brings the one on screen up to date so its
/// buttons keep their focus.
function drawConnecting(view: HTMLElement) {
  const on = view.querySelector<HTMLElement>(":scope > .cc-conn");
  const p = problem(), was = on?.dataset.problem;
  if (p || was) {
    if (p && was === `${p.kind}:${p.text}`) {
      on!.querySelector<HTMLButtonElement>(".acts .retry")!.disabled = loading;
      return;
    }
    return view.replaceChildren(connecting());
  }
  if (on && on.classList.contains("stuck") === stuck()) {
    if (!stuck()) return;
    on.querySelector(".why")!.textContent = waitText();
    on.querySelector<HTMLButtonElement>(".acts .pri")!.disabled = loading;
    return;
  }
  view.replaceChildren(connecting());
}

function draw() {
  const home = state.home, view = $("#view");
  drawSide();
  if (!home || problem()) return drawConnecting(view);
  const top = view.scrollTop;
  drawnWide = isWide(view);
  view.replaceChildren(...page(home, drawnWide));
  view.scrollTop = top;
  applyScroll();
  scrollTarget = null;
  $<HTMLInputElement>("#home-ask").placeholder = home.machines?.length ? "Ask Beep, or start an agent on a machine" : "Ask Beep anything";
  $("#home-agent").style.display = home.machines?.some(m => m.online) ? "" : "none";
}

/// Whether Home has room for two columns.
function isWide(view: HTMLElement) { return view.clientWidth >= WIDE_HOME_PX; }
// Redraws Home when the window crosses the width for two columns, including
// when Home is shown again after a pane (it measures 0 while hidden).
new ResizeObserver(() => {
  if (state.home && isWide($("#view")) !== drawnWide) draw();
}).observe($("#view"));

registerView("home", {
  el: $("#home-box"),
  title: () => ({ title: "Home", sub: new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" }) }),
  mount() { refresh(); refreshGit(); },
});

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

// ---------- Projects ----------

const importButton = (cls: string) => plusLabel("Add a project", cls, addProject);

/// One project per row: its name and host, where it is on each machine,
/// the agents running in it and when one last did something.
function projectTable(rows: Project[]) {
  const table = h("div", "cc-mt cc-pt", ...["Project", "Machine and folder", "Agents", "Last activity"].map(t => h("span", "th", t)));
  for (const p of rows) {
    const open = h("button", "", h("span", "", p.name));
    open.type = "button";
    open.title = p.url || p.name;
    open.addEventListener("click", () => openProjects(p.name));
    const name = h("span", "nm", h("b", "", open), h("span", "os", p.imported ? hostOf(p.url) : "From running agents"));
    name.id = `p-${p.name}`;
    newWindowOn(open, name, p.name);
    const where = h("span", "nm", ...p.machines.map(m => {
      const one = h("span", "os", h("b", "", m.name), ` ${m.folder}`);
      one.title = `${m.name}: ${m.folder}`;
      return one;
    }));
    if (!p.machines.length) where.append(h("span", "os", "–"));
    const last = h("span", "num", p.last ? ago(p.last) : "–");
    table.append(name, where, h("span", "num", p.agents ? String(p.agents) : "–"), last);
  }
  return table;
}

/// Every project as a card, grouped by org: its tasks per state, its
/// workers, what changed last and where it lives.
function overviewSection(rows: Project[]) {
  const groups = overviewGroups(rows, state.overview, state.top, state.groups);
  const out = groups.map(g => g.group ? groupSection(g.group, g)
    : h("section", "pj-org",
      h("h3", "", g.org ? `${g.org}/` : "Other folders", h("small", "", plural(g.cards.length, "repo", "repos"))),
      h("div", "pj-grid", ...g.cards.map(projectCard))));
  const add = h("div", "cc-panel cc-row pj-add",
    h("span", "t", h("b", "", "Add a project"), h("span", "", "A folder your agents worked in, any folder, or a repository to clone.")),
    button("Add", "", addProject));
  return [...out, add];
}

/// A group's section of the overview: its name (or the field renaming it),
/// how many repos, its folders and its buttons, then its cards.
function groupSection(g: ProjectGroup, section: OverviewGroup<Project, OverviewCard>) {
  const n = section.cards.length;
  const head = h("h3", "gp", icon(IC.folder));
  const editing = state.renaming?.id === g.id ? state.renaming : null;
  if (editing) {
    // What is typed lives in `state.renaming`, so a refresh that draws the
    // page again keeps it.
    const input = h("input");
    input.value = editing.draft;
    input.setAttribute("aria-label", "Group name");
    input.addEventListener("input", () => { editing.draft = input.value; });
    const finish = (save: boolean) => {
      if (state.renaming !== editing) return;
      if (save) renameGroup(g, input.value);
      else { state.renaming = null; redrawProjects(); }
    };
    input.addEventListener("keydown", e => {
      if (e.key === "Enter") { e.preventDefault(); finish(true); }
      else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); }
    });
    head.append(input, h("span", "gp-acts", button("Save", "pri", () => finish(true)), button("Cancel", "quiet", () => finish(false))));
    // Focused once it is on the page.
    requestAnimationFrame(() => {
      if (!input.isConnected || document.activeElement === input) return;
      input.focus();
      input.setSelectionRange(input.value.length, input.value.length);
    });
  } else {
    const paths = h("span", "gp-paths", g.paths.map(p => (p.startsWith("/") ? p : `~/${p}`)).join(", "));
    paths.title = "Matched on every machine";
    head.append(g.name, h("small", "", plural(n, "repo", "repos")), paths,
      h("span", "gp-acts",
        button("Rename", "quiet", () => startRename(g)),
        button("Folders…", "quiet", () => editGroup(g)),
        button("Remove", "quiet warn", () => removeGroup(g))));
    head.addEventListener("contextmenu", e => groupMenu(e, g));
  }
  const body = n ? h("div", "pj-grid", ...section.cards.map(projectCard)) : h("span", "gp-none", "No projects in these folders yet.");
  return h("section", "pj-org", head, body);
}

function projectCard(c: OverviewCardRow<Project, OverviewCard>) {
  const pin = h("button", `pa${c.pinned ? " on" : ""}`, icon(IC.pin));
  pin.type = "button";
  pin.setAttribute("aria-label", c.pinned ? "Unpin" : "Pin to top");
  pin.addEventListener("click", e => { e.stopPropagation(); setProjectOnTop(c.name, !c.pinned); });
  const repo = h("b", "", c.repo);
  repo.title = c.name;
  const card = h("div", "pj-card", h("div", "h", projectDot(c.i), repo, pin, h("small", "", c.sum ? workersLabel(c.sum.workers) : (c.agents ? plural(c.agents, "agent", "agents") : "idle"))));
  card.tabIndex = 0;
  card.setAttribute("role", "button");
  card.title = c.name;
  if (c.sum) {
    const bar = h("div", "pj-bar");
    for (const s of barSegments(c.sum.tasks)) {
      const seg = h("i", s.state);
      seg.style.width = `${s.share}%`;
      seg.title = `${s.n} ${s.state}`;
      bar.append(seg);
    }
    const [open, ...rest] = countsLine(c.sum.tasks);
    card.append(bar, h("span", "line", h("b", "", open), rest.map(x => ` · ${x}`).join("")));
    const latest = c.sum.latest;
    card.append(h("span", "line", latestLine(latest), latest ? h("small", "", ` · ${ago(latest.updated_at)}`) : null));
  } else {
    card.append(h("span", "line", c.imported ? "No project agent yet" : "From running agents"));
  }
  const on = c.machines.map(m => m.name);
  card.append(h("span", "line", on.length ? `On ${[...new Set(on)].join(", ")}` : "On no machine yet"));
  const open = () => openProjects(c.name);
  newWindowOn(card, card, c.name);
  card.addEventListener("click", open);
  card.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(); } });
  return card;
}

/// The overview's cards; `null` from a server too old to sum projects up,
/// when the page keeps its table.
const loadOverview = single(async () => {
  try {
    state.overview = await invoke<OverviewCard[] | null>("project_overview");
  } catch (e) {
    console.warn("project overview:", e);
  }
  if (!state.picked) redrawProjectsPage();
});

/// One project: where it lives on every machine, with "Start an agent here"
/// per folder and "Clone here" where it is not, and the coding agents
/// running in it now.
function projectDetail(p: Project) {
  const machines = p.imported ? state.where.get(p.name) : null;
  if (p.imported && machines === undefined) loadWhere(p.name);
  const rows = whereRows(p, machines || null, state.mac?.id || null);
  const inside = (r: RunningAgent, m: { id: string; folder: string }) => r.machine_id === m.id && (r.cwd === m.folder || r.cwd?.startsWith(m.folder + "/"));
  const running = (state.home?.running || []).filter((r): r is RunningAgent => r.kind === "agent" && p.machines.some(m => inside(r, m)));
  if (boardOn(p)) return projectBoard(p, rows, !!machines, running);
  return [...agentTop(p), ...repoSections(p), h("div", "pj-cols pj-half", sessionsSection(p, running), agentsSection(running)), whereSection(p, rows, !!machines)];
}

/// "Where it lives": every machine with the project's folder there, or why
/// there is none.
function whereSection(p: Project, rows: WhereRow[], known: boolean) {
  const where = h("div", "cc-mt cc-pt", ...["Machine", "Folder", ""].map(t => h("span", "th", t)));
  where.style.gridTemplateColumns = "minmax(0,1fr) minmax(0,2fr) auto";
  for (const m of rows) {
    const cells = whereRow(p, m);
    where.append(...cells);
    if (m.id) machineMenu({ id: m.id, name: m.name, online: m.kind !== "offline" }, ...cells);
  }
  // Once every machine answered, the note says the count instead.
  return section("sec-where", "Where it lives", known ? "" : rows.filter(m => m.kind === "here").length, null, where,
    known && h("small", "", `on ${whereCount(rows, true)}`));
}

/// The coding agents running in the project's folders now.
function agentsSection(running: RunningAgent[]) {
  const list = running.length
    ? runningPanel(running)
    : emptyPanel(h("span", "", "No agent is working in this project right now."));
  return section("sec-project-agents", "Agents", running.length || "", null, list);
}

// ---------- Project board ----------

/// How a project with an agent shows: "board" or "list", kept for next time.
const LAYOUT_KEY = "beep.center.projectLayout";
let layout = "board";
try { layout = localStorage.getItem(LAYOUT_KEY) === "list" ? "list" : "board"; } catch {}

function setLayout(to: string) {
  layout = to;
  openTile(null, false);
  try { localStorage.setItem(LAYOUT_KEY, to); } catch {}
  redrawProjects();
  drawTop();
}

/// The board shows for an imported project whose agent's view has loaded.
function boardOn(p: Project) {
  return layout === "board" && p.imported && !!agentFor(state.agents, p.name) && !!state.pview.get(p.name)?.view;
}

/// Opens tile `key` in place of the board, or goes back with null, and
/// stops the polling of every other tile. With `draw` false the caller
/// draws.
function openTile(key: string | null, draw = true) {
  pa.tile = key;
  if (key !== "terminals") terms.stop();
  if (key !== "sync") fsync.stop();
  if (key !== "review") revs.stop();
  if (!draw) return;
  redrawProjects();
  projectsEl().querySelector(":scope > .cc-view")?.scrollTo({ top: 0 });
}

/// Opens the Tasks list and brings task `id`'s row into view.
function openTask(id: string) {
  openTile("tasks");
  const row = [...document.querySelectorAll<HTMLElement>("#main .pj-task")].find(r => r.dataset.task === id);
  if (!row) return;
  row.classList.add("found");
  row.scrollIntoView({ block: "center" });
}

/// The board: what the project chat waits on, then a tile per kind of
/// thing, or one tile's full list once its header is clicked.
function projectBoard(p: Project, rows: WhereRow[], known: boolean, running: RunningAgent[]) {
  const view = state.pview.get(p.name)!.view!;
  const agent = agentFor(state.agents, p.name)!;
  const sessions = state.sessions.get(p.name);
  if (sessions === undefined || sessionsDue.has(p.name)) loadSessions(p.name);
  const out = agentNotes(p, view);
  const full: Record<string, () => Node[]> = {
    tasks: () => [taskColumns(view)],
    prs: () => [thr.prsSection(agent, repoActs(p)), ...thr.threadsPanel(agent)],
    issues: () => [thr.issuesSection(agent, repoActs(p))],
    machines: () => [whereSection(p, rows, known)],
    agents: () => [agentsSection(running), sessionsSection(p, running)],
    terminals: () => terms.panel(p.name, rows),
    sync: () => fsync.panel(p.name),
    branches: () => revs.branchesPanel(p.name),
    review: () => revs.reviewPanel(p.name),
  };
  if (pa.tile && full[pa.tile]) {
    out.push(backRow(() => openTile(null)), ...full[pa.tile]());
    return out;
  }
  const past = sessions ? pastSessions(sessions, running) : [];
  if (terms.list(p.name) === undefined) terms.load(p.name);
  if (revs.branches(p.name) === undefined) revs.load(p.name);
  if (thr.issues(agent.project_id) === undefined) thr.load(agent.project_id);
  // Tiles in sections, two a row: the work, the code host, the machines.
  const tileGroup = (title: string, ...tiles: Node[]) => h("div", "pb-sec", h("h4", "", title), h("div", "pb-grid", ...tiles));
  out.push(
    tileGroup("Work",
      tile("tasks", "Tasks", taskTile(view.tasks, view.workers, askingSessions(running, state.home?.needs_you)), openTile, { open: r => openTask(r.id!) }),
      tile("agents", "Agents", agentTile(running, past), openTile),
      tile("branches", "Branches", branchTile(revs.branches(p.name), revs.reviews(p.name), ago), openTile)),
    tileGroup("Pull requests and issues",
      tile("prs", "Pull requests", threadTile(thr.threads(agent.project_id), prTile(view.tasks), thr.prs(agent.project_id)), openTile, { openUrl }),
      tile("issues", "Issues", issueTile(thr.issues(agent.project_id), thr.issuesError(agent.project_id)), openTile, { openUrl })),
    tileGroup("Machines",
      tile("machines", "Machines", machineTile(rows, state.sync.filter(s => s.project === p.name)), openTile, {
        menu: (r, e) => {
          const m = rows.find(w => w.id === r.id);
          if (m?.id) openMachineMenu(e, { id: m.id, name: m.name, online: m.kind !== "offline" });
        },
      }),
      tile("terminals", "Terminals", terminalTile(terms.list(p.name), ago), openTile),
      tile("sync", "File sync", fileSyncTile(fsync.forTile(p.name), state.sync.filter(s => s.project === p.name), new Map(rows.map(w => [w.id, w.name]))), openTile)));
  return out;
}

/// What the project chat's cards do, on the board and in the list.
function chatActs(p: Project) {
  const agent = agentFor(state.agents, p.name);
  return { ...agentActs, answer: (c: { query_id: string }, decision: string, said: string) => answerCard(agent!, c, decision, said) };
}

/// Above the board or the list: why the project did not refresh, then the
/// cards the project chat waits on.
function agentNotes(p: Project, view: ProjectView): Node[] {
  const out: Node[] = [];
  const error = state.pview.get(p.name)?.error;
  if (error) out.push(errorNote(`Could not refresh the project: ${error}`));
  const needs = cardsSection(view, chatActs(p));
  if (needs) out.push(needs);
  return out;
}

/// Tasks beside Workers.
function taskColumns(view: ProjectView) {
  return h("div", "pj-cols", tasksSection(view, agentActs), workersSection(view, agentActs, IC.chat));
}

/// The Board and List switch in the header of a project with an agent.
function layoutSwitch() {
  return segmented([{ label: "Board", value: "board" }, { label: "List", value: "list" }], layout, key => { if (layout !== key) setLayout(key); });
}

// ---------- Project agent ----------

/// What the agent sections are doing: `busy` and `errors` by task or worker
/// id, `outcomes` of answered cards by query id, and `starting` the project
/// whose agent the prompt bar is making.
/// `tile` is the board tile opened in place of the board, null for the board.
const pa = { busy: new Set<string>(), errors: new Map<string, string>(), outcomes: new Map<string, Outcome>(), tile: null as string | null };

/// The project's terminals; the board's tile and its full view.
const terms = makeTerminals({ invoke, Channel, makeScreen, h, button, ago, redraw: () => redrawProjects() });

/// The project's file sync; the board's tile and its full view.
const fsync = makeFileSync({ invoke, h, button, redraw: () => redrawProjects() });

/// The project's branches and their reviews; the Branches tile, its full
/// list and the review view.
const revs = makeReviews({
  invoke, h, button, ago,
  redraw: () => redrawProjects(),
  openUrl,
  openTile: (key: string) => openTile(key),
  machines: () => state.home?.machines || [],
});

/// The project's open issues and the user's threads there; the Issues
/// tile, the rows of the Pull requests tile and their full lists.
const thr = makeThreads({
  invoke, h, button, ago,
  redraw: () => redrawProjects(),
  openUrl,
  openChat: (chat: string) => { go("pane", { chat: { session: chat } }); reloadChats(); if (!threadChats?.has(chat)) loadThreadChats(); },
  changed: () => loadProjectAgent(),
});

/// The Projects page's element.
function projectsEl() { return views.get("projects")!.el; }

function redrawProjects() {
  if (shown === "projects") drawProjects(projectsEl());
}

/// The Projects page and its header, while it shows.
function redrawProjectsPage() {
  if (shown === "projects") { drawProjects(projectsEl()); drawTop(); }
}

const agentActs = {
  busy: (id: string) => pa.busy.has(id),
  error: (id: string) => pa.errors.get(id),
  outcome: (id: string) => pa.outcomes.get(id),
  openUrl,
  review: (branch: string) => { if (state.picked) revs.openBranch(state.picked, branch); },
  /// Merge… on a task in review asks the project agent to merge once the
  /// branch's newest review allows it and the branch has not moved since;
  /// otherwise it opens the review, which says what stands in the way.
  merge(task: { id: string; branch: string | null }) {
    const name = state.picked, branch = task.branch;
    if (!name || !branch) return;
    const r = (revs.reviews(name) || []).find(x => x.branch === branch);
    if (!r || !mergeBlock(r).startsWith("Merge always")) { revs.openBranch(name, branch); return; }
    return act(task.id, async () => {
      const now = await invoke<ReviewHead | null>("review_check", { id: r.id });
      if (!now || now.stale) { revs.openBranch(name, branch); return; }
      await invoke("review_merge", { review: r.id });
    });
  },
  openWorker(worker: Parameters<typeof workerSession>[0], title: string | null) {
    const at = workerSession(worker, title);
    if (at) go("pane", { agent: at });
  },
  async change(task: { id: string }, change: unknown) {
    await act(task.id, () => invoke("center_update_task", { task: task.id, change }));
  },
  async stop(worker: { id: string }) {
    await act(worker.id, () => invoke("center_stop_worker", { worker: worker.id }));
  },
};

/// Runs `call` for row `id`, showing it busy, then reloads the project.
async function act(id: string, call: () => Promise<unknown>) {
  if (pa.busy.has(id)) return;
  pa.busy.add(id);
  pa.errors.delete(id);
  redrawProjects();
  try {
    await call();
  } catch (e) {
    pa.errors.set(id, failure(e));
  } finally {
    pa.busy.delete(id);
  }
  redrawProjects();
  loadProjectAgent();
}

/// What the Pull requests and Issues sections do on project `p`'s page:
/// Review opens a branch's review, Work on it and Continue fill the bar.
function repoActs(p: Project): RepoActs {
  const view = state.pview.get(p.name)?.view;
  if (revs.reviews(p.name) === undefined) revs.load(p.name);
  return {
    tasks: view?.tasks ?? [],
    reviews: revs.reviews(p.name),
    review: agentActs.review,
    workOn: (issue: IssueRow) => workOnIssue(p.name, issue),
    continuePr: (pr: PullRequestRow) => continuePr(p.name, pr),
  };
}

/// The repository's open pull requests and issues, under the tasks of a
/// project with an agent.
function repoSections(p: Project): Node[] {
  const agent = p.imported ? agentFor(state.agents, p.name) : null;
  if (!agent) return [];
  const acts = repoActs(p);
  return [h("div", "pj-cols pj-half", thr.prsSection(agent, acts), thr.issuesSection(agent, acts))];
}

/// The cards the project chat waits on, then Tasks beside Workers, above
/// the folders, once the project has an agent.
function agentTop(p: Project) {
  const view = p.imported ? state.pview.get(p.name)?.view : null;
  if (!view) return [];
  return [...agentNotes(p, view), taskColumns(view)];
}

/// Answers a card of the project chat. The call returns when the run it
/// resumes ends, so the outcome shows now and the view reloads after.
function answerCard(agent: ProjectAgent, c: { query_id: string }, decision: string, said: string) {
  pa.outcomes.set(c.query_id, { said });
  redrawProjects();
  invoke("answer_approval", { session: agent.chat_id, queryId: c.query_id, decision, onEvent: new Channel() })
    .catch(e => pa.outcomes.set(c.query_id, { error: failure(e) }))
    .finally(() => { redrawProjects(); loadProjectAgent(); });
  // The tasks a plan starts show before its run ends.
  setTimeout(loadProjectAgent, 1500);
}

/// The bar under a project: it starts a coding agent in the project's
/// checkout, by default as it is (the agent makes its own branch), and
/// opens its session. Kept across redraws so what is typed stays.
const pc = {
  project: null as string | null,
  machine: "auto",
  checkout: DEFAULT_CHECKOUT,
  hosts: null as AgentHost[] | null,
  sessions: [] as AgentSession[],
  branches: new Map<string, BaseBranches | null>(),
  starting: false,
  /// The harness, model and thinking picked, and the machine and harness
  /// the model was picked for, as `machine/agent`.
  picked: { agent: "", model: "", effort: null, query: "" } as Picked,
  modelFor: "",
  /// Whether the picker shows.
  open: false,
  /// What Work on it or Continue put in the bar, until it starts or is
  /// cleared.
  target: null as BarTarget | null,
};

const projectComp = (() => {
  const input = h("input");
  input.type = "text";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", "Task for a coding agent");
  const machine = h("button", "pill");
  machine.type = "button";
  machine.title = "The machine it runs on";
  const folder = h("button", "pill");
  folder.type = "button";
  folder.title = "The checkout, a new worktree, or an existing worktree";
  const harness = h("button", "pill");
  harness.type = "button";
  harness.title = "Harness, model and thinking";
  const pick = h("div", "pick");
  const send = h("button", "dbtn pri", "Start", h("kbd", "", "⏎"));
  send.type = "submit";
  // Work on it's branch, editable, and Clear for what Work on it or
  // Continue put in the bar.
  const branch = h("input", "branch");
  branch.type = "text";
  branch.spellcheck = false;
  branch.autocomplete = "off";
  branch.setAttribute("aria-label", "Branch name");
  branch.title = "The new branch, in a new worktree";
  branch.addEventListener("input", () => { if (pc.target?.kind === "issue") pc.target.branch = branch.value; });
  const clear = h("button", "dbtn quiet small", "Clear");
  clear.type = "button";
  clear.title = "Back to a plain start";
  clear.addEventListener("click", () => { pc.target = null; input.value = ""; redrawProjects(); });
  // With a session already in the target's worktree, Resume goes first.
  const fresh = h("button", "dbtn", "Start new");
  fresh.type = "button";
  fresh.addEventListener("click", () => startFromBar(true));
  const err = h("span", "cc-err");
  err.setAttribute("role", "status");
  const row = h("div", "row", machine, folder, branch, clear, h("span", "sp"), harness, fresh, send);
  const form = h("form", "cc-pjc", h("div", "box", input, row, pick), h("span", "note", BAR_NOTE), err);
  form.addEventListener("submit", e => { e.preventDefault(); startFromBar(); });
  machine.addEventListener("click", e => {
    const p = barProject();
    if (!p) return;
    const able = (pc.hosts ?? []).filter(m => m.agents.length && p.places.some(x => x.machine === m.machine_id));
    openMenu(e, "Machine", [
      { label: "Auto", on: () => { pc.machine = "auto"; redrawProjects(); } },
      ...able.map(m => ({ label: m.machine_name, on: () => { pc.machine = m.machine_id; keepMachine(p.name, m.machine_id); redrawProjects(); } })),
    ]);
  });
  folder.addEventListener("click", e => {
    const p = barProject(), place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
    if (!place) return;
    const trees = knownWorktrees(pc.sessions, place.machine_id, place.cwd);
    const pick = (to: CheckoutPick) => () => { pc.checkout = to; pc.target = null; redrawProjects(); };
    openMenu(e, "Folder", [
      { label: `Current checkout (${place.cwd.split("/").pop()})`, on: pick("current") },
      { label: "New worktree", on: pick("new") },
      { label: "New branch in the checkout", on: pick("branch") },
      ...trees.map(t => ({ label: `Worktree ${t.split("/").pop()}`, on: pick({ worktree: t }) })),
    ]);
  });
  // The start form's picker, here above the bar.
  harness.addEventListener("click", () => {
    pc.open = !pc.open;
    pc.picked.query = "";
    redrawProjects();
    if (pc.open) focusSearch(pick);
  });
  // Esc or a click outside closes it.
  form.addEventListener("keydown", e => {
    if (e.key !== "Escape" || !pc.open) return;
    e.preventDefault();
    e.stopPropagation();
    closeBarPicker();
    harness.focus();
  });
  addEventListener("mousedown", e => {
    if (pc.open && !pick.contains(e.target as Node) && !harness.contains(e.target as Node)) closeBarPicker();
  }, true);
  return { form, input, machine, folder, branch, clear, fresh, harness, send, err, pick };
})();

function closeBarPicker() {
  pc.open = false;
  redrawProjects();
}

/// The machine the bar starts on, its harnesses, and the harness picked
/// there, kept with the model when the machine or harness changes.
function barHarness(place: { machine_id: string } | null) {
  const host = place ? pc.hosts?.find(m => m.machine_id === place.machine_id) ?? null : null;
  const offered = host ? [...host.agents].sort(byPreference) : [];
  const agent = harnessFor(pc.picked.agent || null, offered, AGENT_ORDER);
  if (agent && agent !== pc.picked.agent) pc.picked.effort = keepEffort(agent, pc.picked.effort);
  pc.picked.agent = agent ?? "";
  const key = host && agent ? `${host.machine_id}/${agent}` : "";
  if (pc.modelFor !== key) { pc.picked.model = ""; pc.modelFor = key; }
  return { host, offered, agent };
}

/// The picked project and its checkouts.
function barProject() {
  const p = state.picked ? filterProjects(projectRows() || [], state.picked)[0] : null;
  return p ? { name: p.name, places: projectPlaces(p) } : null;
}

/// Reads what the bar needs for project `name`: the machines that run
/// agents, the sessions (for worktrees) and the project's branches.
function loadProjectBar(name: string) {
  const redraw = () => { if (shown === "projects" && state.picked === name) redrawProjects(); };
  invoke<AgentHost[]>("agent_hosts").then(list => { pc.hosts = list; preloadModels(list); redraw(); }).catch(e => { console.warn("agent hosts:", e); pc.hosts ??= []; redraw(); });
  invoke<AgentSession[]>("agent_sessions").then(list => { pc.sessions = list; }).catch(e => console.warn("agent sessions:", e));
  if (!pc.branches.has(name)) {
    invoke<BaseBranches | null>("project_branches", { project: name })
      .catch(() => null)
      .then(list => { pc.branches.set(name, list); redraw(); });
  }
}

/// Points the bar at project `name`: its field, machine, folder and harness.
function drawProjectComp(name: string) {
  const c = projectComp;
  if (pc.project !== name) {
    pc.project = name;
    pc.machine = lastMachine(name) || "auto";
    pc.checkout = DEFAULT_CHECKOUT;
    pc.target = null;
    c.err.textContent = "";
    loadProjectBar(name);
  }
  const p = barProject();
  // The machine last picked, while it still has a checkout and an agent.
  let place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
  if (p && !place && pc.machine !== "auto" && pc.hosts) place = barPlace(p.places, pc.hosts, "auto");
  const host = place ? pc.hosts?.find(m => m.machine_id === place.machine_id) ?? null : null;
  c.input.placeholder = barPrompt(name, !!place || pc.hosts === null);
  c.machine.replaceChildren(place ? place.machine_name : pc.machine === "auto" ? "Auto" : "No checkout", ...(place?.auto ? [" ", h("small", "", "Auto")] : []));
  const t = pc.target;
  const co = t ? targetCheckout(t) : barCheckout(pc.checkout, baseFor(null, pc.branches.get(name)));
  c.folder.replaceChildren(co.label, " ", h("small", "", co.sub));
  c.folder.disabled = !place;
  c.branch.hidden = t?.kind !== "issue";
  if (t?.kind === "issue" && document.activeElement !== c.branch) c.branch.value = t.branch;
  c.clear.hidden = !t;
  const { offered, agent } = barHarness(place);
  if (agent && host) {
    modelsOn(host.machine_id, agent, () => { if (shown === "projects" && state.picked === name) redrawProjects(); });
    c.harness.innerHTML = pillInner(agent, pc.picked.model, pc.picked.effort, host.machine_id);
  } else {
    c.harness.replaceChildren("Coding agent");
  }
  c.harness.disabled = !agent;
  if (!agent) pc.open = false;
  c.harness.setAttribute("aria-expanded", String(pc.open));
  drawBarPicker(host, offered);
  c.send.disabled = pc.starting || !place;
  const was = place && barExisting(name, place);
  c.fresh.hidden = !was;
  c.fresh.disabled = pc.starting;
  c.send.firstChild!.textContent = pc.starting ? "Starting" : was ? resumeLabel(was) : "Start";
}

/// "Resume Fix the crash (Claude Code, 2 h)": the bar's button for a
/// session already in the target's worktree.
function resumeLabel(s: AgentSession) {
  const title = s.title || "the session";
  return `Resume ${title.length > 40 ? title.slice(0, 40) + "..." : title} (${agentLabel(s.agent)}, ${ago(s.last_activity)})`;
}

/// The newest session of ours in the bar target's worktree on the bar's
/// machine, or null; none without a target.
function barExisting(name: string, place: { machine_id: string; cwd: string }) {
  const t = pc.target;
  if (!t || !t.branch.trim()) return null;
  // Where the branch list (read on one machine) found its worktree.
  const listed = pc.branches.get(name) as unknown as { machine_id?: string; branches: readonly { name: string; worktree?: string | null }[] } | null | undefined;
  const known = listed?.machine_id === place.machine_id ? listed.branches.find(b => b.name === t.branch.trim())?.worktree : null;
  return sessionIn(pc.sessions, place.machine_id, targetFolders(t, place.cwd, known));
}

/// Fills the bar for project `name` with `text` and `target`, on the
/// machine last picked, and brings it into view.
function fillBar(name: string, text: string, target: BarTarget) {
  if (pc.project !== name) drawProjectComp(name);
  pc.target = target;
  projectComp.input.value = text;
  projectComp.err.textContent = "";
  // Read again: a session may have started in the worktree since.
  invoke<AgentSession[]>("agent_sessions").then(list => { pc.sessions = list; redrawProjects(); }).catch(e => console.warn("agent sessions:", e));
  redrawProjects();
  projectComp.form.scrollIntoView({ block: "nearest" });
  projectComp.input.focus();
}

/// Work on it: the bar on a new worktree for the issue, from the default
/// branch, with the issue's prompt.
function workOnIssue(name: string, issue: IssueRow) {
  const base = baseFor(null, pc.branches.get(name));
  fillBar(name, issuePrompt(issue), { kind: "issue", number: issue.number, branch: issueBranch(issue.number, issue.title), base });
}

/// Continue: reads the pull request's branch, then fills the bar to start
/// on it. A fork's branch is not on `origin`, so it is refused.
async function continuePr(name: string, pr: PullRequestRow) {
  const agent = agentFor(state.agents, name);
  if (!agent) return;
  const head = await invoke<PullRequestHead>("center_pull_request_head", { projectId: agent.project_id, number: pr.number });
  if (head.fork) throw new Error(`PR #${pr.number} comes from a fork, so its branch ${head.branch} is not in this repository.`);
  fillBar(name, prPrompt(pr), { kind: "pr", number: pr.number, branch: head.branch });
}

/// The picker above the bar, when open.
function drawBarPicker(host: AgentHost | null, offered: string[]) {
  const el = projectComp.pick, p = pc.picked;
  if (!pc.open || !host || !p.agent) { el.innerHTML = ""; closedPicker(el); return; }
  const focused = el.contains(document.activeElement);
  drawPicker(el, {
    picked: p,
    offered,
    everywhere: [...new Set((pc.hosts ?? []).flatMap(m => m.agents))],
    machine: { id: host.machine_id, name: host.machine_name },
    noMachine: "",
    anchor: () => projectComp.harness,
    bounds: () => projectComp.form.parentElement,
    changed: what => {
      if (what === "model") { pc.open = false; redrawProjects(); return projectComp.input.focus(); }
      if (what === "agent") p.effort = keepEffort(p.agent, p.effort);
      if (what !== "models") redrawProjects();
      else if (state.picked) drawProjectComp(state.picked);
      if (what === "agent") focusSearch(el);
    },
  });
  if (focused) focusSearch(el);
}

/// Starts a coding agent on the bar's task in the project's checkout, then
/// opens its session; Esc there comes back to the project. With a target
/// whose worktree already has a session, that session goes on with the
/// task instead, unless `fresh` (Start new) starts another one there.
async function startFromBar(fresh = false) {
  const c = projectComp, p = barProject(), text = c.input.value.trim();
  let place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
  if (p && !place && pc.machine !== "auto") place = barPlace(p.places, pc.hosts ?? [], "auto");
  if (!p || !place || !text || pc.starting) return;
  const t = pc.target;
  if (t?.kind === "issue" && !t.branch.trim()) { c.err.textContent = "Name the branch first."; return; }
  const was = barExisting(p.name, place);
  if (was && !fresh) return resumeFromBar(was, text);
  const { agent } = barHarness(place);
  const model = pc.picked.model.trim() || null, effort = pc.picked.effort;
  // Start new beside a session goes in that session's worktree.
  const { cwd, checkout } = was ? { cwd: was.cwd, checkout: null }
    : t ? { cwd: place.cwd, checkout: targetPlan(t) }
    : startPlace(pc.checkout, place.cwd, baseFor(null, pc.branches.get(p.name)));
  c.err.textContent = "";
  pc.starting = true;
  pc.open = false;
  redrawProjects();
  try {
    const started = await invoke<Started>("agent_start", { machineId: place.machine_id, agent, cwd, prompt: text, chat: null, model, effort, checkout });
    c.input.value = "";
    pc.target = null;
    keepMachine(p.name, place.machine_id);
    go("pane", { agent: started.session });
  } catch (e) {
    c.err.textContent = refusal(e);
  } finally {
    pc.starting = false;
    redrawProjects();
  }
}

/// Sends the bar's task to session `s` as its next message: resumes it
/// when it ended, else prompts it where it runs. Then opens it.
async function resumeFromBar(s: AgentSession, text: string) {
  const c = projectComp;
  c.err.textContent = "";
  pc.starting = true;
  redrawProjects();
  try {
    if (s.status === "ended") {
      const started = await invoke<Started>("agent_resume", { machineId: s.machine_id, agent: s.agent, session: s.id, prompt: text });
      go("pane", { agent: started.session });
    } else {
      await invoke("agent_steer", { machineId: s.machine_id, agent: s.agent, session: s.id, steer: { prompt: { text } } });
      go("pane", { agent: { kind: "agent", ...s } });
    }
    c.input.value = "";
    pc.target = null;
  } catch (e) {
    c.err.textContent = refusal(e);
  } finally {
    pc.starting = false;
    redrawProjects();
  }
}

/// Reads the project agents, then the picked project's tasks, workers and
/// chat. Never two reads at once: one asked for during a read runs once
/// more after it.
let agentLoading = false, agentAgain = false;
async function loadProjectAgent() {
  if (agentLoading) { agentAgain = true; return; }
  agentLoading = true;
  try {
    try {
      state.agents = await invoke<ProjectAgent[] | null>("center_projects");
    } catch (e) {
      console.warn("project agents:", e);
      if (state.agents === undefined) state.agents = null;
    }
    const agent = agentFor(state.agents, state.picked);
    if (agent) {
      const was = state.pview.get(agent.name);
      try {
        const view = await invoke<ProjectView>("center_project", { projectId: agent.project_id, chatId: agent.chat_id });
        state.pview.set(agent.name, { view, error: null });
      } catch (e) {
        state.pview.set(agent.name, { view: was?.view ?? null, error: failure(e) });
      }
    }
  } finally {
    agentLoading = false;
  }
  // The header's Board and List switch shows once the project has an agent.
  redrawProjectsPage();
  if (agentAgain) { agentAgain = false; loadProjectAgent(); }
}

/// A project's checkout on each machine, for the start form.
function projectPlaces(p: Project) {
  return p.machines.filter(m => m.folder).map(m => ({ machine: m.id, cwd: m.folder }));
}

/// One machine's row under "Where it lives": its folder and what to do
/// there, or why there is none.
function whereRow(p: Project, m: WhereRow) {
  const name = h("span", "nm", h("b", "", m.name));
  if (m.kind === "here") {
    const path = m.folder!;
    const start = button("Start an agent here", "small", () => go("pane", { agentnew: { prompt: "", at: { machine: m.id, cwd: path, places: projectPlaces(p), project: p.name } } }));
    const copy = button("Copy path", "quiet small", async () => {
      try { await navigator.clipboard.writeText(path); copy.textContent = "Copied"; } catch (e) { console.warn("copy:", e); }
    });
    const finder = m.mac ? button("Open in Finder", "quiet small", () => invoke("open_folder", { path }).catch(e => console.warn("open folder:", e))) : null;
    const folder = h("span", "nm", h("span", "os", path));
    folder.title = path;
    if (m.syncthing) {
      // Syncthing keeps it in step, so Beep's own sync stays off here.
      const note = h("small", "", "Synced by Syncthing");
      note.title = `Syncthing folder "${m.syncthing}". Beep does not sync it here.`;
      folder.append(" ", note);
    }
    return [name, folder, h("span", "acts", finder, copy, start)];
  }
  const said = h("span", "os", m.text);
  if (m.kind === "none" && m.checked_at) said.title = `Checked ${since(m.checked_at)}`;
  const what = h("span", "nm", said);
  const clone = m.kind === "none" && p.url ? button("Clone here", "small", () => importSheet(false, repoFromRemote(p.url), m.id)) : null;
  return [name, what, h("span", "acts", clone)];
}

/// Asks the server where `name` lives on each machine; while it looks
/// through one, it asks again shortly. A folder it finds brings that
/// machine's sessions with it.
let readingWhere: string | null = null;
let whereTimer: ReturnType<typeof setTimeout> | undefined;
async function loadWhere(name: string) {
  if (readingWhere === name) return;
  readingWhere = name;
  const before = state.where.get(name);
  let got: ProjectMachineRow[] | null = null;
  try {
    const [machines, places] = await Promise.all([
      invoke<ProjectMachineRow[] | null>("project_machines", { project: name }),
      invoke<SyncthingPlace[]>("project_syncthing", { project: name }).catch(e => { console.warn("project syncthing:", e); return []; }),
    ]);
    got = withSyncthing(machines, places);
  } catch (e) {
    console.warn("project machines:", e);
  } finally {
    readingWhere = null;
  }
  state.where.set(name, got);
  const folders = (list: ProjectMachineRow[] | null) => (list || []).filter(m => m.folder).map(m => `${m.id} ${m.folder}`).join("\n");
  if (before !== undefined && folders(before) !== folders(got)) {
    sessionsDue.add(name);
    refresh();
  }
  clearTimeout(whereTimer);
  if (got?.some(m => m.checking)) {
    whereTimer = setTimeout(() => {
      if (shown === "projects" && state.picked === name) loadWhere(name);
    }, 3000);
  }
  if (state.picked === name) redrawProjects();
}

/// The sessions your coding agents ran in the project's folders, newest
/// first, each resumable; those running now are under Agents.
function sessionsSection(p: Project, running: RunningAgent[]) {
  const sessions = state.sessions.get(p.name);
  if ((sessions === undefined || sessionsDue.has(p.name)) && p.imported) loadSessions(p.name);
  let body;
  if (!p.imported) body = emptyPanel(h("span", "", "Import this project to see the sessions your agents ran in it."));
  else if (sessions === undefined) body = emptyPanel(h("span", "", "Reading sessions…"));
  else {
    const past = pastSessions(sessions, running);
    body = past.length
      ? h("div", "cc-panel pv-scroll", ...past.map(sessionRow))
      : emptyPanel(h("span", "", "No sessions from Claude Code, Codex, pi, flsh or OpenCode in these folders yet."));
  }
  return section("sec-sessions", "Sessions", sessions?.length || "", null, body,
    h("small", "", "from your coding agents in its folders on every machine, newest first"),
    sessions !== undefined && readingSessions.has(p.name) && h("small", "ss-updating", "Updating…"));
}

/// The agent view for a session row.
function openAgentRow(r: SessionRow) { go("pane", { agent: { kind: "agent", ...r } }); }

function sessionRow(s: SessionRow) {
  const ended = s.status === "ended";
  const said = h("span", "bad");
  const act: HTMLButtonElement = ended
    ? button("Resume", "small", async () => {
      act.disabled = true;
      act.textContent = "Resuming…";
      said.textContent = "";
      try {
        const started = await invoke<Started>("agent_resume", { machineId: s.machine_id, agent: s.agent, session: s.id, prompt: null });
        sessionsDue.add(state.picked!);
        openAgentRow(started.session);
      } catch (e) {
        act.disabled = false;
        act.textContent = "Resume";
        said.textContent = refusal(e);
      }
    })
    : button("Open", "small", () => openAgentRow(s));
  const meta = [h("span", "ap-ag", agentLabel(s.agent)), ended ? "ended" : s.status, s.machine_name, base(s.cwd), s.last_activity ? ago(s.last_activity) : ""]
    .filter(Boolean).map(x => typeof x === "string" ? h("span", "", x) : x);
  return h("div", "pv-row", h("span", "t", s.title || "Untitled session"), h("span", "m", ...meta), h("span", "acts", said, act));
}

/// The projects whose sessions are being read, and those to read again on
/// the next draw. The last list read stays in `state.sessions` and shows
/// meanwhile, so an opened project lists its sessions at once.
const readingSessions = new Set<string>();
const sessionsDue = new Set<string>();
async function loadSessions(name: string) {
  if (readingSessions.has(name)) return;
  readingSessions.add(name);
  sessionsDue.delete(name);
  let got: SessionRow[] | null = null;
  try {
    got = await invoke<SessionRow[]>("project_sessions", { project: name });
  } catch (e) {
    console.warn("project sessions:", e);
  } finally {
    readingSessions.delete(name);
  }
  const { list, changed } = nextSessions(state.sessions.get(name), got);
  state.sessions.set(name, list);
  if (shown !== "projects" || state.picked !== name) return;
  if (changed) drawProjects(projectsEl());
  else for (const el of document.querySelectorAll(".ss-updating")) el.remove();
}

/// The checkouts the machines found that are not projects yet: the first
/// three, each with Import; the sheet lists them all.
function foundSection() {
  if (!state.found?.length) return null;
  injectAddProjectStyle();
  const list = state.found.slice(0, 3).map(row => foundRow(row,
    { busy: state.importing, error: state.foundError?.name === row.name ? state.foundError.message : null }, importFromPage));
  const more = state.found.length > 3 ? button(`All ${state.found.length}`, "quiet small", addProject) : null;
  return section("sec-found", "Found on your machines", "", more, h("div", "cc-panel ap-list", ...list),
    h("small", "", "folders your coding agents worked in, without a project"));
}

let askedGit = false;
/// The open PRs and issues in repos without a project, each repo with
/// Import project, under the Projects overview.
function elsewhereSection(rows: Project[]) {
  if (!state.git) {
    if (!askedGit) { askedGit = true; refreshGit().then(redrawProjects); }
    return null;
  }
  const repos = outsideThreads(state.git, rows).slice(0, 8);
  if (!repos.length) return null;
  const body = repos.flatMap((r, i) => {
    const imp = rowButton("Import project", i ? "small" : "pri small", () => importSheet(false, r.repo));
    const head = repoLine(undefined, r.repo, r.host, [h("span", "age", ago(r.last)), imp], () => openUrl(repoUrl(r.items[0].url)));
    return [head, ...r.items.slice(0, ITEMS_SHOWN).map(item => openItemRow(item, null))];
  });
  return section("sec-elsewhere", "Your open PRs and issues in repos without a project", "", null,
    h("div", "cc-panel cc-open", ...body));
}

/// "github.com" from a project's web page.
function hostOf(url: string | null) {
  try { return new URL(url ?? "").host; } catch { return ""; }
}

function drawProjects(el: HTMLElement) {
  const rows = projectRows();
  const view = h("div", "cc-view");
  let one: Project | null = null;
  if (!state.home) {
    view.append(emptyPanel(h("b", "", "Loading projects…")));
  } else if (!rows || !rows.length) {
    view.append(h("div", "cc-panel cc-empty",
      h("b", "", "No projects yet"),
      h("span", "", "Add a folder your coding agents worked in, any folder, or a repository to clone."),
      importButton("pri")));
  } else {
    const shownRows = filterProjects(rows, state.picked);
    one = state.picked && shownRows.length === 1 ? shownRows[0] : null;
    if (one) view.append(...projectDetail(one));
    else if (!state.picked && state.overview) view.append(...overviewSection(rows));
    else view.append(section("sec-projects", "Projects", state.picked ? "" : rows.length, null, projectTable(shownRows.length ? shownRows : rows)));
    const found = one ? null : foundSection();
    if (found) view.append(found);
    const elsewhere = !state.picked && state.overview ? elsewhereSection(rows) : null;
    if (elsewhere) view.append(elsewhere);
  }
  // Only the view is drawn again: the composer keeps what is typed and its
  // focus, and the view keeps its scroll.
  const old = el.querySelector(":scope > .cc-view");
  if (old) {
    // Read before the swap: a detached element scrolls to 0.
    const top = old.scrollTop;
    const inner = new Map([...old.querySelectorAll<HTMLElement>("[id] .pv-scroll")].map(s => [s.closest("[id]")!.id, s.scrollTop]));
    old.replaceWith(view);
    view.scrollTop = top;
    for (const [id, at] of inner) {
      const s = view.querySelector<HTMLElement>(`#${CSS.escape(id)} .pv-scroll`);
      if (s) s.scrollTop = at;
    }
  } else {
    el.prepend(view);
  }
  // The bar starts coding agents in the project's checkout.
  if (one) {
    drawProjectComp(one.name);
    if (projectComp.form.parentNode !== el) el.append(projectComp.form);
  } else {
    projectComp.form.remove();
  }
}

registerView("projects", {
  title() {
    if (!state.picked) {
      const rows = projectRows();
      const sub = state.overview && rows?.length ? overviewSub(overviewGroups(rows, state.overview, state.top, state.groups)) : "Repositories on your machines";
      // Groups show on the overview, from a server that keeps them.
      const tools = state.overview && state.groups ? [button("", "small", () => editGroup())] : [];
      if (tools.length) tools[0].append(icon(IC.folder)!, " New group");
      return { title: "Projects", sub, tools: [...tools, importButton("small")] };
    }
    const p = filterProjects(projectRows() || [], state.picked)[0];
    const tools = [];
    if (p?.imported && agentFor(state.agents, p.name)) tools.push(layoutSwitch());
    if (p?.url) {
      const url = p.url, label = openOnLabel(url);
      if (label) tools.push(button(label, "small", () => openUrl(url)));
    }
    if (p) {
      const on = state.top.includes(p.name);
      tools.push(button(on ? "Unpin" : "Pin", "quiet small", () => setProjectOnTop(p.name, !on)));
      if (p.imported) tools.push(button(REMOVE_PROJECT, "quiet small", () => removeProject(p.name)));
    }
    tools.push(button("All projects", "quiet small", () => openProjects()));
    return { title: state.picked, sub: projectLine(p, state.home?.projects, Date.now()), tools };
  },
  mount(el, arg) {
    state.picked = (arg as ProjectsArg | undefined)?.project || null;
    if (state.picked) { sessionsDue.add(state.picked); state.where.delete(state.picked); }
    openTile(null, false);
    drawProjects(el);
    if (!state.picked) { loadFound(); loadOverview(); }
    refresh();
  },
  unmount() {
    openTile(null, false);
  },
});

/// Home's short list of projects: the page has the table.
function projectsSection(rows: Project[]) {
  const more = button("All projects", "quiet small", () => openProjects());
  const list = rows.slice(0, 6).map((p, i) => {
    const row = line(
      projectDot(i),
      p.name,
      [p.machines.map(m => m.name).join(", "), p.agents ? plural(p.agents, "agent", "agents") : ""].filter(Boolean).join(" · "),
      [p.last ? ago(p.last) : ""],
      () => openProjects(p.name));
    newWindowOn(row, row, p.name);
    return row;
  });
  return section("sec-projects-home", "Projects", rows.length, more, h("div", "cc-panel", ...list));
}

// ---------- Pane ----------

const ROUTINES_TITLE: [string, string] = ["Routines", "What Beep does on its own"];
const APPS_TITLE: [string, string] = ["Apps", "What Beep can use for you"];
const SETTINGS_TITLE: [string, string] = ["Settings", ""];

/// Titles of the pane's views; a chat or group takes its own name.
const PANE_TITLES: Record<string, [string, string]> = {
  chats: ["Chats", "Every chat, from the bar, this window and your phone"],
  memories: ["Memories", "What Beep knows about you"],
  routines: ROUTINES_TITLE,
  rnew: ROUTINES_TITLE,
  grants: ROUTINES_TITLE,
  skills: ["Skills", "What Beep knows how to do"],
  apps: APPS_TITLE,
  mcp: APPS_TITLE,
  appperms: APPS_TITLE,
  settings: SETTINGS_TITLE,
  machines: SETTINGS_TITLE,
  research: ["Research", ""],
  agents: ["Coding agents", "On your machines"],
  agentnew: ["Start a coding agent", "It runs on your machine; follow it and answer it here"],
  agent: ["Coding agents", "Follow it and answer it here"],
};

function paneTitle(): ViewHead {
  const on = paneOn();
  if (on.view === "chat") {
    const c = chatList.find(x => x.id === on.session);
    // A chat just started is not in the list until the server has it.
    const at = pane.startedAt();
    const thread = on.session ? threadChats?.get(on.session) : undefined;
    if (thread) return threadHead(thread, c);
    return { title: c?.title ?? "New chat", sub: at ? `Started ${startedWhen(at)}` : "", tools: c ? [pinButton(c)] : [], scope: scopeTag(on.session) };
  }
  if (on.group) {
    const g = chatList.find(x => x.id === on.group);
    return { title: g?.title ?? "Group", tools: g ? [pinButton(g)] : [] };
  }
  const [title, sub] = PANE_TITLES[on.view ?? ""] ?? ["Beep", ""];
  if (on.view === "agentnew") return { title, sub, tools: [button("All sessions", "small", () => go("pane", "agents"))] };
  return { title, sub };
}

/// Beside a chat's title: the project it belongs to, which opens the
/// project, or General.
function scopeTag(session: string | null) {
  // A chat about one thread runs as its project's agent too.
  const thread = session ? threadChats?.get(session) : undefined;
  const agent = chatProject(state.agents, session) ?? (thread ? { name: thread.project } : null);
  if (!agent) return h("span", "cc-tag", "General");
  const i = (projectRows() || []).findIndex(p => p.name === agent.name);
  const b = h("button", "cc-tag pj-scope", projectDot(i >= 0 ? i : undefined), shortName(agent.name));
  b.type = "button";
  b.title = `${agent.name}: open the project`;
  b.addEventListener("click", () => openProjects(agent.name));
  return b;
}

/// The project agents alone, so a chat's header knows its project before
/// any project page has loaded them.
async function loadAgents() {
  try {
    state.agents = await invoke<ProjectAgent[] | null>("center_projects");
  } catch (e) {
    console.warn("project agents:", e);
    return;
  }
  if (shown === "pane") drawTop();
  loadThreadChats();
}

// ---------- Chats about one thread ----------


/// Reads which chats are about one thread, for every project, then
/// redraws what lists or heads them. A project whose read fails keeps
/// what was known of it.
function loadThreadChats() {
  if (threadsLoading) return threadsLoading;
  const agents = state.agents || [];
  const before = threadChats;
  const mine: Promise<void> = Promise.all(agents.map(a => invoke<ThreadChatRow[] | null>("center_thread_chats", { projectId: a.project_id })
    .then(rows => (rows || []).map((c): ThreadChatEntry => ({ ...c, project_id: a.project_id, project: a.name })))
    .catch((e): ThreadChatEntry[] => {
      console.warn("thread chats:", e);
      return [...(before?.values() ?? [])].filter(c => c.project_id === a.project_id);
    })))
    .then(lists => {
      threadChats = new Map(lists.flat().map(c => [c.chat_id, c]));
      reloadChats();
      reloadGroup("projects");
      if (shown === "pane") { drawTop(); pane.chatTop(); }
    })
    .finally(() => { if (threadsLoading === mine) threadsLoading = null; });
  return threadsLoading = mine;
}

/// A thread as `center_thread_view` reads it from its host (ThreadCardRow).
interface ThreadCardView {
  kind: string;
  number: number;
  title: string;
  url: string;
  author: string;
  body: string;
  needs_reply: boolean;
  /// `at` in milliseconds since the Unix epoch.
  comments: { author: string; at: number; body: string }[];
  earlier: number;
}

/// The card atop a thread chat: its element, what it last read (null from
/// a server too old to read one), when, and its error.
interface ThreadCard {
  el: HTMLElement;
  view: ThreadCardView | null | undefined;
  at: number;
  error: string;
  loading: boolean;
  all: boolean;
}
const threadCards = new Map<string, ThreadCard>();
/// How long a thread card's read serves before the chat reads it again.
const THREAD_FRESH_MS = 60000;
/// The comments a thread card shows before "Show all".
const THREAD_COMMENTS = 3;
/// The most of one post a card shows before "Show all".
const THREAD_TEXT = 600;

/// The card atop thread chat `t`, read again when it is stale.
function threadCard(t: ThreadChatEntry) {
  let card = threadCards.get(t.chat_id);
  if (!card) {
    card = { el: h("div", "th-card"), view: undefined, at: 0, error: "", loading: false, all: false };
    threadCards.set(t.chat_id, card);
  }
  if (!card.loading && Date.now() - card.at > THREAD_FRESH_MS) readThreadCard(t, card);
  drawThreadCard(t, card);
  return card.el;
}

async function readThreadCard(t: ThreadChatEntry, card: ThreadCard) {
  card.loading = true;
  try {
    card.view = await invoke<ThreadCardView | null>("center_thread_view", { projectId: t.project_id, repo: t.repo, number: t.number });
    card.error = "";
  } catch (e) {
    card.error = refusal(e);
  } finally {
    card.loading = false;
    card.at = Date.now();
  }
  drawThreadCard(t, card);
  const on = paneOn();
  if (on.view === "chat" && on.session === t.chat_id) drawTop();
}

const clip = (text: string, all: boolean) => all || text.length <= THREAD_TEXT ? text : text.slice(0, THREAD_TEXT).trimEnd() + "...";

function drawThreadCard(t: ThreadChatEntry, card: ThreadCard) {
  const v = card.view, label = threadLabel(t.kind, t.number);
  card.el.hidden = v === null;
  if (v === undefined) {
    card.el.replaceChildren(card.error ? h("span", "who", card.error) : h("span", "who", h("span", "dspin"), ` Reading ${label}…`));
    return;
  }
  if (!v) return card.el.replaceChildren();
  const total = v.earlier + v.comments.length;
  const where = openLabel(v.url) === "Open on GitHub" ? "On GitHub" : "On its host";
  const head = h("span", "who", `${where} · ${plural(total, "comment", "comments")}${v.needs_reply ? " · waits on your reply" : ""}`);
  const post = (author: string, at: number | null, body: string) => {
    const c = h("div", "c", h("b", "", author), " ", h("span", "who", at ? since(at) : "opened it"));
    if (body.trim()) c.append(h("div", "tx", clip(body, card.all)));
    return c;
  };
  const shown = card.all ? v.comments : v.comments.slice(-THREAD_COMMENTS);
  const hidden = v.comments.length - shown.length;
  const rows = [post(v.author, null, v.body), ...shown.map(c => post(c.author, c.at, c.body))];
  const long = [v.body, ...shown.map(c => c.body)].some(b => b.length > THREAD_TEXT);
  const more: Node[] = hidden > 0 || (long && !card.all)
    ? [button(hidden > 0 ? `Show ${plural(hidden, "earlier comment", "earlier comments")}` : "Show all", "quiet small", () => { card.all = true; drawThreadCard(t, card); })]
    : [];
  if (card.all && v.earlier) more.push(h("span", "who", `${plural(v.earlier, "older comment", "older comments")} on the host`));
  if (card.error) more.push(h("span", "who", card.error));
  card.el.replaceChildren(head, ...rows, ...more);
}

/// A thread chat's header: its title, its project to go back to, whether
/// it waits on a reply, and a link to the thread.
function threadHead(t: ThreadChatEntry, c: ChatItem | undefined): ViewHead {
  const view = threadCards.get(t.chat_id)?.view;
  const scope = h("span", "th-scope", scopeTag(t.chat_id));
  if (view?.needs_reply) scope.append(stateChip("blocked", "needs reply"));
  const tools: Node[] = [];
  if (view?.url) tools.push(button(openLabel(view.url), "", () => openUrl(view.url)));
  if (c) tools.push(pinButton(c));
  const title = view ? `${threadLabel(view.kind, view.number)} ${view.title}`.trim() : (c?.title || t.title || threadLabel(t.kind, t.number));
  return { title, tools, scope };
}

function pinButton(c: ChatItem) {
  return button(c.pinned ? "Unpin" : "Pin", "", () => setPinned(c, !c.pinned));
}

registerView("pane", {
  el: $("#pane-box"),
  title: paneTitle,
  mount(el, arg) {
    pane.mounted(true);
    pane.open(arg as PaneTarget);
  },
  unmount() {
    pane.mounted(false);
    pane.leave();
  },
});

/// The window stops showing a chat or group when the pane moves off it, so
/// a focused window no longer keeps their notices quiet.
let last: Pick<PaneNow, "session"> & { view: string | null; group?: string | null } = { view: null, session: null };
$("#bar").dataset.view = pane.current().view ?? "";
pane.onChange(now => {
  if (shown === "pane") {
    if (last.view === "chat" && now.view !== "chat" && now.view !== "group") invoke("new_chat").catch(() => {});
    if (last.group && !now.group) invoke("close_group").catch(() => {});
  }
  last = now;
  $("#bar").dataset.view = now.view ?? "";
  drawSide();
});

// ---------- Handoffs ----------

/// Shows what the bar or tray asked for, once; Home when nothing is waiting.
async function takePending(fallback: string | null) {
  const target = await invoke<CenterTarget | null>("center_pending").catch(() => null);
  if (target == null) return fallback && go(fallback);
  if (target === "home") return go("home");
  if (target === "instructions") return go("instructions");
  if (target === "providers") return go("providers");
  go("pane", target);
  // Home is not on screen, but the sidebar counts come from it.
  refresh();
}

listen("center-open", () => takePending(null));

// ---------- Settings ----------

/// Settings as a sheet over the window: a dimmed backdrop and a centered
/// sheet with its sections on the left. Esc or a click outside closes it;
/// the view behind keeps its state. The sheet draws at once from what
/// this Mac knows and fills in the server's parts as they answer.
let settingsOpen: { scrim: HTMLElement; keys: (e: KeyboardEvent) => void; back: HTMLElement | null } | null = null;

export function openSettings(section?: string) {
  if (settingsOpen) return section && showSection(settingsOpen.scrim, section);
  const sheet = h("div", "cc-set");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Settings");
  sheet.tabIndex = -1;
  const nav = h("nav", "dset-nav", h("b", "", "Settings"));
  const body = h("div", "dset-body", h("div", "dset-top", h("span", "tt", h("b", "", "Settings")), h("kbd", "", "esc")), ...skeleton([80, 60, 72, 50]));
  sheet.append(nav, body);
  const scrim = h("div", "cc-set-scrim", sheet);
  const back = document.activeElement as HTMLElement | null;
  scrim.addEventListener("mousedown", e => { if (e.target === scrim) closeSettings(); });
  // While open, keys stay in the sheet: Esc closes it, and nothing reaches
  // the page behind.
  const keys = (e: KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); closeSettings(); }
    else if (e.metaKey && e.key === ",") e.preventDefault();
    else if (e.key === "Tab") trapTab(e, sheet);
    e.stopImmediatePropagation();
  };
  window.addEventListener("keydown", keys, true);
  document.body.append(scrim);
  settingsOpen = { scrim, keys, back };
  sheet.focus();
  pane.settings(sheet)
    .then(() => { if (section) showSection(scrim, section); })
    .catch(e => console.warn("settings:", e));
}

/// Scrolls the open sheet to `section`, such as "machines".
function showSection(scrim: HTMLElement, section: string) {
  scrim.querySelector<HTMLElement>(`[data-goto="${section}"]`)?.click();
}

export function closeSettings() {
  if (!settingsOpen) return;
  const { scrim, keys, back } = settingsOpen;
  settingsOpen = null;
  window.removeEventListener("keydown", keys, true);
  scrim.remove();
  if (back?.isConnected) back.focus();
}

/// Keeps Tab inside the sheet.
function trapTab(e: KeyboardEvent, sheet: HTMLElement) {
  const all = [...sheet.querySelectorAll<HTMLElement>("button:not([disabled]), [href], [tabindex]:not([tabindex='-1'])")];
  if (!all.length) return;
  const first = all[0], last = all[all.length - 1];
  // Focus lost to the page (a redraw removed it): bring it back in.
  if (!sheet.contains(document.activeElement)) { e.preventDefault(); return (e.shiftKey ? last : first).focus(); }
  if (e.shiftKey && (document.activeElement === first || document.activeElement === sheet)) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
}


registerInstructions({ registerView, invoke, h, button, ago, openSettings });
registerProviders({ registerView, invoke, h, button, openSettings, redrawTop: () => { if (shown === "providers") drawTop(); } });

// ---------- Keys ----------

initSideResize($(".cwin"), $("#side-grip"));
mountUsage($("#usage"), refresh => invoke<UsageView | null>("usage_get", { refresh }));

initPalette({ go, jump, pane, settings: openSettings, providers: () => go("providers"), project: openProjects, addProject, branches: name => { openProjects(name); openTile("branches"); }, findIcon: IC.find });

/// ⌘ and a letter: the pane page it opens.
const PANE_KEYS: Record<string, string> = { m: "memories", j: "routines", l: "skills" };

document.addEventListener("keydown", e => {
  // Keys typed into a terminal are the terminal's, Esc and Tab too.
  if (inTerminal(e)) return;
  // Esc on an opened board tile goes back to the board.
  if (e.key === "Escape" && shown === "projects" && pa.tile && !e.defaultPrevented) {
    e.preventDefault();
    return openTile(null);
  }
  if (!e.metaKey) return;
  const key = e.key.toLowerCase();
  // ⌘W closes this window; Beep keeps running in the menu bar.
  if (key === "w" && !e.shiftKey) { e.preventDefault(); return window.close(); }
  if (key === "k") { e.preventDefault(); return openPalette(); }
  if (key === ",") { e.preventDefault(); return openSettings(); }
  const to = PANE_KEYS[key];
  // ⌘M on the start form picks the model there.
  if (to && !(key === "m" && shown === "pane" && paneOn().view === "agentnew")) { e.preventDefault(); return go("pane", to); }
  // In the pane, the pane's own keys call newChatHere.
  if (key === "n" && shown !== "pane") { e.preventDefault(); return newChatHere(); }
});

/// The machines online in `home`, as a key that changes when one connects.
function onlineMachines(home: HomeView | null) {
  return (home?.machines || []).filter(m => m.online).map(m => m.id).sort().join(" ");
}

/// Whether `e` was typed into a terminal's screen.
function inTerminal(e: KeyboardEvent) {
  return e.target instanceof Element && !!e.target.closest(".tm-xterm");
}

/// ⌘N: the project's chat while a project or its chat shows, else a new
/// general chat. A project with no chat gets its bar, which starts a
/// coding agent.
function newChatHere() {
  const on = paneOn();
  const name = newChatProject({ shown, picked: state.picked, view: on.view, session: on.session }, state.agents);
  const agent = name && agentFor(state.agents, name);
  if (agent) return go("pane", { chat: { session: agent.chat_id } });
  if (name && shown === "projects" && projectComp.form.isConnected) return projectComp.input.focus();
  go("pane", { chat: { session: null } });
}
pane.connect({
  openSettings,
  openInstructions() {
    closeSettings();
    go("instructions");
  },
  openProviders() {
    closeSettings();
    go("providers");
  },
  settingsAway(view) {
    closeSettings();
    if (view) go("pane", view);
  },
  newChat: newChatHere,
  goBack: (from: BackTo) => go(from.view, from.arg as ViewArg),
  chatTop(session) {
    const t = threadChats?.get(session);
    return t ? threadCard(t) : null;
  },
});

// ---------- Refresh ----------

let loading = false, again = false, retryTimer: ReturnType<typeof setTimeout> | undefined;

/// Fetches Home. Never two calls at once: a request during one runs once more after it.
async function refresh() {
  if (loading) { again = true; return; }
  clearTimeout(retryTimer);
  loading = true;
  try {
    const [home, mac, sync, top, projectGroups] = await Promise.all([
      invoke<HomeView>("center_home"),
      invoke<MacControl>("mac_control").catch(e => { console.warn("this Mac's state:", e); return state.mac; }),
      // Sync progress only adds to the Now column; Home draws without it.
      invoke<SyncRow[]>("project_sync").catch(e => { console.warn("project sync:", e); return state.sync; }),
      invoke<string[]>("projects_on_top").catch(e => { console.warn("projects on top:", e); return state.top; }),
      invoke<ProjectGroup[] | null>("project_groups").catch(e => { console.warn("project groups:", e); return state.groups; }),
    ]);
    state.mac = mac;
    state.top = top || [];
    state.groups = projectGroups;
    state.sync = sync;
    if (home.updates) home.updates = withoutDismissed(home.updates, dismissedUpdates);
    const was = onlineMachines(state.home);
    state.home = home;
    fresh = true;
    // A machine came online: its harnesses' models are read ahead of the
    // picker.
    if (onlineMachines(home) !== was) {
      invoke<AgentHost[]>("agent_hosts").then(preloadModels).catch(e => console.warn("agent hosts:", e));
    }
    state.error = null;
    const live = new Set(state.home.needs_you.map(cardKey));
    for (const key of state.outcomes.keys()) if (!live.has(key)) state.outcomes.delete(key);
  } catch (e) {
    state.error = String(e);
  } finally {
    loading = false;
  }
  draw();
  // Every terminal's state refreshes with Home, while Home or the
  // Terminals page shows, or before the first list for the sidebar.
  if (state.home?.machines && (shown === "home" || shown === "terminals" || allTerms.list(ALL) === undefined)) allTerms.load(ALL);
  // A project's agent refreshes with Home: on focus, on notices and every
  // 10 seconds while the window shows.
  if (shown === "projects") {
    drawProjects(projectsEl());
    drawTop();
    if (state.picked) {
      loadProjectAgent();
      const p = filterProjects(projectRows() || [], state.picked)[0];
      if (p && boardOn(p)) terms.load(state.picked);
    } else {
      loadOverview();
    }
  }
  reloadGroup("projects");
  reloadGroup("pinned");
  reloadGroup("machines");
  if (again) { again = false; refresh(); }
  // Until Home first loads, a failed try is made again soon, seen or not,
  // unless waiting will not fix it.
  else if (!fresh && !problem()) retryTimer = setTimeout(refresh, CONNECT_RETRY_MS);
}

// "Still connecting" takes over once the wait passes its limit.
setTimeout(() => { if (!state.home) draw(); }, CONNECT_STUCK_MS);

window.addEventListener("focus", refresh);
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") refresh(); });
setInterval(() => { if (document.visibilityState === "visible") refresh(); }, 10000);
// Open PRs and issues: the server keeps them a few minutes, so once a minute
// on Home is plenty.
window.addEventListener("focus", () => { if (shown === "home") refreshGit(); });
setInterval(() => { if (document.visibilityState === "visible" && shown === "home") refreshGit(); }, 60000);
for (const name of ["link-state", "sessions-changed", "work-question", "work-resolved", "group-approval", "approval-answered", "reconnected", "home-changed"]) {
  listen(name, () => refresh());
}
for (const name of ["sessions-changed", "group-changed", "reconnected"]) {
  listen(name, () => reloadChats());
}

$<HTMLFormElement>("#home-comp").addEventListener("submit", async e => {
  e.preventDefault();
  const input = $<HTMLInputElement>("#home-ask"), note = $("#home-err"), prompt = input.value.trim();
  if (!prompt) return;
  note.textContent = "";
  input.value = "";
  go("pane", { chat: { session: null } });
  try {
    await pane.compose(prompt);
  } catch (err) {
    // Back on Home with the prompt as typed, where the error shows.
    input.value = prompt;
    go("home");
    note.textContent = refusal(err);
  }
  reloadChats();
});

$("#home-agent")?.addEventListener("click", () => {
  const input = $<HTMLInputElement>("#home-ask"), prompt = input.value.trim();
  input.value = "";
  go("pane", { agentnew: { prompt } });
});

/// The first view. A project's own window starts on that project and
/// never reads what the bar or tray left for the main window, so each
/// window keeps its own history from there.
async function start() {
  const project = await invoke<string | null>("center_window_project").catch(() => null);
  document.title = windowTitle(project);
  const first = startView(project);
  if (!first) return takePending("home");
  go(first.view, first.arg);
}

/// Shows what the app kept from the server's last answers, so the window
/// opens on Home, its projects and sidebar before the server answers.
async function showCached() {
  const [cached, mac] = await Promise.all([
    invoke<CachedViews | null>("center_cached").catch(e => { console.warn("kept views:", e); return null; }),
    invoke<MacControl>("mac_control").catch(() => null),
  ]);
  const home = seed(state, cached, dismissedUpdates);
  state.mac ??= mac;
  if (!home) return;
  draw();
  reloadGroup("projects");
  reloadGroup("pinned");
  reloadGroup("machines");
}

// A reload starts with no chat open, as the window just opened does.
Promise.all([invoke("new_chat").catch(() => {}), showCached()]).then(start);
loadAgents();
