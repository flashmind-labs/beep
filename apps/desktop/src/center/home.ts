// Home: its tabs, the Running now, Needs you, Coming up and Updates
// sections, and the Connecting page before Home first loads.
import { harnessEl } from "../agents/harnessicon.ts";
import { ago, plural } from "../ui/format.ts";
import { dbtn as button, emptyPanel, errorNote, note, svgEl as icon } from "../ui/parts.ts";
import { invoke, Channel } from "../ui/tauri.ts";
import { connectProblem } from "./connect.ts";
import { pane } from "../bar/pane.ts";
import { machineDisk, needsLine, runningKey, runningAt, runningLine, byRecent, thisMacRow, type HomeView, type MachineCard, type NeedsYou, type RunningAgent, type RunningRow, type UpdateRow, type CronRow } from "./home-state.ts";
import { el as h } from "../project/projects-state.ts";
import { waitingFirst } from "./openitems-state.ts";
import { ALL } from "../terminals/terminals.ts";
import { type Outcome, $, IC, CONNECT_STUCK_MS, state } from "./state.ts";
import { whenOf, scheduleText, failure, conflict, dot, section, avatar, tag, rowButton, bare, line } from "./pieces.ts";
import { addMachineButton, useThisMac, machineMenu, machinesSection, offlineNote } from "../machines/table.ts";
import { allTerms, openTerminals, terminalLine } from "../terminals/view.ts";
import { gitSection, itemLine, refreshGit } from "./openitems.ts";
import { registerView, go } from "./views.ts";
import { drawSide, projectRows } from "./sidebar.ts";
import { homeProjectRow, projectsSection } from "../project/page.ts";
import { loading, refresh, fresh } from "./refresh.ts";

export function runningRow(r: RunningRow) {
  const { title, sub } = runningLine(r, state.home?.projects || []);
  const right = [r.status === "asking" ? tag("wait", "needs you") : ago(runningAt(r))];
  const row = r.kind === "research"
    ? bare(line(null, title, sub, right, () => invoke("open_research", { session: r.session }).catch(() => {})))
    // The pane's agent view follows and answers it.
    : line(harnessEl(r.agent), title, sub, right, () => go("pane", { agent: r }));
  row.dataset.key = runningKey(r);
  return row;
}


// ---------- Needs you ----------

type ChatNeed = Extract<NeedsYou, { kind: "chat" }>;
type AgentNeed = Extract<NeedsYou, { kind: "agent" }>;

export function cardKey(n: NeedsYou) { return n.kind === "chat" ? `chat:${n.pending.query_id}` : `agent:${n.machine_id}:${n.session}:${n.question.id}`; }

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

