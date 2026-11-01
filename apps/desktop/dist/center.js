// The command center window: Home. It asks the app for one HomeView and draws it.
const { invoke, Channel } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

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
      avatar("beep", "b"),
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
    title.addEventListener("click", () => showBar({ chat: { session: n.session_id } }));
    head = [avatar("beep", "b"), h("span", "t", title, h("span", "", `Chat: ${n.session_title}`))];
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

// ---------- Sidebar and page ----------

/// Opens one of the bar's own views.
function showBar(view) {
  invoke("center_show_bar", { view }).catch(e => console.warn("showing the bar:", e));
}

function jump(id) {
  const view = $("#view");
  if (!id) view.scrollTo({ top: 0 });
  else document.getElementById(id)?.scrollIntoView({ block: "start" });
}

function sideItem(svg, label, count, opts = {}) {
  const b = h("button", "cc-si", svg ? icon(svg) : null, label);
  b.type = "button";
  if (opts.current) b.setAttribute("aria-current", "true");
  if (count) b.append(h("span", `n${opts.hot ? " hot" : ""}`, String(count)));
  if (opts.bar) b.addEventListener("click", () => showBar(opts.bar));
  else b.addEventListener("click", () => jump(opts.target));
  return b;
}

function projects(running) {
  const found = new Map();
  for (const r of running) if (r.kind === "agent" && base(r.cwd)) found.set(base(r.cwd), (found.get(base(r.cwd)) || 0) + 1);
  return [...found];
}

function sidebar(home) {
  const { machines, running, coming_up: coming, needs_you: needs } = home;
  const side = [h("div", "cc-head", ""), h("div", "cc-brand", h("span", "dlogo", "b"), h("b", "", "Beep"))];
  side[0].setAttribute("data-tauri-drag-region", "");
  const pilot = h("div", "cc-grp", h("h3", "", "Pilot"), sideItem(IC.home, "Home", needs.length, { current: true, hot: true }));
  if (running) pilot.append(sideItem(IC.run, "Running", running.length, { target: "sec-running" }));
  if (coming) pilot.append(sideItem(IC.cal, "Coming up", coming.length, { target: "sec-coming" }));
  side.push(pilot);
  side.push(h("div", "cc-grp", h("h3", "", "Library"),
    sideItem(IC.chat, "Chats", 0, { bar: "chats" }),
    sideItem(IC.mem, "Memories", 0, { bar: "memories" }),
    sideItem(IC.rout, "Routines", 0, { bar: "routines" })));
  const projs = running ? projects(running) : [];
  if (projs.length) {
    side.push(h("div", "cc-grp", h("h3", "", "Projects"), ...projs.map(([name, n], i) => {
      const b = sideItem(null, name, n, { target: "sec-running" });
      const sw = h("span", "cc-pdot");
      sw.style.background = PROJECT_COLORS[i % PROJECT_COLORS.length];
      b.prepend(sw);
      return b;
    })));
  }
  if (machines?.length) {
    side.push(h("div", "cc-grp machines", h("h3", "", "Machines"), ...machines.map(m => {
      const b = sideItem(null, m.name, m.agents, { target: `m-${m.id}` });
      b.prepend(dot(m));
      return b;
    })));
  }
  let foot;
  if (!machines) foot = [h("b", "", "Machines unavailable"), "Not available for this account or server."];
  else if (!machines.length) foot = [h("b", "", "No machines"), "Link one to run agents on it."];
  else {
    const on = machines.filter(m => m.online).length;
    foot = [h("span", "", h("span", "ddot"), " ", h("b", "", plural(machines.length, "machine", "machines")), " linked"), `${on} online, ${machines.length - on} offline or asleep`];
  }
  side.push(h("div", "cc-foot", ...foot.map(f => h("span", "", f))));
  return side;
}

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

/// Before the first load succeeds: the sidebar frame and what went wrong.
function drawUnloaded() {
  const head = h("div", "cc-head", "");
  head.setAttribute("data-tauri-drag-region", "");
  const home = h("button", "cc-si", icon(IC.home), "Home");
  home.type = "button";
  home.setAttribute("aria-current", "true");
  $("#side").replaceChildren(head, h("div", "cc-brand", h("span", "dlogo", "b"), h("b", "", "Beep")), h("div", "cc-grp", h("h3", "", "Pilot"), home));
  $("#view").replaceChildren(h("div", "cc-note", h("span", "ddot bad"), h("span", "", "Can't reach Beep right now. Retrying.")));
}

function draw() {
  const home = state.home;
  if (!home) { if (state.error) drawUnloaded(); return; }
  const view = $("#view"), top = view.scrollTop;
  $("#side").replaceChildren(...sidebar(home));
  view.replaceChildren(...page(home));
  view.scrollTop = top;
  $("#ask").placeholder = home.machines?.length ? "Ask Beep, or start an agent on a machine" : "Ask Beep anything";
  $("#date").textContent = new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });
}

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
  if (again) { again = false; refresh(); }
}

window.addEventListener("focus", refresh);
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") refresh(); });
setInterval(() => { if (document.visibilityState === "visible") refresh(); }, 10000);
for (const name of ["sessions-changed", "work-question", "work-resolved", "group-approval", "reconnected"]) {
  listen(name, () => refresh());
}

$("#comp").addEventListener("submit", async e => {
  e.preventDefault();
  const input = $("#ask"), note = $("#comp-err"), prompt = input.value.trim();
  if (!prompt) return;
  note.textContent = "";
  try {
    await invoke("center_compose", { prompt });
    input.value = "";
  } catch (err) {
    note.textContent = failure(err);
  }
});

refresh();
