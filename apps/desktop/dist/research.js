// A research window: the lead's chat on the left, the team's workspace in
// the middle and its agents on the right. Rust does the networking; this
// polls the research and draws it.
"use strict";

const { invoke, Channel } = window.__TAURI__.core;

const SESSION = new URLSearchParams(location.search).get("session");
/// How often the window asks for news while the team works, and otherwise.
const POLL_BUSY = 2500, POLL_IDLE = 10000;
const RUNNING = new Set(["researching", "reviewing", "writing"]);
const SCOUT_COLORS = ["#5CC49C", "#8FB8E8", "#E8C46D", "#E89A6D", "#9AD0D8"];

const ICON = {
  check: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  x: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  bad: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  ret: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M20 5v7a3 3 0 0 1-3 3H5m4-4l-4 4 4 4"/></svg>',
};

const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const $ = sel => document.querySelector(sel);

const R = {
  research: null, // the last GetResearch, without its entries
  entries: [],
  after: 0,
  turns: [],
  side: { view: "agents", agent: null }, // or "thread" with an agent, or "room"
  tab: null, // null follows the research: notes while it works, the draft once written
  sending: null, // the lead's reply streaming in, as HTML
  error: "",
  note: "",
};

// ---------- Agents ----------

function look(id) {
  if (id === "lead") return { ini: "L", c: "var(--coral)" };
  if (id === "critic") return { ini: "R", c: "#C59AE8" };
  if (id === "writer") return { ini: "W", c: "var(--fg)" };
  const n = +id.replace("scout-", "") || 1;
  return { ini: String(n), c: SCOUT_COLORS[(n - 1) % SCOUT_COLORS.length] };
}
const rav = id => { const l = look(id); return `<span class="rav" style="background:${l.c}">${l.ini}</span>`; };
const agent = id => R.research?.agents.find(a => a.id === id) ?? { id, name: id, role: "", model: "" };
const nameOf = id => agent(id).name;

/// Which members work in each phase; the others have finished or wait.
const PHASE = { researching: ["scout"], reviewing: ["critic", "scout"], writing: ["writer", "critic"] };
const kind = id => id.startsWith("scout-") ? "scout" : id;
const round = () => R.research?.round ?? 1;
/// A scout from an earlier round, done while a later one works.
const earlier = id => kind(id) === "scout" && (agent(id).round ?? 1) < round();
/// When the newest round started: its scouts' first entry, else the research's start.
function roundStart() {
  const r = R.research;
  if (!r || round() === 1) return r?.started_at ?? Date.now();
  const first = R.entries.find(e => kind(e.agent) === "scout" && agent(e.agent).round === round());
  return first?.at ?? r.started_at;
}

/// What an agent is doing: a line and its state (run, ok, wait or none).
function doing(id) {
  const st = R.research?.status ?? "planning";
  const mine = R.entries.filter(e => e.agent === id);
  const notes = mine.filter(e => e.kind === "note").length;
  const sources = R.research?.sources.filter(s => s.agent === id).length ?? 0;
  const said = mine.filter(e => e.kind === "said" && e.to).length;
  const summary = { scout: `Done: ${notes} notes, ${sources} sources`, critic: `Done: ${said} challenges`, writer: "Done: the paper", lead: "Posted the result" }[kind(id)];
  if (id === "lead") {
    if (st === "planning") return ["Waiting for your go", "wait"];
    if (st === "reviewing") return [round() > 1 ? `Coordinating round ${round()}` : "Coordinating", "run"];
    if (RUNNING.has(st)) return ["Watching the room", "run"];
    return st === "done" ? [summary, "ok"] : ["", ""];
  }
  if (st === "done") return [summary, "ok"];
  if (!RUNNING.has(st)) return mine.length ? [summary.replace("Done: ", ""), ""] : ["", ""];
  if (earlier(id)) return [summary, "ok"];
  const order = ["researching", "reviewing", "writing"];
  const active = PHASE[st].includes(kind(id));
  if (active) {
    const step = [...mine].reverse().find(e => e.kind === "step");
    return [step?.label ?? "Starting", "run"];
  }
  const starts = { critic: "reviewing", writer: "writing", scout: "researching" }[kind(id)];
  if (order.indexOf(starts) > order.indexOf(st)) return [kind(id) === "critic" ? "Starts after the scouts" : "Starts after review", ""];
  return [summary, "ok"];
}