function comingRow(c: CronRow) {
  const w = whenOf(c.next_run_at);
  return line(h("span", "cc-when", h("b", "", w.time), w.day), c.label.trim() || c.prompt.trim() || "Routine", scheduleText(c.schedule), [], () => go("pane", "routines"));
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
export function startedWhen(ms: number) {
  const t = updateTime(ms);
  return t === "Yesterday" ? "yesterday" : t;
}

/// Updates dismissed here that a list may still carry, by id.
export const dismissedUpdates = new Set<number>();

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

/// Opens the update's chat, where Beep's first message tells it and the user
/// replies; the same chat each time.
async function openUpdate(u: UpdateRow) {
  dismissUpdates([u.id]);
  try {
    const session = await invoke<string | null>("center_open_update_chat", { id: u.id });
    if (session) return go("pane", { chat: { session } });
    go("pane", { chat: { session: null } });
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

// ---------- Home ----------

type HomeTab = "overview" | "git" | "projects" | "machines";
/// The Home tab on screen.
export let homeTab: HomeTab = "overview";
/// The tab each section Home can jump to sits on.
export const TAB_OF: Record<string, HomeTab> = {
  "sec-needs": "overview", "sec-running": "overview", "sec-updates": "overview", "sec-coming": "overview",
  "sec-git": "git","sec-projects-home": "projects", "sec-machines": "machines",
};
/// Rows an Overview box shows; its title opens the rest.
const BOX_ROWS = 5;

function openHomeTab(tab: HomeTab) {
  homeTab = tab;
  draw();
  $("#view").scrollTop = 0;
}

/// Every machine across the top of Home: what it is doing and its free
/// disk, opening its row in Machines.
function machineStrip(machines: MachineCard[]) {
  const cells: HTMLElement[] = machines.map(m => {
    const disk = machineDisk(m).text;
    const now = m.online ? (m.agents ? plural(m.agents, "agent", "agents") : "idle") : "offline";
    const cell = h("button", `m${m.online ? "" : " off"}`, dot(m), h("b", "", m.name), h("span", "", disk === "–" ? now : `${now} · ${disk}`));
    cell.type = "button";
    cell.addEventListener("click", () => jump(`m-${m.id}`));
    machineMenu(m, cell);
    return cell;
  });
  const mac = thisMacRow(state.mac, machines);
  if (mac) {
    const cell = h("div", "m off", h("span", "ddot off"), h("b", "", mac.name), h("span", `${mac.bad ? "bad" : ""}`, mac.state));
    if (mac.action) cell.append(rowButton(mac.action, "quiet small", useThisMac));
    cell.title = `This Mac: ${mac.title}`;
    cells.push(cell);
  }
  return h("div", "cc-mstrip", ...cells, addMachineButton("quiet small"));
}

/// Home: the machines strip, then one tab at a time. A tab with nothing in
/// it is left out; Overview and Machines stay.
function page(home: HomeView) {
  const { machines, running } = home;
  const projects = projectRows() || [], git = state.git;
  const tabs: { key: HomeTab; label: string; count: number | null; on: boolean; make: () => HTMLElement[] }[] = [
    { key: "overview", label: "Overview", count: null, on: true, make: () => overview(home) },
    { key: "git", label: "PRs and issues", count: git?.items.length ?? 0, on: !!git?.hosts?.length, make: () => [gitSection(git!)] },
    { key: "projects", label: "Projects", count: projects.length, on: projects.length > 0, make: () => [projectsSection(projects)] },
    { key: "machines", label: "Machines", count: null, on: !!machines, make: () => [machinesSection(machines!)] },
  ];
  const shownTabs = tabs.filter(t => t.on);
  const tab = shownTabs.find(t => t.key === homeTab) || shownTabs[0];
  const bar = h("div", "cc-tabs");
  bar.setAttribute("role", "tablist");
  for (const t of shownTabs) {
    const b = h("button", "", t.label, t.count ? h("span", "n", String(t.count)) : null);
    b.type = "button";
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(t === tab));
    b.addEventListener("click", () => openHomeTab(t.key));
    bar.append(b);
  }
  const out: HTMLElement[] = [];
  if (machines) out.push(machineStrip(machines));
  out.push(bar);
  if (state.error) out.push(errorNote(`Could not refresh: ${state.error}`));
  if (!machines && !running) {
    out.push(note("off", "Machines and running sessions aren't available for this account or server."));
  } else if (machines) {
    const off = offlineNote(machines);
    if (off) out.push(off);
  }
  out.push(...tab.make());
  return out;
}

/// An Overview box: its title opens the full list, then its first rows.
export function homeBox(id: string, title: string, count: number | string, open: () => void, rows: Node[]) {
  return section(id, button(title, "lk", open), count, null, h("div", "cc-panel", ...rows.slice(0, BOX_ROWS)));
}

/// What needs you, then boxes two by two: Running and Projects, Pull
/// requests and Issues, Terminals and Coming up. A list with nothing in it
/// leaves its place empty; a row with nothing in it is left out.
export function overview(home: HomeView) {
  const { running, coming_up: coming, needs_you: needs, updates } = home;
  const items = [...(state.git?.items || [])].sort(waitingFirst);
  const prs = items.filter(i => i.kind === "pull_request"), issues = items.filter(i => i.kind === "issue");
  const terms = home.machines?.length ? allTerms.list(ALL) || [] : [], projects = projectRows() || [];
  const rows = [
    [running?.length ? homeBox("sec-running", "Running", running.length, () => go("pane", "agents"), byRecent(running).map(runningRow)) : null,
      projects.length ? homeBox("", "Projects", projects.length, () => openHomeTab("projects"), projects.map((p, i) => homeProjectRow(p, i))) : null],
    [prs.length ? homeBox("", "Pull requests", prs.length, () => openHomeTab("git"), prs.map(itemLine)) : null,
      issues.length ? homeBox("", "Issues", issues.length, () => openHomeTab("git"), issues.map(itemLine)) : null],
    [terms.length ? homeBox("", "Terminals", terms.length, () => openHomeTab("machines"), terms.map(t => terminalLine(t, () => openTerminals({ id: t.id })))) : null,
      coming?.length ? homeBox("sec-coming", "Coming up", coming.length, () => go("pane", "routines"), coming.map(comingRow)) : null],
  ].filter(r => r.some(b => b));
  const out: HTMLElement[] = [];
  if (needs.length) out.push(needsSection(needs));
  if (updates?.length) out.push(updatesSection(updates));
  for (const [a, b] of rows) out.push(h("div", "cc-cols", a, b));
  if (!out.length) out.push(emptyPanel(h("span", "", "Nothing needs you, and nothing is running.")));
  return out;
}

// ---------- Connecting ----------

/// True once the first Home has been waited for too long.
export function stuck() { return !state.home && Date.now() - state.since >= CONNECT_STUCK_MS; }

/// Before Home first loads, a failure waiting will not fix, or null.
export function problem() { return fresh ? null : connectProblem(state.error); }

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
  return `No answer from the server for ${secs} seconds. Still trying.`;
}

/// What Home shows before the server first answers: pulsing dots, never a blank
/// page.
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
    return h("div", "cc-conn", pulse, h("b", "", "Connecting to Beep…"));
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

export function draw() {
  const home = state.home, view = $("#view");
  drawSide();
  if (!home || problem()) return drawConnecting(view);
  const top = view.scrollTop;
  view.replaceChildren(...page(home));
  view.scrollTop = top;
  applyScroll();
  scrollTarget = null;
  $<HTMLInputElement>("#home-ask").placeholder = home.machines?.length ? "Ask Beep, or start an agent on a machine" : "Ask Beep anything";
  $("#home-agent").style.display = home.machines?.some(m => m.online) ? "" : "none";
}


registerView("home", {
  el: $("#home-box"),
  title: () => ({ title: "Home", sub: new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" }) }),
  mount() { refresh(); refreshGit(); },
});

/// Element Home scrolls to once it has drawn.
export let scrollTarget: string | null = null;

export function jump(id?: string) {
  if (!go("home")) return;
  const tab = !id ? "overview" : id.startsWith("m-") ? "machines" : TAB_OF[id];
  if (tab && tab !== homeTab) {
    homeTab = tab;
    if (state.home) draw();
  }
  const view = $("#view");
  if (!id) return view.scrollTo({ top: 0 });
  scrollTarget = id;
  // Home redraws when it mounts, so the target is also applied after that draw.
  applyScroll();
}

export function applyScroll() {
  if (!scrollTarget) return;
  const el = document.getElementById(scrollTarget);
  if (el) { el.scrollIntoView({ block: "start" }); scrollTarget = null; }
}
