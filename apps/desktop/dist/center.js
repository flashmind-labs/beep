// The command center window: a sidebar, Home, and the bar's page as a pane
// (app.js, loaded before this module) for chats, memories and routines.
const { invoke, Channel } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

import { splitPinned } from "./pinned.js";
import { initPalette, openPalette } from "./palette.js";
import { initSideResize } from "./sidewidth.js";
import { connectProblem } from "./connect.js";
import { withoutDismissed } from "./updates.js";
import { WIDE_HOME_PX, agentOf, base, homeColumns, homeRows, machineCaps, machineDisk, machineMemory, machineNow, machineOs, needsLine, runningLine, thisMacRow } from "./home.js";
import { openAddMachine as openAddMachineSheet } from "./addmachine.js";
import { openImportProject } from "./importproject.js";
import { openAddProject, foundRow, importFound, injectStyle as injectAddProjectStyle } from "./addproject.js";
import { mergeProjects, filterProjects, agentLabel, pastSessions, whereRows, whereCount, repoFromRemote } from "./projects-state.js";
import { ITEMS_SHOWN, itemChips, openTitle, projectsWithOpen, replyPrompt, reviewStart } from "./openitems.js";
import { agentFor, tasksSection, workersSection, chatSection, startSection } from "./project.js";

const $ = sel => document.querySelector(sel);

/// An element with `cls`; string children become text, never markup.
function h(tag, cls, ...kids) {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  for (const k of kids) if (k != null && k !== false) el.append(k);
  return el;
}
function icon(svg) {
  const s = h("span");
  s.style.display = "inline-flex";
  s.innerHTML = svg; // fixed markup from this file, no server text
  return s.firstChild;
}

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
const state = { home: null, mac: null, error: null, outcomes: new Map(), shown: false, since: Date.now(), picked: null, found: null, importing: null, foundError: null, sessions: new Map(), where: new Map(), agents: undefined, pview: new Map(), git: null, gitAll: new Set() };
/// Whether Home was last drawn in two columns.
let drawnWide = false;

// ---------- Formatting ----------

