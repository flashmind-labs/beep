// The command center window: a sidebar, Home, and the bar's page as a pane
// (app.js, loaded before this module) for chats, memories and routines.
const { invoke, Channel } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

import { splitPinned } from "./pinned.js";
import { initPalette, openPalette } from "./palette.js";
import { initSideResize } from "./sidewidth.js";
import { connectProblem } from "./connect.js";

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
  pin: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  group: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0113 0"/><circle cx="17" cy="9" r="2.5"/><path d="M16 14a5 5 0 016 5"/></svg>',
  doc: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>',
  rout: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>',
};

const CAPS = { gpu: "GPU", cuda: "CUDA", blender: "Blender", unreal: "Unreal", xcode: "Xcode", android: "Android", docker: "Docker" };
const AGENTS = { claude: { cls: "claude", ini: "CC", name: "Claude Code" }, codex: { cls: "codex", ini: "CX", name: "Codex" } };
const PROJECT_COLORS = ["#F07A6A", "#6FA8DC", "#B18CE0", "#E9B949", "#3B9B7A"];
const OFFLINE_NOTE_MS = 60 * 60 * 1000;
/// Before Home first loads: how long "Connecting" shows before "Still
/// connecting", and how soon a failed try is made again.
const CONNECT_STUCK_MS = 15000;
const CONNECT_RETRY_MS = 3000;

/// `since` is when the window started waiting for its first Home.
const state = { home: null, error: null, outcomes: new Map(), shown: false, since: Date.now() };

// ---------- Formatting ----------