const stateIcon = st => st === "run" ? '<span class="dspin"></span>'
  : st === "ok" ? `<span class="dok">${ICON.check}</span>`
  : st === "wait" ? '<span class="ddot" style="background:var(--coral)"></span>' : "<span></span>";

// ---------- Title bar ----------

function minutes(from, to) {
  const m = Math.max(1, Math.round(((to ?? Date.now()) - from) / 60000));
  return `${m} min`;
}

function drawBar() {
  const r = R.research, st = r?.status;
  $("#name").textContent = r?.topic || "Research";
  document.title = r?.topic || "Research";
  const working = r ? r.agents.filter(a => a.id !== "lead" && doing(a.id)[1] === "run").length : 0;
  const stat = {
    planning: '<span class="ddot" style="background:var(--coral)"></span>Waiting for your go',
    researching: `<span class="dspin"></span>${round() > 1 ? `Round ${round()}` : "Researching"}, ${working} agent${working === 1 ? "" : "s"} working`,
    reviewing: '<span class="dspin"></span>Reviewing the findings',
    writing: '<span class="dspin"></span>Writing the paper',
    done: `<span class="dok">${ICON.check}</span>Done in ${r ? minutes(roundStart(), r.finished_at) : ""}`,
    failed: '<span class="ddot bad"></span>The team failed',
    stopped: '<span class="ddot off"></span>Stopped',
    interrupted: '<span class="ddot off"></span>Interrupted by a server restart',
  }[st] ?? (R.sending !== null ? '<span class="dspin"></span>Planning' : "");
  $("#stat").innerHTML = stat;
  $("#stop").innerHTML = RUNNING.has(st) ? '<button class="dbtn warn" data-act="stop">Stop</button>' : "";
}

// ---------- The lead's chat ----------

function lastReplyIndex() {
  for (let i = R.turns.length - 1; i >= 0; i--) if (R.turns[i].role === "assistant") return i;
  return -1;
}

function drawChat() {
  const chat = $("#chat"), stuck = chat.scrollHeight - chat.scrollTop - chat.clientHeight < 40;
  const st = R.research?.status;
  const last = lastReplyIndex();
  let html = "";
  R.turns.forEach((t, i) => {
    if (t.role === "user") html += `<div class="ask">${esc(t.text)}</div>`;
    else if (t.role === "scheduled") html += `<div class="dmuted">${esc(t.label || "Research finished")}</div>`;
    else if (t.role === "assistant" && t.html) {
      html += `<div class="rwho">${rav("lead")}Beep, lead</div><div class="reply">${t.html}</div>`;
      if (i === last && st === "planning" && R.sending === null) {
        html += `<div class="racts"><button class="dbtn pri" data-act="go">Start<kbd>⌘${ICON.ret}</kbd></button></div>`;
      }
      if (i === last && st === "done" && R.sending === null) {
        html += `<div class="racts"><button class="dbtn" data-act="tab:draft">Open the paper</button><button class="dbtn" data-act="export">Export PDF</button></div>`;
      }
    }
  });
  if (R.sending !== null) html += `<div class="rwho">${rav("lead")}Beep, lead</div><div class="reply">${R.sending}<span class="caret"></span></div>`;
  if (RUNNING.has(st)) html += `<div class="rstatus"><span class="dspin"></span>${esc(progress())}</div>`;
  if (R.note) html += `<div class="dmuted">${esc(R.note)}</div>`;
  if (R.error) html += `<div class="derr">${esc(R.error)}</div>`;
  chat.innerHTML = html;
  if (stuck) chat.scrollTop = chat.scrollHeight;
  $("#ask").placeholder = st === "planning" ? "Say go, or what to change" : st === "done" ? "Ask a follow-up" : "Message the lead";
}