function plural(n, one, many) { return `${n} ${n === 1 ? one : many}`; }
function ago(ms) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.round(s / 60)} min`;
  if (s < 86400) return `${Math.round(s / 3600)} h`;
  return `${Math.round(s / 86400)} d`;
}
function whenOf(iso) {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return { time: "", day: "" };
  const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  const days = Math.round((new Date(at).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 86400000);
  const day = days <= 0 ? "today" : days === 1 ? "tomorrow" : at.toLocaleDateString([], { weekday: "short" });
  return { time, day };
}
/// A routine's schedule in words. The row already shows when it runs next,
/// so a one-off is just "Once".
function scheduleText(s) {
  if (/^\d{4}-\d\d-\d\dT/.test(s)) return "Once";
  return pane()?.when?.(s) ?? s;
}

// ---------- Pieces ----------

function dot(m) { return h("span", `ddot${!m.online ? " off" : m.agents ? " busy" : ""}`); }

/// A Home section: its title and count, an optional button on the right,
/// then its body.
function section(id, title, count, action, body) {
  const el = h("div");
  el.id = id;
  const head = h("h4", "", title);
  if (count) head.append(h("span", "n", String(count)));
  el.append(h("div", "cc-sec", head, action), body);
  return el;
}
function avatar(cls, ini) { return h("span", `cc-ag ${cls}`, ini); }
function tag(cls, ...kids) { return h("span", `cc-tag${cls ? " " + cls : ""}`, ...kids); }

function button(label, cls, on) {
  const b = h("button", `dbtn${cls ? " " + cls : ""}`, label);
  b.type = "button";
  b.addEventListener("click", on);
  return b;
}

/// A button inside a row that does its own thing, not the row's.
function rowButton(label, cls, on) {
  return button(label, cls, e => { e.stopPropagation(); on(); });
}

/// One row of Running now, Needs you, Updates or Coming up: a title and one
/// muted line, opening `open` on a click, with `right` beside it.
function line(lead, title, sub, right, open) {
  const t = h("button", "t", h("b", "", title), h("span", "", sub));
  t.type = "button";
  t.title = title;
  const row = h("div", "cc-row cc-line", lead, t, h("span", "r", ...right));
  row.addEventListener("click", open);
  return row;
}

// ---------- Machines ----------

function addMachineButton(cls) {
  const b = button("", cls, openAddMachine);
  b.append(icon(IC.plus), " Add a machine");
  return b;
}

/// One machine per row: name and OS, agents, what it can run, free disk,
/// memory in use and what it is doing now.
function machineTable(machines) {
  const table = h("div", "cc-mt", ...["Machine", "Agents", "Can run", "Disk", "Memory", "Now"].map(t => h("span", "th", t)));
  for (const m of machines) {
    const off = m.online ? "" : " off";
    const name = h("span", `nm${off}`, h("b", "", dot(m), h("span", "", m.name)), h("span", "os", machineOs(m)));
    name.id = `m-${m.id}`;
    name.title = m.name;
    const caps = machineCaps(m);
    const can = h("span", `caps${off}`, caps.join(" · ") || "–");
    if (caps.length) can.title = caps.join(", ");
    const now = h("span", `num${off}`, machineNow(m));
    if (!m.online) now.title = ago(m.seen_at) === "now" ? "Seen just now" : `Seen ${ago(m.seen_at)} ago`;
    const d = machineDisk(m), mem = machineMemory(m);
    const disk = h("span", `num${off}${d.low ? " low" : ""}`, d.text);
    if (d.title) disk.title = d.title;
    const memory = h("span", `num${off}`, mem.text);
    if (mem.title) memory.title = mem.title;
    table.append(name, h("span", `num${off}`, m.online && m.agents ? String(m.agents) : "–"), can, disk, memory, now);
  }
  const mac = thisMacRow(state.mac, machines);
  if (mac) table.append(...thisMacCells(mac));
  return table;
}

/// This Mac's row while the server does not list it: its state across the
/// middle columns and, while it is off, Add under Now.
function thisMacCells(mac) {
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
    state.mac = await invoke("set_mac_control", { enabled: true });
  } catch (e) {
    console.warn("turning on this Mac:", e);
  }
  draw();
  reloadGroup("machines");
  // The link takes a moment; Home looks again soon after.
  for (const ms of [1500, 5000]) setTimeout(refresh, ms);
}

function machinesSection(machines) {
  if (!machines.length && !thisMacRow(state.mac, machines)) {
    return section("sec-machines", "Machines", "", addMachineButton("small"),
      h("div", "cc-panel cc-empty", h("b", "", "No machines linked yet"), addMachineButton("pri")));
  }
  const online = machines.filter(m => m.online).length;
  const count = machines.length ? `${online} of ${machines.length} online` : "";
  return section("sec-machines", "Machines", count, addMachineButton("small"), machineTable(machines));
}

function offlineNote(machines) {
  const gone = machines.filter(m => !m.online && Date.now() - m.seen_at < OFFLINE_NOTE_MS);
  if (!gone.length) return null;
  const names = gone.map(m => m.name).join(", ");
  return h("div", "cc-note", h("span", "ddot bad"), h("span", "", h("b", "", `${names} ${gone.length === 1 ? "is" : "are"} offline.`), " Their sessions left Running now. They come back when beep-node runs there again."));
}

// ---------- Running now ----------

function runningRow(r) {
  const { title, sub } = runningLine(r);
  const right = [r.status === "asking" ? tag("wait", "needs you") : ago(r.started_at)];
  if (r.kind === "research") {
    return line(avatar("beep", icon(IC.find)), title, sub, right, () => invoke("open_research", { session: r.session }).catch(() => {}));
  }
  const ag = agentOf(r.agent);
  // The pane's agent view (app.js) follows and answers it.
  return line(avatar(ag.cls, ag.ini), title, sub, right, () => go("pane", { agent: r }));
}

function runningSection(running) {
  return section("sec-running", "Running now", running.length, null, h("div", "cc-panel", ...running.map(runningRow)));
}

// ---------- Needs you ----------

function cardKey(n) { return n.kind === "chat" ? `chat:${n.pending.query_id}` : `agent:${n.machine_id}:${n.session}:${n.question.id}`; }

function setOutcome(key, outcome) { state.outcomes.set(key, outcome); draw(); }

function failure(e) {
  const text = String(e);
  return conflict(e) ? "Already answered" : text.length > 90 ? text.slice(0, 90) + "..." : text;
}
function conflict(e) { return /^conflict/i.test(String(e)); }

function answerChat(n, decision, said) {
  const key = cardKey(n);
  setOutcome(key, { said });
  // The call returns when the run it resumes ends, so the outcome shows now.
  invoke("answer_approval", { session: n.session_id, queryId: n.pending.query_id, decision, onEvent: new Channel() })
    .then(() => refresh())
    .catch(e => setOutcome(key, { error: failure(e), final: conflict(e) }));
}

/// Opens the session an agent's question came from, where it is answered.
function openAsking(n) {
  const r = state.home?.running?.find(r => r.kind === "agent" && r.machine_id === n.machine_id && r.id === n.session);
  go("pane", { agent: r || { kind: "agent", machine_id: n.machine_id, machine_name: n.machine_name, id: n.session, agent: n.agent, title: n.title, cwd: "", status: "asking", external: false } });
}

/// Allow answers a chat's approval from the row; the chat itself has the
/// details, Always allow and Deny. Answer opens the agent's session.
function needsRow(n) {
  const out = state.outcomes.get(cardKey(n));
  const { title, sub, button: label } = needsLine(n);
  let lead, open, act;
  if (n.kind === "chat") {
    lead = avatar("beep", icon(IC.chat));
    open = () => go("pane", { chat: { session: n.session_id } });
    act = () => answerChat(n, "allow", "Allowed");
  } else {
    const ag = agentOf(n.agent);
    lead = avatar(ag.cls, ag.ini);
    open = act = () => openAsking(n);
  }
  let right;
  if (out?.said) right = [h("span", "ok", out.said)];
  else if (out?.final) right = [h("span", "err", out.error)];
  else right = [rowButton(label, "pri small", act)];
  const row = line(lead, title, sub, right, open);
  if (out?.error && !out.final) row.querySelector(".t > span").replaceChildren(h("span", "err", out.error));
  if (out?.said) row.style.opacity = ".55";
  return row;
}

function needsSection(needs) {
  return section("sec-needs", "Needs you", needs.length, null, h("div", "cc-panel", ...needs.map(needsRow)));
}

// ---------- Coming up ----------

function comingSection(rows) {
  return section("sec-coming", "Coming up", rows.length, null, h("div", "cc-panel", ...rows.map(c => {
    const w = whenOf(c.next_run_at);
    return line(h("span", "cc-when", h("b", "", w.time), w.day), c.label.trim() || c.prompt.trim() || "Routine", scheduleText(c.schedule), [], () => go("pane", "routines"));
  })));
}

// ---------- Updates ----------

/// When an update came: "09:41" today, then "Yesterday", "Thu" this week, "3 Oct".
function updateTime(ms) {
  const at = new Date(ms);
  const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(ms).setHours(0, 0, 0, 0)) / 86400000);
  if (days <= 0) return at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  if (days === 1) return "Yesterday";
  if (days < 7) return at.toLocaleDateString([], { weekday: "short" });
  return at.toLocaleDateString([], { day: "numeric", month: "short" });
}

/// Updates dismissed here that a list may still carry, by id.
const dismissedUpdates = new Set();

/// Takes updates off Home at once and tells the server; a failed dismiss
/// brings them back on the next refresh.
function dismissUpdates(ids) {
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
async function openUpdate(u) {
  dismissUpdates([u.id]);
  go("pane", { chat: { session: null } });
  try {
    await pane()?.draft(u.summary);
  } catch (e) {
    console.warn("opening an update:", e);
  }
}

function updateRow(u) {
  const letter = (u.app_name || "").slice(0, 1).toUpperCase();
  const close = rowButton(icon(IC.x), "quiet", () => dismissUpdates([u.id]));
  close.setAttribute("aria-label", "Dismiss");
  close.style.padding = "2px 6px";
  const sub = u.app_name ? `${u.app_name} · ${updateTime(u.at)}` : updateTime(u.at);
  const row = line(avatar("beep", letter || icon(IC.chat)), u.summary, sub, [close], () => openUpdate(u));
  row.querySelector(".t").title = "Ask Beep about this";
  return row;
}

/// What the event triage agent found in the user's apps, apart from any
/// chat. Hidden while there is nothing.
function updatesSection(updates) {
  const clear = button("Clear", "quiet small", () => dismissUpdates(updates.map(u => u.id)));
  return section("sec-updates", "Updates", updates.length, clear, h("div", "cc-panel", ...updates.map(updateRow)));
}

// ---------- Open PRs and issues ----------

function openUrl(url) {
  invoke("open_url", { url }).catch(e => console.warn("opening a link:", e));
}

/// Review prefills Start a coding agent: in the project's folder on an
/// online machine when it is imported, else on Auto.
function reviewItem(item, project) {
  const start = reviewStart(item, project, state.home?.machines);
  go("pane", { agentnew: start });
}

/// Reply opens a new chat with Beep that asks for help with the item; Beep
/// reads it with its GitHub tools, and posting asks first.
async function replyItem(item) {
  go("pane", { chat: { session: null } });
  try {
    await pane()?.compose(replyPrompt(item));
  } catch (e) {
    console.warn("starting a reply:", e);
  }
  reloadChats();
}

function openItemRow(item, project) {
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

/// One project's row, its counts and its newest items.
function openProjectRows(p, i) {
  const counts = [p.prs ? plural(p.prs, "PR", "PRs") : null, p.issues ? plural(p.issues, "issue", "issues") : null]
    .filter(Boolean).map(t => h("span", "", t));
  const right = [...counts, p.waiting ? tag("wait", `${p.waiting} waiting on you`) : null, h("span", "age", ago(p.last))];
  const sub = p.project ? p.host : `${p.host} · not imported`;
  const head = line(h("span", "cc-pdot"), p.repo, sub, right, () => (p.project ? openProjects(p.project.name) : openUrl(p.url)));
  head.classList.add("cc-gr");
  head.querySelector(".cc-pdot").style.background = PROJECT_COLORS[i % PROJECT_COLORS.length];
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

/// Open PRs and issues on the user's git hosts, one row per project with
/// its items under it. A host that failed says why above the rows.
function gitSection(view) {
  const projects = projectsWithOpen(view, projectRows() || []);
  const notes = view.hosts.filter(x => x.error).map(x =>
    h("div", "cc-note", h("span", "ddot bad"), h("span", "", x.error)));
  const body = projects.length
    ? projects.flatMap(openProjectRows)
    : [h("div", "cc-empty", h("span", "", notes.length ? "Nothing to show from the hosts that answered." : "No open pull requests or issues."))];
  const total = projects.reduce((n, p) => n + p.items.length, 0);
  const count = projects.length ? `${total} in ${plural(projects.length, "project", "projects")}` : "";
  return section("sec-git", openTitle(view.hosts), count, null, h("div", "cc-panel cc-open", ...notes, ...body));
}

let readingGit = false;
/// Asks the server for the open items; it keeps them a few minutes, so
/// this runs less often than Home's refresh.
async function refreshGit() {
  if (readingGit) return;
  readingGit = true;
  try {
    state.git = await invoke("git_items");
  } catch (e) {
    console.warn("open PRs and issues:", e);
  } finally {
    readingGit = false;
  }
  if (shown === "home") draw();
}

// ---------- Views ----------

const pane = () => window.beepPane;
const views = new Map();
let shown = null;

/// Adds a view the window can show. `mount(el, arg)` draws it into its own
/// element, `unmount()` runs when another view takes over. `title` is a
/// string or a function giving `{ title, sub, tools }`.
export function registerView(name, view) {
  let el = view.el;
  if (!el) {
    el = h("div", "cc-box");
    el.hidden = true;
    $("#main").append(el);
  }
  views.set(name, { ...view, el });
}

/// Shows view `name`, passing `arg` to its mount. The others are hidden,
/// not destroyed, so a reply streaming in the pane keeps going under Home.
export function go(name, arg) {
  // Settings open over whatever shows, which stays as it is.
  if (name === "pane" && arg === "settings") {
    if (!shown) go("home");
    return openSettings();
  }
  const view = views.get(name);
  if (!view) return;
  if (shown && shown !== name) views.get(shown).unmount?.();
  shown = name;
  for (const [n, v] of views) v.el.hidden = n !== name;
  view.mount(view.el, arg);
  drawTop();
  drawSide();
}

function drawTop() {
  const view = views.get(shown);
  const head = typeof view?.title === "function" ? view.title() : { title: view?.title ?? "Beep" };
  $("#title").textContent = head.title || "Beep";
  $("#date").textContent = head.sub || "";
  $("#tools").replaceChildren(...(head.tools || []));
}

// ---------- Sidebar ----------

const groups = new Map();

/// Adds a sidebar group. `load()` gives its items, or null to hide the
/// group; `row(item)` draws one; `onPick(item)` runs on a click;
/// `onTitle()`, when given, makes the title a button. Groups sort by
/// `order`.
export function registerSidebarGroup(name, group) {
  groups.set(name, { title: "", ...group, items: null, loading: false, again: false });
  reloadGroup(name);
}

/// Loads a group's items again. Never two loads at once: one asked for
/// during a load runs once more after it.
async function reloadGroup(name) {
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

function sideItem(svg, label, count, opts = {}) {
  const b = h("button", `cc-si${opts.cls ? " " + opts.cls : ""}`, svg ? icon(svg) : null, h("span", "tt", label));
  b.type = "button";
  if (opts.current) b.setAttribute("aria-current", "true");
  if (count) b.append(h("span", `n${opts.hot ? " hot" : ""}`, String(count)));
  if (opts.kbd) b.append(h("kbd", "", opts.kbd));
  if (opts.on) b.addEventListener("click", opts.on);
  return b;
}

/// Element Home scrolls to once it has drawn.
let scrollTarget = null;

function jump(id) {
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
function paneOn() {
  return shown === "pane" && pane() ? pane().current() : { view: null, session: null, group: null };
}

function drawSide() {
  const home = state.home;
  const head = h("div", "cc-head", "");
  head.setAttribute("data-tauri-drag-region", "");
  const top = h("div", "cc-grp",
    sideItem(IC.find, "Search", 0, { cls: "cc-search", on: openPalette, kbd: "⌘K" }),
    sideItem(IC.home, "Home", home?.needs_you.length, { current: shown === "home", hot: true, on: () => go("home") }));
  if (home?.running) top.append(sideItem(IC.run, "Running", home.running.length, { on: () => jump("sec-running") }));
  if (home?.coming_up) top.append(sideItem(IC.cal, "Coming up", home.coming_up.length, { on: () => jump("sec-coming") }));
  const side = [head, h("div", "cc-brand", h("b", "", "Beep")), top];
  for (const [name, g] of [...groups].sort((a, b) => a[1].order - b[1].order)) {
    // Rows that fill in once the server answers, never an empty sidebar.
    if (name === "chats" && g.items == null && !home) side.push(h("div", "cc-grp", h("h3", "", "Chats"), ...skeleton()));
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
      if (g.onPick) el.addEventListener("click", () => g.onPick(item));
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
  $("#side").replaceChildren(...side);
  // A chat's title comes from the list, which may just have loaded.
  if (shown === "pane") drawTop();
}

/// A group title that opens what the group lists.
function titleButton(title, on) {
  const b = h("button", "cc-gt", title);
  b.type = "button";
  b.addEventListener("click", on);
  return b;
}

function skeleton() {
  return [70, 55, 62].map(w => {
    const row = h("div", "cc-skel");
    row.style.width = `${w}%`;
    return row;
  });
}

function footer(machines) {
  const p = problem();
  if (p) return [h("span", "", h("span", `ddot ${p.kind === "unlinked" ? "off" : "bad"}`), ` ${{ unlinked: "Not linked", revoked: "Removed" }[p.kind] ?? "Refused"}`)];
  if (!state.home) return [h("span", "", h("span", `ddot ${stuck() ? "bad" : "off"}`), stuck() ? " Not connected" : " Connecting…")];
  if (!machines) return ["Machines unavailable"];
  if (!machines.length) return ["No machines linked"];
  const on = machines.filter(m => m.online).length;
  return [h("span", "", h("span", "ddot"), " ", h("b", "", String(on)), ` of ${plural(machines.length, "machine", "machines")} online`)];
}

/// Pinned chats and groups, then the newest of the rest, the open one
/// highlighted.
const SIDE_CHATS = 5;
let chatList = [];
let chatFetch = null;

/// One fetch feeds both groups, even when both ask at once.
function fetchChats() {
  if (chatFetch) return chatFetch;
  const mine = pane().chatRows().then(rows => {
    if (chatFetch === mine) chatList = rows;
    return rows;
  }).finally(() => { if (chatFetch === mine) chatFetch = null; });
  return chatFetch = mine;
}

function reloadChats() {
  // A fetch that started before this change may miss it: the next one is new.
  chatFetch = null;
  return Promise.all([reloadGroup("pinned"), reloadGroup("chats")]);
}

/// Pinned projects join the Pinned group here once projects can be pinned:
/// return items shaped like chat rows ({ kind: "project", id, title }) and
/// give `chatRow` and the group's `onPick` a branch for them.
async function pinnedProjects() { return []; }

/// Gives the pin button of chat `id` the focus again, unless focus has gone
/// elsewhere meanwhile (a redraw leaves it on the page body).
function restorePinFocus(id) {
  const on = document.activeElement;
  if (on && on !== document.body) return;
  document.querySelector(`.cc-chat .pa[data-id="${CSS.escape(id)}"]`)?.focus();
}

async function setPinned(c, pinned) {
  const active = document.activeElement;
  const hadFocus = active?.classList.contains("pa") && active.dataset.id === c.id;
  try {
    await invoke(c.kind === "group" ? "pin_group" : "pin_session", { id: c.id, pinned });
  } catch (e) {
    console.warn("pinning a chat:", e);
  }
  await reloadChats();
  if (hadFocus) restorePinFocus(c.id);
}

function chatRow(c) {
  const on = paneOn();
  const open = c.kind === "group" ? on.group === c.id : on.view === "chat" && on.session === c.id;
  const mark = c.kind === "group" ? icon(IC.group) : icon(c.research ? IC.doc : IC.chat);
  const toggle = h("button", "pa", icon(IC.pin));
  toggle.type = "button";
  toggle.dataset.id = c.id;
  toggle.setAttribute("aria-label", c.pinned ? "Unpin" : "Pin to top");
  toggle.addEventListener("click", e => { e.stopPropagation(); setPinned(c, !c.pinned); });
  // Two real buttons in one row, never one inside the other.
  const openBtn = h("button", "open", mark, h("span", "tt", c.title));
  openBtn.type = "button";
  openBtn.title = c.title;
  if (open) openBtn.setAttribute("aria-current", "true");
  return h("div", "cc-si cc-chat", openBtn, toggle);
}

function pickChat(c) {
  if (c.kind === "group") return go("pane", { group: { id: c.id } });
  // A research opens in its own window, as from the bar's list.
  if (c.research) return invoke("open_research", { session: c.id }).catch(() => go("pane", { chat: { session: c.id } }));
  go("pane", { chat: { session: c.id } });
}

registerSidebarGroup("pinned", {
  order: 2,
  title: "Pinned",
  async load() {
    if (!pane()) return [];
    return [...splitPinned(await fetchChats(), SIDE_CHATS).pinned, ...await pinnedProjects()];
  },
  row: chatRow,
  onPick: pickChat,
});

registerSidebarGroup("chats", {
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
    if (!pane()) return [];
    return splitPinned(await fetchChats(), SIDE_CHATS).recent;
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
function plusButton(label, on) {
  const b = h("button", "", icon(IC.plus));
  b.type = "button";
  b.title = label;
  b.setAttribute("aria-label", label);
  b.addEventListener("click", e => { e.stopPropagation(); on(); });
  return b;
}

/// An empty group's row: what is missing. The group's + adds one.
function emptyRow(text) {
  return h("div", "cc-si none", text);
}

/// Opens the Add a machine sheet; Home refreshes once it closes.
export function openAddMachine() {
  openAddMachineSheet({ onClose: refresh });
}

/// Opens the Projects page, filtered to `name` when given.
export function openProjects(name) {
  go("projects", { project: name || null });
}

/// Opens a project once it is in: Home and the found folders load again.
function landed(p) {
  state.picked = p.name;
  state.sessions.delete(p.name);
  state.where.delete(p.name);
  loadFound();
  refresh();
  openProjects(p.name);
}

/// Opens the Import a project sheet, at the folder choice with `choose`;
/// the project is selected once it is in. `repo` and `machine` fill it in,
/// as Clone here does.
function importSheet(choose = false, repo = null, machine = null) {
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
let finding = false;
async function loadFound() {
  if (finding) return;
  finding = true;
  try {
    state.found = await invoke("found_projects");
  } catch (e) {
    console.warn("found projects:", e);
    state.found = state.found || [];
  } finally {
    finding = false;
  }
  if (shown === "projects" && !state.picked) drawProjects(views.get("projects").el);
}

/// Imports a found row from the Projects page and opens it.
async function importFromPage(row) {
  if (state.importing) return;
  state.importing = row.name;
  state.foundError = null;
  drawProjects(views.get("projects").el);
  try {
    const project = await importFound(row);
    state.importing = null;
    landed(project);
  } catch (e) {
    state.importing = null;
    state.foundError = { name: row.name, message: String(e).replace(/^(bad request|error):\s*/i, "") };
    if (shown === "projects") drawProjects(views.get("projects").el);
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
  load: async () => projectRows()?.map((p, i) => ({ ...p, i })) ?? null,
  row(p) {
    const b = sideItem(null, p.name, p.agents, { current: shown === "projects" && state.picked === p.name });
    b.title = p.machines.map(m => `${m.name}: ${m.folder}`).join("\n") || p.name;
    const sw = h("span", "cc-pdot");
    sw.style.background = PROJECT_COLORS[p.i % PROJECT_COLORS.length];
    b.prepend(sw);
    return b;
  },
  onPick: p => openProjects(p.name),
});

// The machines Home lists, each with its online dot. It shows once Home has
// loaded, empty or not, unless the server is too old to list machines.
registerSidebarGroup("machines", {
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
    if (m.thisMac) return thisMacItem(m.thisMac);
    const b = sideItem(null, m.name, m.agents || 0);
    b.prepend(dot(m));
    b.title = `${m.name}: ${m.online ? (m.agents ? plural(m.agents, "agent", "agents") + " running" : "online") : "offline"}`;
    return b;
  },
  onPick: m => jump(m.thisMac ? "m-this-mac" : `m-${m.id}`),
});

/// The sidebar's This Mac: its state, or Add while it is off.
function thisMacItem(mac) {
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
function page(home, wide) {
  const { machines, running, coming_up: coming, needs_you: needs } = home;
  const out = [];
  if (state.error) out.push(h("div", "cc-note", h("span", "ddot bad"), h("span", "cc-err", `Could not refresh: ${state.error}`)));
  if (!machines && !running) {
    out.push(h("div", "cc-note", h("span", "ddot off"), h("span", "", "Machines and running sessions aren't available for this account or server.")));
  } else if (machines) {
    const note = offlineNote(machines);
    if (note) out.push(note);
  }
  const make = {
    machines: () => machinesSection(machines),
    running: () => runningSection(running),
    needs: () => needsSection(needs),
    updates: () => updatesSection(home.updates),
    coming: () => comingSection(coming),
    projects: () => projectsSection(projectRows() || []),
    git: () => gitSection(state.git),
  };
  const laid = { ...home, git: state.git };
  const cols = wide && homeColumns(laid);
  if (cols) {
    const col = keys => h("div", "", ...keys.map(k => make[k]()));
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
function problem() { return state.home ? null : connectProblem(state.error); }

/// What is wrong, said plainly, with the way to fix it and a Retry.
function problemView(p) {
  const acts = h("div", "acts");
  if (p.action) acts.append(button(p.action, "pri", () => go("pane", p.view)));
  const retry = button("Try again", p.action ? "" : "pri", () => { refresh(); retry.disabled = true; });
  retry.classList.add("retry");
  retry.disabled = loading;
  acts.append(retry, button("Show log", "", () => invoke("open_log").catch(e => console.warn("opening the log:", e))));
  const box = h("div", "cc-conn stuck", h("span", `ddot ${p.kind === "unlinked" ? "off" : "bad"}`), h("b", "", p.title), h("span", "why", p.text), acts);
  box.dataset.problem = `${p.kind}:${p.text}`;
  return box;
}

/// What Home shows before the server first answers: pulsing dots, never a
/// blank page. Once stuck it says so and offers Retry; retries go on anyway.
function connecting() {
  const p = problem();
  if (p) return problemView(p);
  const pulse = h("div", "cc-pulse", h("i"), h("i"), h("i"));
  if (!stuck()) {
    return h("div", "cc-conn", pulse, h("b", "", "Connecting to Beep…"), h("span", "", "Your chats, machines and approvals load once the server answers."));
  }
  const secs = Math.round((Date.now() - state.since) / 1000);
  const why = h("span", "why", `The server has not answered for ${secs} seconds. Beep keeps trying every few seconds; your chats show up as soon as it answers.`);
  const retry = button("Retry now", "pri", () => { refresh(); retry.disabled = true; });
  retry.disabled = loading;
  const log = button("Show log", "", () => invoke("open_log").catch(e => console.warn("opening the log:", e)));
  const box = h("div", "cc-conn stuck", pulse, h("b", "", "Still connecting to Beep"), why, h("div", "acts", retry, log));
  return box;
}

/// Draws the waiting view, or brings the one on screen up to date so its
/// buttons keep their focus.
function drawConnecting(view) {
  const on = view.querySelector(":scope > .cc-conn");
  const p = problem(), was = on?.dataset.problem;
  if (p || was) {
    if (p && was === `${p.kind}:${p.text}`) {
      on.querySelector(".acts .retry").disabled = loading;
      return;
    }
    return view.replaceChildren(connecting());
  }
  if (on && on.classList.contains("stuck") === stuck()) {
    if (!stuck()) return;
    const secs = Math.round((Date.now() - state.since) / 1000);
    on.querySelector(".why").textContent = `The server has not answered for ${secs} seconds. Beep keeps trying every few seconds; your chats show up as soon as it answers.`;
    on.querySelector(".acts .pri").disabled = loading;
    return;
  }
  view.replaceChildren(connecting());
}

function draw() {
  const home = state.home, view = $("#view");
  drawSide();
  if (!home) return drawConnecting(view);
  const top = view.scrollTop;
  drawnWide = isWide(view);
  view.replaceChildren(...page(home, drawnWide));
  view.scrollTop = top;
  applyScroll();
  scrollTarget = null;
  $("#home-ask").placeholder = home.machines?.length ? "Ask Beep, or start an agent on a machine" : "Ask Beep anything";
  $("#home-agent").style.display = home.machines?.some(m => m.online) ? "" : "none";
}

/// Whether Home has room for two columns.
function isWide(view) { return view.clientWidth >= WIDE_HOME_PX; }
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

// ---------- Projects ----------

function importButton(cls) {
  const b = button("", cls, addProject);
  b.append(icon(IC.plus), " Add a project");
  return b;
}

/// One project per row: its name and host, where it is on each machine,
/// the agents running in it and when one last did something.
function projectTable(rows) {
  const table = h("div", "cc-mt cc-pt", ...["Project", "Machine and folder", "Agents", "Last activity"].map(t => h("span", "th", t)));
  for (const p of rows) {
    const open = h("button", "", h("span", "", p.name));
    open.type = "button";
    open.title = p.url || p.name;
    open.addEventListener("click", () => openProjects(p.name));
    const name = h("span", "nm", h("b", "", open), h("span", "os", p.imported ? hostOf(p.url) : "From running agents"));
    name.id = `p-${p.name}`;
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

/// One project: where it lives on every machine, with "Start an agent here"
/// per folder and "Clone here" where it is not, and the coding agents
/// running in it now.
function projectDetail(p) {
  const machines = p.imported ? state.where.get(p.name) : null;
  if (p.imported && machines === undefined) loadWhere(p.name);
  const rows = whereRows(p, machines || null, state.mac?.id || null);
  const where = h("div", "cc-mt cc-pt", ...["Machine", "Folder", ""].map(t => h("span", "th", t)));
  where.style.gridTemplateColumns = "minmax(0,1fr) minmax(0,2fr) auto";
  for (const m of rows) where.append(...whereRow(p, m));
  const whereEl = section("sec-where", "Where it lives", rows.filter(m => m.kind === "here").length, null, where);
  if (machines) whereEl.querySelector("h4").append(h("small", "", `on ${whereCount(rows, true)}`));
  const out = [whereEl];
  const inside = (r, m) => r.machine_id === m.id && (r.cwd === m.folder || r.cwd?.startsWith(m.folder + "/"));
  const running = (state.home?.running || []).filter(r => p.machines.some(m => inside(r, m)));
  const list = running.length
    ? h("div", "cc-panel", ...running.map(runningRow))
    : h("div", "cc-panel cc-empty", h("span", "", "No agent is working in this project right now."));
  out.push(section("sec-project-agents", "Agents", running.length || "", null, list));
  out.push(sessionsSection(p, running));
  return [...agentTop(p), ...out, ...agentBottom(p)];
}

// ---------- Project agent ----------

/// What the agent sections are doing: `busy` and `errors` by task or worker
/// id, `outcomes` of answered cards by query id, `sending` the text on its
/// way to the project chat, `starting` the project whose chat is being made
/// and `startError` why that failed. `remembered` is set once the window
/// made the project chat its current chat.
const pa = { busy: new Set(), errors: new Map(), outcomes: new Map(), sending: null, starting: null, startError: null, remembered: false };

function redrawProjects() {
  if (shown === "projects") drawProjects(views.get("projects").el);
}

const agentActs = {
  busy: id => pa.busy.has(id),
  error: id => pa.errors.get(id),
  outcome: id => pa.outcomes.get(id),
  openUrl: url => invoke("open_url", { url }).catch(e => console.warn("open url:", e)),
  async change(task, change) {
    await act(task.id, () => invoke("center_update_task", { task: task.id, change }));
  },
  async stop(worker) {
    await act(worker.id, () => invoke("center_stop_worker", { worker: worker.id }));
  },
};

/// Runs `call` for row `id`, showing it busy, then reloads the project.
async function act(id, call) {
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

/// Tasks beside Workers, above the folders, once the project has an agent.
function agentTop(p) {
  const view = p.imported ? state.pview.get(p.name)?.view : null;
  if (!view) return [];
  const out = [];
  const error = state.pview.get(p.name)?.error;
  if (error) out.push(h("div", "cc-note", h("span", "ddot bad"), h("span", "cc-err", `Could not refresh the project: ${error}`)));
  out.push(h("div", "pj-cols", tasksSection(view, agentActs), workersSection(view, agentActs, IC.chat)));
  return out;
}

/// The project chat at the bottom, or the call to start it. Nothing from a
/// server without project agents, or for a project that is not imported.
function agentBottom(p) {
  if (!p.imported || !state.agents) return [];
  const agent = agentFor(state.agents, p.name);
  if (!agent) {
    return [startSection({ starting: pa.starting === p.name, error: pa.startError, start: () => startProjectChat(p.name) })];
  }
  const view = state.pview.get(p.name)?.view;
  if (!view) return [section("sec-project-chat", "Project chat", "", null, h("div", "cc-panel cc-empty", h("span", "", "Reading the project chat…")))];
  return [chatSection(view, {
    ...agentActs,
    sending: pa.sending?.name === p.name ? pa.sending.text : null,
    answer: (c, decision, said) => answerCard(agent, c, decision, said),
    openChat: () => go("pane", { chat: { session: agent.chat_id } }),
  })];
}

/// Makes the project's agent and its chat, then shows them.
async function startProjectChat(name) {
  if (pa.starting) return;
  pa.starting = name;
  pa.startError = null;
  redrawProjects();
  try {
    const agent = await invoke("center_open_project", { name });
    state.agents = [...(state.agents || []).filter(a => a.name !== agent.name), agent];
  } catch (e) {
    pa.startError = failure(e);
  } finally {
    pa.starting = null;
  }
  redrawProjects();
  loadProjectAgent();
}

/// Answers a card of the project chat. The call returns when the run it
/// resumes ends, so the outcome shows now and the view reloads after.
function answerCard(agent, c, decision, said) {
  pa.outcomes.set(c.query_id, { said });
  redrawProjects();
  invoke("answer_approval", { session: agent.chat_id, queryId: c.query_id, decision, onEvent: new Channel() })
    .catch(e => pa.outcomes.set(c.query_id, { error: failure(e) }))
    .finally(() => { redrawProjects(); loadProjectAgent(); });
  // The tasks a plan starts show before its run ends.
  setTimeout(loadProjectAgent, 1500);
}

/// The composer under a project with an agent; kept across redraws so
/// what is typed stays.
const projectComp = (() => {
  const input = h("input");
  input.type = "text";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.placeholder = "Ask the project agent, or describe new work";
  input.setAttribute("aria-label", "Ask the project agent");
  const note = h("span", "cc-err");
  note.setAttribute("role", "status");
  const send = h("button", "dbtn pri", "Send", h("kbd", "", "⏎"));
  send.type = "submit";
  const form = h("form", "cc-comp", input, note, send);
  form.addEventListener("submit", e => {
    e.preventDefault();
    sendToProject(input, note);
  });
  return form;
})();

async function sendToProject(input, note) {
  const agent = agentFor(state.agents, state.picked);
  const text = input.value.trim();
  if (!agent || !text || pa.sending) return;
  note.textContent = "";
  input.value = "";
  pa.sending = { name: agent.name, text };
  pa.remembered = true;
  redrawProjects();
  // The turn shows once the server has it, before the reply ends.
  setTimeout(loadProjectAgent, 1500);
  try {
    await invoke("center_project_send", { chatId: agent.chat_id, text, onEvent: new Channel() });
  } catch (e) {
    if (!input.value) input.value = text;
    note.textContent = failure(e);
  } finally {
    pa.sending = null;
  }
  redrawProjects();
  loadProjectAgent();
  reloadChats();
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
      state.agents = await invoke("center_projects");
    } catch (e) {
      console.warn("project agents:", e);
      if (state.agents === undefined) state.agents = null;
    }
    const agent = agentFor(state.agents, state.picked);
    if (agent) {
      const was = state.pview.get(agent.name);
      try {
        const view = await invoke("center_project", { projectId: agent.project_id, chatId: agent.chat_id });
        state.pview.set(agent.name, { view, error: null });
      } catch (e) {
        state.pview.set(agent.name, { view: was?.view ?? null, error: failure(e) });
      }
    }
  } finally {
    agentLoading = false;
  }
  redrawProjects();
  if (agentAgain) { agentAgain = false; loadProjectAgent(); }
}

/// One machine's row under "Where it lives": its folder and what to do
/// there, or why there is none.
function whereRow(p, m) {
  const name = h("span", "nm", h("b", "", m.name));
  if (m.kind === "here") {
    const start = button("Start an agent here", "small", () => go("pane", { agentnew: { prompt: "", at: { machine: m.id, cwd: m.folder } } }));
    const copy = button("Copy path", "quiet small", async () => {
      try { await navigator.clipboard.writeText(m.folder); copy.textContent = "Copied"; } catch (e) { console.warn("copy:", e); }
    });
    const finder = m.mac ? button("Open in Finder", "quiet small", () => invoke("open_folder", { path: m.folder }).catch(e => console.warn("open folder:", e))) : null;
    const folder = h("span", "nm", h("span", "os", m.folder));
    folder.title = m.folder;
    return [name, folder, h("span", "acts", finder, copy, start)];
  }
  const said = h("span", "os", m.text);
  if (m.kind === "none" && m.checked_at) said.title = `Checked ${ago(m.checked_at)} ago`;
  const what = h("span", "nm", said);
  const clone = m.kind === "none" && p.url ? button("Clone here", "small", () => importSheet(false, repoFromRemote(p.url), m.id)) : null;
  return [name, what, h("span", "acts", clone)];
}

/// Asks the server where `name` lives on each machine; while it looks
/// through one, it asks again shortly. A folder it finds brings that
/// machine's sessions with it.
let readingWhere = null;
let whereTimer = null;
async function loadWhere(name) {
  if (readingWhere === name) return;
  readingWhere = name;
  const before = state.where.get(name);
  let got = null;
  try {
    got = await invoke("project_machines", { project: name });
  } catch (e) {
    console.warn("project machines:", e);
  } finally {
    readingWhere = null;
  }
  state.where.set(name, got);
  const folders = list => (list || []).filter(m => m.folder).map(m => `${m.id} ${m.folder}`).join("\n");
  if (before !== undefined && folders(before) !== folders(got)) {
    state.sessions.delete(name);
    refresh();
  }
  clearTimeout(whereTimer);
  if (got?.some(m => m.checking)) {
    whereTimer = setTimeout(() => {
      if (shown === "projects" && state.picked === name) loadWhere(name);
    }, 3000);
  }
  if (shown === "projects" && state.picked === name) drawProjects(views.get("projects").el);
}

/// The sessions your coding agents ran in the project's folders, newest
/// first, each resumable; those running now are under Agents.
function sessionsSection(p, running) {
  const sessions = state.sessions.get(p.name);
  if (sessions === undefined && p.imported) loadSessions(p.name);
  let body;
  if (!p.imported) body = h("div", "cc-panel cc-empty", h("span", "", "Import this project to see the sessions your agents ran in it."));
  else if (sessions === undefined) body = h("div", "cc-panel cc-empty", h("span", "", "Reading sessions…"));
  else {
    const past = pastSessions(sessions, running);
    body = past.length
      ? h("div", "cc-panel pv-scroll", ...past.map(sessionRow))
      : h("div", "cc-panel cc-empty", h("span", "", "No sessions from Claude Code, Codex, pi or flsh in these folders yet."));
  }
  const el = section("sec-sessions", "Sessions", sessions?.length || "", null, body);
  el.querySelector("h4").append(h("small", "", "from your coding agents in its folders on every machine, newest first"));
  return el;
}

/// The agent view for a session row.
function openAgentRow(r) { go("pane", { agent: { kind: "agent", ...r } }); }

function sessionRow(s) {
  const ended = s.status === "ended";
  const said = h("span", "bad");
  const act = ended
    ? button("Resume", "small", async () => {
      act.disabled = true;
      act.textContent = "Resuming…";
      said.textContent = "";
      try {
        const started = await invoke("agent_resume", { machineId: s.machine_id, agent: s.agent, session: s.id, prompt: null });
        state.sessions.delete(state.picked);
        openAgentRow(started.session);
      } catch (e) {
        act.disabled = false;
        act.textContent = "Resume";
        said.textContent = String(e).replace(/^(bad request|error):\s*/i, "");
      }
    })
    : button("Open", "small", () => openAgentRow(s));
  const meta = [h("span", "ap-ag", agentLabel(s.agent)), ended ? "ended" : s.status, s.machine_name, base(s.cwd), s.last_activity ? ago(s.last_activity) : ""]
    .filter(Boolean).map(x => typeof x === "string" ? h("span", "", x) : x);
  return h("div", "pv-row", h("span", "t", s.title || "Untitled session"), h("span", "m", ...meta), h("span", "acts", said, act));
}

let readingSessions = null;
async function loadSessions(name) {
  if (readingSessions === name) return;
  readingSessions = name;
  let got = [];
  try {
    got = await invoke("project_sessions", { project: name });
  } catch (e) {
    console.warn("project sessions:", e);
  } finally {
    readingSessions = null;
  }
  state.sessions.set(name, got);
  if (shown === "projects" && state.picked === name) drawProjects(views.get("projects").el);
}

/// The checkouts the machines found that are not projects yet: the first
/// three, each with Import; the sheet lists them all.
function foundSection() {
  if (!state.found?.length) return null;
  injectAddProjectStyle();
  const list = state.found.slice(0, 3).map(row => foundRow(row,
    { busy: state.importing, error: state.foundError?.name === row.name ? state.foundError.message : null }, importFromPage));
  const more = state.found.length > 3 ? button(`All ${state.found.length}`, "quiet small", addProject) : null;
  const el = section("sec-found", "Found on your machines", "", more, h("div", "cc-panel ap-list", ...list));
  el.querySelector("h4").append(h("small", "", "folders your coding agents worked in, without a project"));
  return el;
}

/// "github.com" from a project's web page.
function hostOf(url) {
  try { return new URL(url).host; } catch { return ""; }
}

function drawProjects(el) {
  const rows = projectRows();
  const view = h("div", "cc-view");
  if (!state.home) {
    view.append(h("div", "cc-panel cc-empty", h("b", "", "Loading projects…")));
  } else if (!rows || !rows.length) {
    view.append(h("div", "cc-panel cc-empty",
      h("b", "", "No projects yet"),
      h("span", "", "Add a folder your coding agents worked in, any folder, or a repository to clone."),
      importButton("pri")));
  } else {
    const shownRows = filterProjects(rows, state.picked);
    const one = state.picked && shownRows.length === 1 ? shownRows[0] : null;
    if (one) view.append(...projectDetail(one));
    else view.append(section("sec-projects", "Projects", state.picked ? "" : rows.length, null, projectTable(shownRows.length ? shownRows : rows)));
    const found = one ? null : foundSection();
    if (found) view.append(found);
  }
  // Only the view is drawn again: the composer keeps what is typed and its
  // focus, and the view keeps its scroll.
  const old = el.querySelector(":scope > .cc-view");
  const oldChat = old?.querySelector(".pj-chat");
  // The chat follows its newest turn unless the user scrolled up in it.
  const follow = !oldChat || oldChat.scrollHeight - oldChat.scrollTop - oldChat.clientHeight < 24;
  if (old) {
    old.replaceWith(view);
    view.scrollTop = old.scrollTop;
  } else {
    el.prepend(view);
  }
  const chat = view.querySelector(".pj-chat");
  if (chat) chat.scrollTop = follow ? chat.scrollHeight : oldChat.scrollTop;
  const withAgent = !!(state.picked && agentFor(state.agents, state.picked));
  if (withAgent && projectComp.parentNode !== el) el.append(projectComp);
  if (!withAgent) projectComp.remove();
}

registerView("projects", {
  title() {
    const tools = [importButton("small")];
    if (state.picked) {
      const p = filterProjects(projectRows() || [], state.picked)[0];
      if (p?.url) tools.unshift(button(`Open on ${hostOf(p.url) === "github.com" ? "GitHub" : hostOf(p.url)}`, "small", () => invoke("open_url", { url: p.url }).catch(() => {})));
      tools.unshift(button("All projects", "quiet small", () => openProjects()));
    }
    return { title: state.picked || "Projects", sub: state.picked ? "Project" : "Repositories on your machines", tools };
  },
  mount(el, arg) {
    state.picked = arg?.project || null;
    if (state.picked) { state.sessions.delete(state.picked); state.where.delete(state.picked); }
    pa.startError = null;
    drawProjects(el);
    if (!state.picked) loadFound();
    refresh();
  },
  unmount() {
    // The project chat stops being this window's chat, so its notices show.
    if (pa.remembered) { pa.remembered = false; invoke("new_chat").catch(() => {}); }
  },
});

/// Home's short list of projects: the page has the table.
function projectsSection(rows) {
  const more = button("All projects", "quiet small", () => openProjects());
  const list = rows.slice(0, 6).map(p => line(
    h("span", "cc-pdot"),
    p.name,
    [p.machines.map(m => m.name).join(", "), p.agents ? plural(p.agents, "agent", "agents") : ""].filter(Boolean).join(" · "),
    [p.last ? ago(p.last) : ""],
    () => openProjects(p.name)));
  list.forEach((row, i) => { row.querySelector(".cc-pdot").style.background = PROJECT_COLORS[i % PROJECT_COLORS.length]; });
  return section("sec-projects-home", "Projects", rows.length, more, h("div", "cc-panel", ...list));
}

// ---------- Pane ----------

/// Titles of the pane's views; a chat or group takes its own name.
const PANE_TITLES = {
  chats: ["Chats", "Every chat, from the bar, this window and your phone"],
  memories: ["Memories", "What Beep knows about you"],
  routines: ["Routines", "What Beep does on its own"],
  rnew: ["Routines", "What Beep does on its own"],
  grants: ["Routines", "What Beep does on its own"],
  skills: ["Skills", "What Beep knows how to do"],
  apps: ["Apps", "What Beep can use for you"],
  mcp: ["Apps", "What Beep can use for you"],
  appperms: ["Apps", "What Beep can use for you"],
  settings: ["Settings", ""],
  machines: ["Settings", ""],
  research: ["Research", ""],
  agents: ["Coding agents", "On your machines"],
  agentnew: ["Coding agents", "Start one on a machine"],
  agent: ["Coding agents", "Follow it and answer it here"],
};

function paneTitle() {
  const on = paneOn();
  if (on.view === "chat") {
    const c = chatList.find(x => x.id === on.session);
    // A chat just started is not in the list until the server has it.
    return { title: c?.title ?? "New chat", tools: c ? [pinButton(c)] : [] };
  }
  if (on.group) {
    const g = chatList.find(x => x.id === on.group);
    return { title: g?.title ?? "Group", tools: g ? [pinButton(g)] : [] };
  }
  const [title, sub] = PANE_TITLES[on.view] ?? ["Beep", ""];
  return { title, sub };
}

function pinButton(c) {
  return button(c.pinned ? "Unpin" : "Pin", "", () => setPinned(c, !c.pinned));
}

registerView("pane", {
  el: $("#pane-box"),
  title: paneTitle,
  mount(el, arg) {
    pane()?.mounted(true);
    pane()?.open(arg);
  },
  unmount() {
    pane()?.mounted(false);
    pane()?.leave();
  },
});

/// The window stops showing a chat or group when the pane moves off it, so
/// a focused window no longer keeps their notices quiet.
let last = { view: null, session: null };
$("#bar").dataset.view = pane()?.current().view ?? "";
pane()?.onChange(now => {
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
async function takePending(fallback) {
  const target = await invoke("center_pending").catch(() => null);
  if (target == null) return fallback && go(fallback);
  if (target === "home") return go("home");
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
let settingsOpen = null;

export function openSettings(section) {
  if (settingsOpen) return section && showSection(settingsOpen.scrim, section);
  const sheet = h("div", "cc-set");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Settings");
  sheet.tabIndex = -1;
  const nav = h("nav", "dset-nav", h("b", "", "Settings"));
  const body = h("div", "dset-body", h("div", "dset-top", h("span", "tt", h("b", "", "Settings")), h("kbd", "", "esc")), ...skeletonRows());
  sheet.append(nav, body);
  const scrim = h("div", "cc-set-scrim", sheet);
  const back = document.activeElement;
  scrim.addEventListener("mousedown", e => { if (e.target === scrim) closeSettings(); });
  // While open, keys stay in the sheet: Esc closes it, and nothing reaches
  // the page behind.
  const keys = e => {
    if (e.key === "Escape") { e.preventDefault(); closeSettings(); }
    else if (e.metaKey && e.key === ",") e.preventDefault();
    else if (e.key === "Tab") trapTab(e, sheet);
    e.stopImmediatePropagation();
  };
  window.addEventListener("keydown", keys, true);
  document.body.append(scrim);
  settingsOpen = { scrim, keys, back };
  sheet.focus();
  pane()?.settings(sheet)
    .then(() => { if (section) showSection(scrim, section); })
    .catch(e => console.warn("settings:", e));
}

/// Scrolls the open sheet to `section`, such as "machines".
function showSection(scrim, section) {
  scrim.querySelector(`[data-goto="${section}"]`)?.click();
}

export function closeSettings() {
  if (!settingsOpen) return;
  const { scrim, keys, back } = settingsOpen;
  settingsOpen = null;
  window.removeEventListener("keydown", keys, true);
  scrim.remove();
  if (back?.isConnected) back.focus();
}

function skeletonRows() {
  return [80, 60, 72, 50].map(w => {
    const row = h("div", "cc-skel");
    row.style.width = `${w}%`;
    return row;
  });
}

/// Keeps Tab inside the sheet.
function trapTab(e, sheet) {
  const all = [...sheet.querySelectorAll("button:not([disabled]), [href], [tabindex]:not([tabindex='-1'])")];
  if (!all.length) return;
  const first = all[0], last = all[all.length - 1];
  // Focus lost to the page (a redraw removed it): bring it back in.
  if (!sheet.contains(document.activeElement)) { e.preventDefault(); return (e.shiftKey ? last : first).focus(); }
  if (e.shiftKey && (document.activeElement === first || document.activeElement === sheet)) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
}

window.beepOpenSettings = openSettings;
/// A page Settings leads to, such as Apps: the sheet closes and the pane
/// shows it; null only closes.
window.beepSettingsAway = view => {
  closeSettings();
  if (view) go("pane", view);
};

// ---------- Keys ----------

initSideResize($(".cwin"), $("#side-grip"));

initPalette({ go, jump, pane, findIcon: IC.find });

document.addEventListener("keydown", e => {
  if (!e.metaKey) return;
  const key = e.key.toLowerCase();
  if (key === "k") { e.preventDefault(); return openPalette(); }
  if (key === ",") { e.preventDefault(); return openSettings(); }
  const to = { m: "memories", j: "routines", l: "skills" }[key];
  if (to) { e.preventDefault(); return go("pane", to); }
  if (key === "n" && shown !== "pane") { e.preventDefault(); return go("pane", { chat: { session: null } }); }
});

// ---------- Refresh ----------

let loading = false, again = false, retryTimer = null;

/// Fetches Home. Never two calls at once: a request during one runs once more after it.
async function refresh() {
  if (loading) { again = true; return; }
  clearTimeout(retryTimer);
  loading = true;
  try {
    const [home, mac] = await Promise.all([
      invoke("center_home"),
      invoke("mac_control").catch(e => { console.warn("this Mac's state:", e); return state.mac; }),
    ]);
    state.mac = mac;
    if (home.updates) home.updates = withoutDismissed(home.updates, dismissedUpdates);
    state.home = home;
    state.error = null;
    const live = new Set(state.home.needs_you.map(cardKey));
    for (const key of state.outcomes.keys()) if (!live.has(key)) state.outcomes.delete(key);
  } catch (e) {
    state.error = String(e);
  } finally {
    loading = false;
  }
  draw();
  if (shown === "projects") { drawProjects(views.get("projects").el); drawTop(); }
  // A project's agent refreshes with Home: on focus, on notices and every
  // 10 seconds while the window shows.
  if (shown === "projects" && state.picked) loadProjectAgent();
  reloadGroup("projects");
  reloadGroup("machines");
  if (again) { again = false; refresh(); }
  // Until Home first loads, a failed try is made again soon, seen or not,
  // unless waiting will not fix it.
  else if (!state.home && !problem()) retryTimer = setTimeout(refresh, CONNECT_RETRY_MS);
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
for (const name of ["link-state", "sessions-changed", "work-question", "work-resolved", "group-approval", "approval-answered", "reconnected"]) {
  listen(name, () => refresh());
}
for (const name of ["sessions-changed", "group-changed", "reconnected"]) {
  listen(name, () => reloadChats());
}

$("#home-comp").addEventListener("submit", async e => {
  e.preventDefault();
  const input = $("#home-ask"), note = $("#home-err"), prompt = input.value.trim();
  if (!prompt || !pane()) return;
  note.textContent = "";
  input.value = "";
  go("pane", { chat: { session: null } });
  try {
    await pane().compose(prompt);
  } catch (err) {
    // Back on Home with the prompt as typed, where the error shows.
    input.value = prompt;
    go("home");
    note.textContent = failure(err);
  }
  reloadChats();
});

$("#home-agent")?.addEventListener("click", () => {
  const input = $("#home-ask"), prompt = input.value.trim();
  input.value = "";
  go("pane", { agentnew: { prompt } });
});

// A reload starts with no chat open, as the window just opened does.
invoke("new_chat").catch(() => {}).then(() => takePending("home"));