function plural(n, one, many) { return `${n} ${n === 1 ? one : many}`; }
function ago(ms) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.round(s / 60)} min`;
  if (s < 86400) return `${Math.round(s / 3600)} h`;
  return `${Math.round(s / 86400)} d`;
}
function base(path) { return String(path || "").split("/").filter(Boolean).pop() || ""; }
function agentOf(name) {
  return AGENTS[name] || { cls: "", ini: String(name || "?").slice(0, 2).toUpperCase(), name: name || "Agent" };
}
function gb(mb) { return `${Math.round(mb / 1024)} GB`; }
function whenOf(iso) {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return { time: "", day: "" };
  const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  const days = Math.round((new Date(at).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 86400000);
  const day = days <= 0 ? "today" : days === 1 ? "tomorrow" : at.toLocaleDateString([], { weekday: "short" });
  return { time, day };
}
function scheduleText(s) {
  if (s.startsWith("arrive:")) return "on arrival";
  if (/^\d{4}-\d\d-\d\dT/.test(s)) return "once";
  return s;
}

// ---------- Pieces ----------

function dot(m) { return h("span", `ddot${!m.online ? " off" : m.agents ? " busy" : ""}`); }
function section(id, title, note, ...body) {
  const el = h("div");
  el.id = id;
  el.append(h("div", "cc-sec", h("h4", "", title), h("small", "", note)), ...body);
  return el;
}
function empty(...kids) { return h("div", "cc-panel cc-empty", ...kids); }
function avatar(cls, ini) { return h("span", `cc-ag ${cls}`, ini); }
function tag(cls, ...kids) { return h("span", `cc-tag${cls ? " " + cls : ""}`, ...kids); }

function machineCard(m) {
  const chips = [];
  const gpus = m.gpus.map(g => g.model);
  for (const c of m.capabilities) if (!(c === "gpu" && gpus.length)) chips.push(CAPS[c] || c);
  chips.push(...gpus, ...m.apps.map(a => a.name));
  const unique = [...new Set(chips)];
  const caps = h("div", "cc-caps", ...unique.slice(0, 6).map(c => h("span", "cc-cap", c)));
  if (unique.length > 6) caps.append(h("span", "cc-cap", `+${unique.length - 6}`));
  const env = m.env;
  const os = env ? env.os_version || env.os : "";
  const hw = env ? `${plural(env.cpus, "core", "cores")}, ${gb(env.memory_mb)}` : "";
  const left = !m.online ? `seen ${ago(m.seen_at)} ago` : m.agents ? plural(m.agents, "agent", "agents") : "idle";
  const card = h("div", `cc-mach${m.online ? "" : " off"}`,
    h("div", "h", dot(m), h("b", "", m.name)),
    h("span", "os", os),
    caps,
    h("div", "st", h("span", "", left), h("span", "", m.online ? hw : "offline")));
  card.id = `m-${m.id}`;
  return card;
}

function runningRow(r) {
  if (r.kind === "research") {
    const row = h("button", "cc-row link",
      avatar("beep", icon(IC.find)),
      h("span", "t", h("b", "", r.topic), h("span", "", `Research, round ${r.round}, ${plural(r.sources, "source", "sources")}`)),
      h("span", "r", tag("run", h("span", "dspin"), r.status), ago(r.started_at)));
    row.addEventListener("click", () => invoke("open_research", { session: r.session }).catch(() => {}));
    return row;
  }
  const ag = agentOf(r.agent);
  const project = base(r.cwd);
  let status;
  if (r.status === "asking") status = tag("wait", "needs you");
  else if (r.external && r.status !== "working" && r.status !== "running") status = tag("term", "in terminal");
  else if (r.status === "working" || r.status === "running" || r.status === "starting") status = tag("run", h("span", "dspin"), r.doing || r.status);
  else status = tag("", r.status);
  const row = h("button", "cc-row link",
    avatar(ag.cls, ag.ini),
    h("span", "t", h("b", "", r.title || project || ag.name), h("span", "", `${project ? project + ", " : ""}${ag.name} on ${r.machine_name}`)),
    h("span", "r", status, ago(r.started_at)));
  // The pane's agent view (app.js) follows and answers it.
  row.addEventListener("click", () => go("pane", { agent: r }));
  return row;
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

function answerAgent(n, option) {
  const key = cardKey(n);
  setOutcome(key, { said: `Answered: ${option.name}` });
  invoke("center_answer_agent", { machineId: n.machine_id, agent: n.agent, session: n.session, question: n.question.id, reply: option.id })
    .then(() => refresh())
    .catch(e => setOutcome(key, { error: failure(e), final: conflict(e) }));
}

function button(label, cls, on) {
  const b = h("button", `dbtn${cls ? " " + cls : ""}`, label);
  b.type = "button";
  b.addEventListener("click", on);
  return b;
}

function needsCard(n) {
  const out = state.outcomes.get(cardKey(n));
  let head, body, acts;
  if (n.kind === "chat") {
    const a = n.pending.approval;
    const title = h("b", "", `${a.app}: ${a.action}`);
    title.classList.add("cc-link");
    title.title = "Open this chat";
    title.addEventListener("click", () => go("pane", { chat: { session: n.session_id } }));
    head = [avatar("beep", icon(IC.chat)), h("span", "t", title, h("span", "", `Chat: ${n.session_title}`))];
    body = a.fields.length ? h("div", "kv", ...a.fields.flatMap(f => [h("span", "", f.label), h("span", "", f.value)])) : null;
    acts = [
      button("Allow", "pri", () => answerChat(n, "allow", "Allowed once")),
      button("Always allow", "", () => answerChat(n, "always", "Allowed, and always allowed from now on")),
      button("Deny", "", () => answerChat(n, "deny", "Denied")),
    ];
  } else {
    const ag = agentOf(n.agent), q = n.question;
    head = [avatar(ag.cls, ag.ini), h("span", "t", h("b", "", q.text), h("span", "", `${n.title || ag.name} on ${n.machine_name}`))];
    body = q.detail ? h("div", "kv", h("span", "", "Input"), h("span", "", q.detail)) : null;
    acts = q.options.map((o, i) => button(o.name, i === 0 && o.allow ? "pri" : "", () => answerAgent(n, o)));
  }
  const actions = h("div", "acts");
  if (out?.said) actions.append(h("span", "ok", out.said));
  else if (out?.final) actions.append(h("span", "err", out.error));
  else {
    actions.append(...acts);
    if (out?.error) actions.append(h("span", "err", out.error));
  }
  const card = h("div", "cc-ask", h("div", "h", ...head), body, actions);
  if (out?.said) card.style.opacity = ".55";
  return card;
}

// ---------- Sections ----------

/// The Machines header's note, then its Add a machine button.
function addMachineButton(cls) {
  const b = button("", cls, openAddMachine);
  b.append(icon(IC.plus), " Add a machine");
  return b;
}

function machinesSection(machines) {
  const online = machines.filter(m => m.online).length;
  let el;
  if (!machines.length) {
    const link = h("ol", "", h("li", "", "Install beep-node on the computer."), h("li", "", h("span", "", "Run "), h("code", "", "beep-node link"), h("span", "", " there.")), h("li", "", "Paste the link it prints into a chat here, or into Beep on your phone."));
    el = section("sec-machines", "Machines", "None linked", empty(
      h("b", "", "No machines linked yet"),
      h("span", "", "Link a computer to run coding agents and commands on it from here, from the bar or from your phone."),
      link,
      addMachineButton("pri")));
  } else {
    el = section("sec-machines", "Machines", `${online} of ${machines.length} online`, h("div", "cc-machs", ...machines.map(machineCard)));
  }
  const head = el.querySelector(".cc-sec"), note = head.querySelector("small");
  head.append(h("span", "cc-sec-r", note, addMachineButton("small")));
  return el;
}

function offlineNote(machines) {
  const gone = machines.filter(m => !m.online && Date.now() - m.seen_at < OFFLINE_NOTE_MS);
  if (!gone.length) return null;
  const names = gone.map(m => m.name).join(", ");
  return h("div", "cc-note", h("span", "ddot bad"), h("span", "", h("b", "", `${names} ${gone.length === 1 ? "is" : "are"} offline.`), " Their sessions left Running now. They come back when beep-node runs there again."));
}

function runningSection(running) {
  const machines = new Set(running.filter(r => r.kind === "agent").map(r => r.machine_id)).size;
  const note = running.length ? `${plural(running.length, "session", "sessions")}${machines ? " on " + plural(machines, "machine", "machines") : ""}` : "Nothing running";
  const body = running.length ? h("div", "cc-panel", ...running.map(runningRow))
    : h("div", "cc-panel cc-empty", "Sessions you start here, in the bar or in a terminal show up once a machine is linked.");
  return section("sec-running", "Running now", note, body);
}

function needsSection(needs) {
  const body = needs.length ? h("div", "cc-panel", ...needs.map(needsCard)) : h("div", "cc-panel cc-empty", "Nothing needs you right now.");
  return section("sec-needs", "Needs you", "Also on your phone", body);
}

function comingSection(rows) {
  const body = rows.length ? h("div", "cc-panel", ...rows.map(c => {
    const w = whenOf(c.next_run_at);
    return h("div", "cc-row",
      h("span", "cc-when", h("b", "", w.time), w.day),
      h("span", "t", h("b", "", c.label), h("span", "", new Date(c.next_run_at).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" }))),
      h("span", "r", tag("", scheduleText(c.schedule))));
  })) : h("div", "cc-panel cc-empty", "No routines are scheduled.");
  return section("sec-coming", "Coming up", "Runs Beep starts on its own", body);
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
/// group; `row(item)` draws one; `onPick(item)` runs on a click. Groups
/// sort by `order`.
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
      const h3 = h("h3", "", g.title);
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
  foot() {
    if (!chatList.length) return [];
    return [sideItem(null, "All chats", 0, { cls: "more", current: paneOn().view === "chats", on: () => go("pane", "chats") })];
  },
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

function projects(running) {
  const found = new Map();
  for (const r of running) if (r.kind === "agent" && base(r.cwd)) found.set(base(r.cwd), (found.get(base(r.cwd)) || 0) + 1);
  return [...found];
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

/// An empty group's row: what is missing, and a way to add one.
function emptyRow(text, on) {
  const b = h("button", "cc-si none add", `${text} · `, h("span", "act", "Add one"));
  b.type = "button";
  b.addEventListener("click", on);
  return b;
}

/// Adds a machine. Another branch puts the add-a-machine sheet here; until
/// then it opens the Machines part of Settings.
export function openAddMachine() {
  openSettings("machines");
}

/// Opens the projects view. Until there is one, Home's Running section,
/// where the projects here come from.
export function openProjects() {
  jump("sec-running");
}

// Projects found in what runs, until the project view replaces this group.
// It shows once Home has loaded, empty or not.
registerSidebarGroup("projects", {
  order: 5,
  title: "Projects",
  action: () => plusButton("Add a project", openProjects),
  empty: () => emptyRow("No projects yet", openProjects),
  load: async () => state.home?.running ? projects(state.home.running).map(([name, n], i) => ({ name, n, i })) : null,
  row(p) {
    const b = sideItem(null, p.name, p.n);
    const sw = h("span", "cc-pdot");
    sw.style.background = PROJECT_COLORS[p.i % PROJECT_COLORS.length];
    b.prepend(sw);
    return b;
  },
  onPick: () => jump("sec-running"),
});

// The machines Home lists, each with its online dot. It shows once Home has
// loaded, empty or not, unless the server is too old to list machines.
registerSidebarGroup("machines", {
  order: 5.5,
  title: "Machines",
  action: () => plusButton("Add a machine", openAddMachine),
  empty: () => emptyRow("No machines yet", openAddMachine),
  load: async () => state.home?.machines ?? null,
  row(m) {
    const b = sideItem(null, m.name, m.agents || 0);
    b.prepend(dot(m));
    b.title = `${m.name}: ${m.online ? (m.agents ? plural(m.agents, "agent", "agents") + " running" : "online") : "offline"}`;
    return b;
  },
  onPick: m => jump(`m-${m.id}`),
});

// ---------- Home ----------

function page(home) {
  const { machines, running, coming_up: coming, needs_you: needs } = home;
  const out = [];
  if (state.error) out.push(h("div", "cc-note", h("span", "ddot bad"), h("span", "cc-err", `Could not refresh: ${state.error}`)));
  if (!machines && !running) {
    out.push(h("div", "cc-note", h("span", "ddot off"), h("span", "", "Machines and running sessions aren't available for this account or server.")));
  } else {
    const note = machines && offlineNote(machines);
    if (note) out.push(note);
    if (machines) out.push(machinesSection(machines));
  }
  const cols = h("div", "cc-cols");
  if (running) {
    cols.append(runningSection(running), needsSection(needs));
    out.push(cols);
    if (coming) out.push(comingSection(coming));
  } else {
    cols.append(needsSection(needs));
    if (coming) cols.append(comingSection(coming)); else cols.style.gridTemplateColumns = "minmax(0, 1fr)";
    out.push(cols);
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
  view.replaceChildren(...page(home));
  view.scrollTop = top;
  applyScroll();
  scrollTarget = null;
  $("#home-ask").placeholder = home.machines?.length ? "Ask Beep, or start an agent on a machine" : "Ask Beep anything";
  $("#home-agent").style.display = home.machines?.some(m => m.online) ? "" : "none";
}

registerView("home", {
  el: $("#home-box"),
  title: () => ({ title: "Home", sub: new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" }) }),
  mount() { refresh(); },
});

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
    state.home = await invoke("center_home");
    state.error = null;
    const live = new Set(state.home.needs_you.map(cardKey));
    for (const key of state.outcomes.keys()) if (!live.has(key)) state.outcomes.delete(key);
  } catch (e) {
    state.error = String(e);
  } finally {
    loading = false;
  }
  draw();
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