function progress() {
  const r = R.research, st = r.status;
  const scouts = r.agents.filter(a => kind(a.id) === "scout" && !earlier(a.id));
  const took = minutes(roundStart());
  if (st === "researching") {
    const busy = scouts.filter(a => doing(a.id)[1] === "run").length;
    const n = `${busy} of ${scouts.length} scout${scouts.length === 1 ? "" : "s"} working`;
    return round() > 1 ? `Round ${round()}: ${n}, ${took}` : `Researching: ${n}, ${took}`;
  }
  if (st === "reviewing") return `The critic and the lead are reviewing, ${took}`;
  return round() > 1 ? `The writer is revising the paper, ${took}` : `The writer is drafting, ${took}`;
}

async function loadChat() {
  const turns = await invoke("research_history", { session: SESSION }).catch(() => null);
  if (!turns || R.sending !== null) return;
  if (JSON.stringify(turns) === JSON.stringify(R.turns)) return;
  R.turns = turns;
  drawChat();
}

/// Sends `text` to the lead; `start` makes the chat a research first.
async function send(text, start = false) {
  text = text.trim();
  if (!text || R.sending !== null) return;
  R.error = "";
  R.note = "";
  R.turns.push({ role: "user", text });
  R.sending = "";
  drawChat();
  drawBar();
  const channel = new Channel();
  channel.onmessage = ev => {
    if (ev.kind === "text") { R.sending = ev.html; drawChat(); }
    else if (ev.kind === "error") R.error = ev.message;
  };
  try {
    await invoke("research_send", { session: SESSION, message: text, start, onEvent: channel });
  } catch (err) {
    R.error = String(err);
  }
  R.sending = null;
  await loadChat();
  drawChat();
  poll(true);
}

// ---------- Workspace ----------

/// Turns "[3]" and "[1, 4]" in rendered text into citation marks.
function cite(html) {
  const t = document.createElement("template");
  t.innerHTML = html;
  const walk = document.createTreeWalker(t.content, NodeFilter.SHOW_TEXT);
  const texts = [];
  while (walk.nextNode()) if (!walk.currentNode.parentElement?.closest("a, code, pre")) texts.push(walk.currentNode);
  for (const node of texts) {
    if (!/\[\d+(?:\s*,\s*\d+)*\]/.test(node.data)) continue;
    const span = document.createElement("span");
    span.innerHTML = esc(node.data).replace(/\[(\d+(?:\s*,\s*\d+)*)\]/g, "<sup>$1</sup>");
    node.replaceWith(...span.childNodes);
  }
  return t.innerHTML;
}

function tab() {
  const r = R.research;
  if (R.tab) return R.tab;
  return r?.draft ? "draft" : "notes";
}

function drawWork() {
  const r = R.research, st = r?.status ?? "planning", t = tab();
  const notes = R.entries.filter(e => e.kind === "note");
  const b = (id, label) => `<button role="tab" aria-selected="${t === id}" data-act="tab:${id}">${label}</button>`;
  const sources = r?.sources.length ? ` ${r.sources.length}` : "";
  $("#tabs").innerHTML = b("draft", "Draft") + b("notes", "Notes") + b("sources", `Sources${sources}`) + '<span class="sp"></span>'
    + (r?.draft ? '<button class="dbtn quiet" data-act="copy">Copy</button><button class="dbtn quiet" data-act="export">Export PDF</button>' : "");
  let body;
  if (!r || (st === "planning" && !notes.length)) body = '<div class="dmuted rempty">The workspace fills in once the team starts.</div>';
  else if (t === "draft" && !r.draft) body = `<div class="dmuted rempty">${RUNNING.has(st) ? "The writer starts after the critic's review." : "No paper was written."}</div>`;
  else if (t === "draft") {
    const by = `Written by ${esc(nameOf("writer"))}, reviewed by ${esc(nameOf("critic"))}. ${r.sources.length} sources.`;
    body = `<div class="rdoc"><span class="by">${by}</span>${cite(r.draft_html)}</div>`;
  } else if (t === "notes") {
    body = notes.length ? `<div class="rdoc">${notes.map(n => {
      const src = n.sources.length ? ` <sup>${n.sources.join(", ")}</sup>` : "";
      return `<div class="rnote" style="--c:${look(n.agent).c}"><b>${esc(nameOf(n.agent))}</b><span>${esc(n.content)}${src}</span></div>`;
    }).join("")}</div>` : '<div class="dmuted rempty">No notes yet.</div>';
  } else {
    body = r.sources.length ? r.sources.map(s => `<div class="rsrc"><span class="n">${s.number}</span><a href="${esc(s.url)}">${esc(s.title)}</a>
      <small>${esc(host(s.url))}, kept by ${esc(nameOf(s.agent))}</small><q>${esc(s.quote)}</q></div>`).join("") : '<div class="dmuted rempty">No sources yet.</div>';
  }
  const work = $("#work"), top = work.scrollTop;
  work.innerHTML = body;
  work.scrollTop = top;
}

