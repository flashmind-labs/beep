// The command center window: a sidebar, Home, and the bar's page as a pane
// (app.js, loaded before this module) for chats, memories and routines.
const { invoke, Channel } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

import { splitPinned } from "./pinned.js";
import { initPalette, openPalette } from "./palette.js";

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

const state = { home: null, error: null, outcomes: new Map(), shown: false };

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
  return h("div", "cc-row",
    avatar(ag.cls, ag.ini),
    h("span", "t", h("b", "", r.title || project || ag.name), h("span", "", `${project ? project + ", " : ""}${ag.name} on ${r.machine_name}`)),
    h("span", "r", status, ago(r.started_at)));
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

function machinesSection(machines) {
  const online = machines.filter(m => m.online).length;
  if (!machines.length) {
    const link = h("ol", "", h("li", "", "Install beep-node on the computer."), h("li", "", h("span", "", "Run "), h("code", "", "beep-node link"), h("span", "", " there.")), h("li", "", "Paste the link it prints into a chat here, or into Beep on your phone."));
    return section("sec-machines", "Machines", "None linked", empty(
      h("b", "", "No machines linked yet"),
      h("span", "", "Link a computer to run coding agents and commands on it from here, from the bar or from your phone."),
      link));
  }
  return section("sec-machines", "Machines", `${online} of ${machines.length} online`, h("div", "cc-machs", ...machines.map(machineCard)));
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
    if (g.items == null) continue;
    const grp = h("div", `cc-grp ${name}`);
    if (g.title) {
      const h3 = h("h3", "", g.title);
      if (g.action) h3.append(g.action());
      grp.append(h3);
    }
    const none = typeof g.empty === "function" ? g.empty() : g.empty;
    if (none && !g.items.length) grp.append(h("span", "cc-si none", none));
    for (const item of g.items) {
      const el = g.row(item);
      if (g.onPick) el.addEventListener("click", () => g.onPick(item));
      grp.append(el);
    }
    if (g.foot) grp.append(...g.foot());
    if (grp.childElementCount > (g.title ? 1 : 0)) side.push(grp);
  }
  side.push(h("div", "cc-foot", ...footer(home?.machines).map(f => h("span", "", f))));
  $("#side").replaceChildren(...side);
  // A chat's title comes from the list, which may just have loaded.
  if (shown === "pane") drawTop();
}

function footer(machines) {
  if (!state.home) return [];
  if (!machines) return ["Machines unavailable"];
  if (!machines.length) return ["No machines linked"];
  const on = machines.filter(m => m.online).length;
  return [h("span", "", h("span", "ddot"), " ", h("b", "", String(on)), ` of ${plural(machines.length, "machine", "machines")} online`)];
}

/// Pinned chats and groups, then the newest of the rest, the open one
/// highlighted.
const SIDE_CHATS = 12;
let chatList = [];
let chatFetch = null;

/// One fetch feeds both groups, even when both ask at once.
function fetchChats() {
  chatFetch ??= pane().chatRows().then(rows => (chatList = rows)).finally(() => { chatFetch = null; });
  return chatFetch;
}

function reloadChats() {
  reloadGroup("pinned");
  reloadGroup("chats");
}

/// Pinned projects join the Pinned group here once projects can be pinned:
/// return items shaped like chat rows ({ kind: "project", id, title }) and
/// give `chatRow` and the group's `onPick` a branch for them.
async function pinnedProjects() { return []; }

async function setPinned(c, pinned) {
  try {
    await invoke(c.kind === "group" ? "pin_group" : "pin_session", { id: c.id, pinned });
  } catch (e) {
    console.warn("pinning a chat:", e);
  }
  reloadChats();
}

function chatRow(c) {
  const on = paneOn();
  const open = c.kind === "group" ? on.group === c.id : on.view === "chat" && on.session === c.id;
  const mark = c.kind === "group" ? icon(IC.group) : icon(c.research ? IC.doc : IC.chat);
  const toggle = h("span", "pa", icon(IC.pin));
  toggle.setAttribute("role", "button");
  toggle.setAttribute("aria-label", c.pinned ? "Unpin" : "Pin to top");
  toggle.addEventListener("click", e => { e.stopPropagation(); setPinned(c, !c.pinned); });
  const b = h("button", "cc-si", mark, h("span", "tt", c.title), toggle);
  b.type = "button";
  if (open) b.setAttribute("aria-current", "true");
  return b;
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
  row: v => sideItem(v.svg, v.label, 0, { current: paneOn().view === v.view }),
  onPick: v => go("pane", v.view),
});

function projects(running) {
  const found = new Map();
  for (const r of running) if (r.kind === "agent" && base(r.cwd)) found.set(base(r.cwd), (found.get(base(r.cwd)) || 0) + 1);
  return [...found];
}

// Projects found in what runs, until the project view replaces this group.
registerSidebarGroup("projects", {
  order: 5,
  title: "Projects",
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

function draw() {
  const home = state.home, view = $("#view");
  drawSide();
  if (!home) {
    if (state.error) view.replaceChildren(h("div", "cc-note", h("span", "ddot bad"), h("span", "", "Can't reach Beep right now. Retrying.")));
    return;
  }
  const top = view.scrollTop;
  view.replaceChildren(...page(home));
  view.scrollTop = top;
  applyScroll();
  scrollTarget = null;
  $("#home-ask").placeholder = home.machines?.length ? "Ask Beep, or start an agent on a machine" : "Ask Beep anything";
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

// ---------- Keys ----------

initPalette({ go, jump, pane, findIcon: IC.find });

document.addEventListener("keydown", e => {
  if (!e.metaKey) return;
  const key = e.key.toLowerCase();
  if (key === "k") { e.preventDefault(); return openPalette(); }
  const to = { m: "memories", j: "routines", l: "skills", ",": "settings" }[key];
  if (to) { e.preventDefault(); return go("pane", to); }
  if (key === "n" && shown !== "pane") { e.preventDefault(); return go("pane", { chat: { session: null } }); }
});

// ---------- Refresh ----------

let loading = false, again = false;

/// Fetches Home. Never two calls at once: a request during one runs once more after it.
async function refresh() {
  if (loading) { again = true; return; }
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
  if (again) { again = false; refresh(); }
}

window.addEventListener("focus", refresh);
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") refresh(); });
setInterval(() => { if (document.visibilityState === "visible") refresh(); }, 10000);
for (const name of ["sessions-changed", "work-question", "work-resolved", "group-approval", "approval-answered", "reconnected"]) {
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

takePending("home");