function host(url) {
  try { return new URL(url).host.replace(/^www\./, ""); } catch { return url; }
}

// ---------- Agents panel ----------

function msg(e) {
  const to = e.to ? ` <span class="to">to ${esc(nameOf(e.to))}</span>` : "";
  return `<div class="rmsg">${rav(e.agent)}<b>${esc(nameOf(e.agent))}${to}</b><p>${esc(e.content)}</p></div>`;
}

function drawSide() {
  const side = $("#side"), r = R.research, view = R.side.view;
  $("#grid").classList.toggle("wide", view !== "agents");
  side.classList.toggle("dim", view === "agents");
  if (view === "thread") {
    const a = agent(R.side.agent);
    const mine = R.entries.filter(e => e.agent === a.id || (e.kind === "said" && e.to === a.id));
    let html = "", steps = [];
    const flush = () => { if (steps.length) html += `<div class="dsteps">${steps.join("")}</div>`; steps = []; };
    for (const e of mine) {
      if (e.kind === "step" && e.agent === a.id) {
        steps.push(`<div class="dstep"><span class="s">${e.success ? `<span class="dok">${ICON.check}</span>` : `<span class="dbad">${ICON.bad}</span>`}</span><span>${esc(e.label)}</span></div>`);
      } else if (e.kind === "said") { flush(); html += msg(e); }
      else if (e.kind === "note") { flush(); html += `<div class="rnote" style="--c:${look(a.id).c}"><b>Note</b><span>${esc(e.content)}${e.sources.length ? ` <sup>${e.sources.join(", ")}</sup>` : ""}</span></div>`; }
    }
    flush();
    const [now, st] = doing(a.id);
    if (st === "run") html += `<div class="dsteps"><div class="dstep run"><span class="s"><span class="dspin"></span></span><span>${esc(now)}</span></div></div>`;
    side.innerHTML = `<div class="rthead">${rav(a.id)}<span class="who"><b>${esc(a.name)}</b><small>${esc(a.role)} · ${esc(a.model)}</small></span><button data-act="back" aria-label="Back to agents">${ICON.x}</button></div>
      <div class="rpane thread">${html || '<div class="dmuted">Nothing yet.</div>'}</div>`;
  } else if (view === "room") {
    const said = R.entries.filter(e => e.kind === "said");
    side.innerHTML = `<div class="rthead"><b>Room</b><small>where the agents talk</small><button data-act="back" aria-label="Back to agents">${ICON.x}</button></div>
      <div class="rpane thread">${said.map(msg).join("") || '<div class="dmuted">Nobody has said anything yet.</div>'}</div>`;
  } else {
    const row = a => {
      const [now, st] = doing(a.id);
      return `<button class="rag" data-act="agent:${esc(a.id)}">${rav(a.id)}<b>${esc(a.name)}</b>${stateIcon(st)}<small>${esc(now || a.role)}</small><span class="model">${esc(a.model)}</span></button>`;
    };
    // Each later round's scouts sit under a Round line; the critic and writer, who work in every round, come last.
    const agents = r?.agents ?? [{ id: "lead", name: "Lead", role: "Plans and coordinates", model: "", round: 0 }];
    const later = agents.filter(a => (a.round ?? 0) > 1);
    let rows = agents.filter(a => (a.round ?? 0) <= 1 && a.id !== "critic" && a.id !== "writer").map(row).join("");
    for (let n = 2; n <= round(); n++) {
      const of = later.filter(a => a.round === n);
      if (of.length) rows += `<div class="rsec">Round ${n}</div>${of.map(row).join("")}`;
    }
    if (later.length) rows += '<div class="rhr"></div>';
    rows += agents.filter(a => a.id === "critic" || a.id === "writer").map(row).join("");
    const said = R.entries.filter(e => e.kind === "said").length;
    const room = r && r.status !== "planning" ? `${said} message${said === 1 ? "" : "s"}` : "Opens when the team starts";
    side.innerHTML = `<div class="rpane"><div class="rsec">Agents</div>${rows}<div class="rhr"></div>
      <button class="rag" data-act="room"><span class="rav" style="background:var(--elev);color:var(--fg)">#</span><b>Room</b><span></span><small>${room}</small></button></div>`;
  }
  const pane = side.querySelector(".thread");
  if (pane) pane.scrollTop = pane.scrollHeight;
}

function draw() {
  drawBar();
  drawChat();
  drawWork();
  drawSide();
}

// ---------- Polling ----------

let timer = null;
async function poll(now = false) {
  clearTimeout(timer);
  if (!now) await loadChat();
  try {
    const r = await invoke("get_research", { session: SESSION, after: R.after });
    if (r) {
      const was = R.research?.status;
      R.entries.push(...r.entries);
      if (r.entries.length) R.after = r.entries.at(-1).seq;
      delete r.entries;
      R.research = r;
      if (was && was !== r.status) await loadChat();
      draw();
    }
  } catch (err) {
    console.warn("research poll", err);
  }
  const busy = !R.research || R.research.status === "planning" || RUNNING.has(R.research.status);
  timer = setTimeout(poll, busy ? POLL_BUSY : POLL_IDLE);
}

// ---------- Input ----------

async function act(a, v) {
  if (a === "go") return send("Go");
  if (a === "stop") {
    await invoke("stop_research", { session: SESSION }).catch(err => { R.error = String(err); });
    return poll(true);
  }
  if (a === "tab") { R.tab = v; return drawWork(); }
  if (a === "agent") { if (R.research?.status === "planning" && v !== "lead") return; R.side = { view: "thread", agent: v }; return drawSide(); }
  if (a === "room") { if (!R.research || R.research.status === "planning") return; R.side = { view: "room", agent: null }; return drawSide(); }
  if (a === "back") { R.side = { view: "agents", agent: null }; return drawSide(); }
  if (a === "copy") return copy(R.research?.draft ?? "");
  if (a === "export") {
    R.note = "Typesetting the PDF";
    drawChat();
    try {
      const path = await invoke("export_research", { session: SESSION });
      R.note = `Saved ${path.split("/").pop()} to Downloads`;
    } catch (err) {
      R.note = "";
      R.error = String(err);
    }
    return drawChat();
  }
}

async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const t = document.createElement("textarea");
    t.value = text;
    document.body.append(t);
    t.select();
    document.execCommand("copy");
    t.remove();
  }
  R.note = "Copied the paper as Markdown";
  drawChat();
}

document.addEventListener("click", e => {
  const b = e.target.closest("[data-act]");
  if (b) {
    const [a, v] = b.dataset.act.split(/:(.*)/);
    return void act(a, v);
  }
  const link = e.target.closest("a[href]");
  if (!link) return;
  e.preventDefault();
  invoke("open_url", { url: link.href }).catch(() => {});
});

const ask = $("#ask");
ask.addEventListener("input", () => {
  ask.style.height = "auto";
  ask.style.height = Math.min(ask.scrollHeight, 140) + "px";
});
document.addEventListener("keydown", e => {
  if (e.metaKey && e.key === "Enter" && R.research?.status === "planning" && R.sending === null) {
    e.preventDefault();
    return void send("Go");
  }
  if (e.metaKey && e.key.toLowerCase() === "w") { e.preventDefault(); return window.close(); }
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.target === ask) {
    e.preventDefault();
    const text = ask.value;
    ask.value = "";
    ask.style.height = "auto";
    return void send(text);
  }
});

(async () => {
  draw();
  await loadChat();
  const topic = await invoke("research_pending", { session: SESSION }).catch(() => null);
  if (topic && !R.turns.length) send(topic, true);
  poll(true);
  ask.focus();
})();
