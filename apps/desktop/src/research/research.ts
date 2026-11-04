// research/: the research window, and research in the bar and the command
// center. Entry: this file (dist/research.js).
// A research window: the lead's chat on the left, the team's workspace in the
// middle and its agents on the right.
import { followSkin } from "../ui/skin.ts";
followSkin();
import { invoke, Channel } from "../ui/tauri.ts";
import { esc } from "../ui/format.ts";
import type { ResearchStatus as Status } from "./research-list.ts";

/// A member of the team (Rust's `AgentView`).
interface Agent {
  id: string;
  name: string;
  role: string;
  model: string;
  /// A scout's round; 0 for agents in every round. Rust always sends it;
  /// only the stand-in for an unknown id lacks it.
  round?: number;
}

/// A source the scouts kept (Rust's `SourceView`).
interface Source {
  number: number;
  url: string;
  title: string;
  quote: string;
  agent: string;
}

type Said = { kind: "said"; to: string | null; content: string };
type StepEntry = { kind: "step"; label: string; success: boolean };
type Note = { kind: "note"; content: string; sources: number[] };
/// What an agent did (Rust's `EntryView`).
type Entry = { seq: number; agent: string; at: number } & (Said | StepEntry | Note);

/// A research as Rust sends it (`ResearchView`).
interface ResearchView {
  topic: string;
  status: Status;
  agents: Agent[];
  entries: Entry[];
  sources: Source[];
  /// The paper as Markdown, for copying and export.
  draft: string;
  /// The paper as sanitized HTML.
  draft_html: string;
  started_at: number;
  finished_at: number | null;
  /// The newest round, from 1.
  round: number;
}
/// A research as kept here: its entries are in `R.entries`.
type Research = Omit<ResearchView, "entries">;

/// A message of the lead's chat (Rust's `Turn`), as far as this reads it.
type Turn =
  | { role: "user"; text: string }
  | { role: "assistant"; html: string }
  | { role: "scheduled"; label: string }
  | { role: "approval" };

/// What streams in while the lead replies (Rust's `BarEvent`), as far as this reads it.
type SendEvent =
  | { kind: "text"; html: string }
  | { kind: "error"; message: string }
  | { kind: "started" | "reasoning" | "tool_started" | "tool_result" | "sources" | "replies" | "approval" | "auto_picked" | "transcript" | "finished" };

/// What an agent is doing: a line and its state.
type Doing = [line: string, state: "run" | "ok" | "wait" | ""];

const SESSION = new URLSearchParams(location.search).get("session");
/// How often the window asks for news while the team works, and once it
/// has finished (a follow-up from another device can start a new round).
const POLL_BUSY = 2500, POLL_IDLE = 30000;
/// While the team works the lead's chat rarely changes, so it is read on
/// every few polls, and whenever the research changes phase.
const CHAT_EVERY = 4;
const RUNNING: ReadonlySet<Status | undefined> = new Set<Status>(["researching", "reviewing", "writing"]);
const SCOUT_COLORS = ["#5CC49C", "#8FB8E8", "#E8C46D", "#E89A6D", "#9AD0D8"];

const ICON = {
  check: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  x: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  down: '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
  ret: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M20 5v7a3 3 0 0 1-3 3H5m4-4l-4 4 4 4"/></svg>',
};

const $ = <T extends Element = HTMLElement>(sel: string) => document.querySelector<T>(sel);
/// An element the page surely has.
const $$ = <T extends Element = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
/// "1 scout", "2 scouts".
const count = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const ignore = () => {};

const ask = $$<HTMLTextAreaElement>("#ask");

const R = {
  research: null as Research | null, // the last GetResearch, without its entries
  entries: [] as Entry[],
  after: 0,
  turns: [] as Turn[],
  side: { view: "agents", agent: null } as
    | { view: "agents" | "room"; agent: null }
    | { view: "thread"; agent: string },
  pick: null as number | null, // the round the agents panel shows; null follows the newest
  rounds: false, // the round menu is open
  tab: null as string | null, // null follows the research: notes while it works, the draft once written
  sending: null as string | null, // the lead's reply streaming in, as HTML
  error: "",
  note: "",
  exporting: false,
};

// ---------- Agents ----------

function look(id: string) {
  if (id === "lead") return { ini: "L", c: "var(--hot)" };
  if (id === "critic") return { ini: "R", c: "#C59AE8" };
  if (id === "writer") return { ini: "W", c: "var(--fg)" };
  const n = +id.replace("scout-", "") || 1;
  return { ini: String(n), c: SCOUT_COLORS[(n - 1) % SCOUT_COLORS.length] };
}
const rav = (id: string) => { const l = look(id); return `<span class="rav" style="background:${l.c}">${l.ini}</span>`; };
const agent = (id: string): Agent => R.research?.agents.find(a => a.id === id) ?? { id, name: id, role: "", model: "" };
const nameOf = (id: string) => agent(id).name;

/// Which members work in each phase; the others have finished or wait.
const PHASE: Record<string, string[]> = { researching: ["scout"], reviewing: ["critic", "scout"], writing: ["writer", "critic"] };
const kind = (id: string) => id.startsWith("scout-") ? "scout" : id;
const round = () => R.research?.round ?? 1;
/// A scout from an earlier round, done while a later one works.
const earlier = (id: string) => kind(id) === "scout" && (agent(id).round ?? 1) < round();
/// When the newest round started: its scouts' first entry, else the research's start.
function roundStart() {
  const r = R.research;
  if (!r || round() === 1) return r?.started_at ?? Date.now();
  const first = R.entries.find(e => kind(e.agent) === "scout" && agent(e.agent).round === round());
  return first?.at ?? r.started_at;
}

/// What an agent is doing: a line and its state (run, ok, wait or none).
function doing(id: string): Doing {
  const st = R.research?.status ?? "planning";
  const mine = R.entries.filter(e => e.agent === id);
  const notes = mine.filter(e => e.kind === "note").length;
  const sources = R.research?.sources.filter(s => s.agent === id).length ?? 0;
  const said = mine.filter(e => e.kind === "said" && e.to).length;
  const summary = ({ scout: `Done: ${notes} notes, ${sources} sources`, critic: `Done: ${said} challenges`, writer: "Done: the paper", lead: "Posted the result" } as Record<string, string>)[kind(id)];
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
  if (PHASE[st].includes(kind(id))) return [mine.findLast(e => e.kind === "step")?.label ?? "Starting", "run"];
  const starts = ({ critic: "reviewing", writer: "writing", scout: "researching" } as Record<string, string>)[kind(id)];
  if (order.indexOf(starts) > order.indexOf(st)) return [kind(id) === "critic" ? "Starts after the scouts" : "Starts after review", ""];
  return [summary, "ok"];
}

const ok = () => `<span class="dok">${ICON.check}</span>`;
const stateIcon = (st: Doing[1]) => st === "run" ? '<span class="dspin"></span>'
  : st === "ok" ? ok()
  : st === "wait" ? '<span class="ddot" style="background:var(--hot)"></span>' : "<span></span>";

// ---------- Title bar ----------

function minutes(from: number, to?: number | null) {
  return `${Math.max(1, Math.round(((to ?? Date.now()) - from) / 60000))} min`;
}

function drawBar() {
  const r = R.research, st = r?.status;
  $$("#name").textContent = r?.topic || "Research";
  document.title = r?.topic || "Research";
  const working = r ? r.agents.filter(a => a.id !== "lead" && doing(a.id)[1] === "run").length : 0;
  const lines: Record<Status, string> = {
    planning: '<span class="ddot" style="background:var(--hot)"></span>Waiting for your go',
    researching: `<span class="dspin"></span>${round() > 1 ? `Round ${round()}` : "Researching"}, ${count(working, "agent")} working`,
    reviewing: '<span class="dspin"></span>Reviewing the findings',
    writing: '<span class="dspin"></span>Writing the paper',
    done: `${ok()}Done in ${r ? minutes(roundStart(), r.finished_at) : ""}`,
    failed: '<span class="ddot bad"></span>The team failed',
    stopped: '<span class="ddot off"></span>Stopped',
    interrupted: '<span class="ddot off"></span>Interrupted by a server restart',
  };
  $$("#stat").innerHTML = (st ? lines[st] : undefined) ?? (R.sending !== null ? '<span class="dspin"></span>Planning' : "");
  $$("#stop").innerHTML = RUNNING.has(st) ? '<button class="dbtn warn" data-act="stop">Stop</button>' : "";
}

// ---------- The lead's chat ----------

const leadSays = (html: string) => `<div class="rwho">${rav("lead")}Beep, lead</div><div class="reply">${html}</div>`;

function drawChat() {
  const chat = $$("#chat"), stuck = chat.scrollHeight - chat.scrollTop - chat.clientHeight < 40;
  const st = R.research?.status;
  const last = R.turns.findLastIndex(t => t.role === "assistant");
  let html = "";
  R.turns.forEach((t, i) => {
    if (t.role === "user") html += `<div class="ask">${esc(t.text)}</div>`;
    else if (t.role === "scheduled") html += `<div class="dmuted">${esc(t.label || "Research finished")}</div>`;
    else if (t.role === "assistant" && t.html) {
      html += leadSays(t.html);
      if (i === last && st === "planning" && R.sending === null) html += `<div class="racts"><button class="dbtn pri" data-act="go">Start<kbd>⌘${ICON.ret}</kbd></button></div>`;
      if (i === last && st === "done" && R.sending === null) html += `<div class="racts"><button class="dbtn" data-act="tab:draft">Open the paper</button>${exportButton("dbtn")}</div>`;
    }
  });
  if (R.sending !== null) html += leadSays(`${R.sending}<span class="caret"></span>`);
  if (RUNNING.has(st)) html += `<div class="rstatus"><span class="dspin"></span>${esc(progress())}</div>`;
  if (R.note) html += `<div class="dmuted">${esc(R.note)}</div>`;
  if (R.error) html += `<div class="derr">${esc(R.error)}</div>`;
  chat.innerHTML = html;
  if (stuck) chat.scrollTop = chat.scrollHeight;
  ask.placeholder = st === "planning" ? "Say go, or what to change" : st === "done" ? "Ask a follow-up" : "Message the lead";
}

/// The line under the chat while the team works.
function progress() {
  const r = R.research!, st = r.status;
  const scouts = r.agents.filter(a => kind(a.id) === "scout" && !earlier(a.id));
  const took = minutes(roundStart());
  if (st === "researching") {
    const busy = scouts.filter(a => doing(a.id)[1] === "run").length;
    const n = `${busy} of ${count(scouts.length, "scout")} working`;
    return round() > 1 ? `Round ${round()}: ${n}, ${took}` : `Researching: ${n}, ${took}`;
  }
  if (st === "reviewing") return `The critic and the lead are reviewing, ${took}`;
  return round() > 1 ? `The writer is revising the paper, ${took}` : `The writer is drafting, ${took}`;
}

async function loadChat() {
  const turns = await invoke<Turn[]>("research_history", { session: SESSION }).catch(() => null);
  if (!turns || R.sending !== null) return;
  if (JSON.stringify(turns) === JSON.stringify(R.turns)) return;
  R.turns = turns;
  drawChat();
}

/// Sends `text` to the lead; `start` makes the chat a research first.
async function send(text: string, start = false) {
  text = text.trim();
  if (!text || R.sending !== null) return;
  R.error = "";
  R.note = "";
  R.turns.push({ role: "user", text });
  R.sending = "";
  drawChat();
  drawBar();
  const channel = new Channel<SendEvent>();
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

/// Citation numbers as links to their sources, which open in the browser.
function sup(numbers: number[]) {
  const links = numbers.map(n => {
    const s = R.research?.sources.find(s => s.number === n);
    return s ? `<a class="rcite" href="${esc(s.url)}" title="${esc(s.title)}">${n}</a>` : String(n);
  });
  return `<sup>${links.join(", ")}</sup>`;
}

/// Turns "[3]" and "[1, 4]" in rendered text into citation marks.
function cite(html: string) {
  const t = document.createElement("template");
  t.innerHTML = html;
  const walk = document.createTreeWalker(t.content, NodeFilter.SHOW_TEXT);
  const texts: Text[] = [];
  while (walk.nextNode()) if (!walk.currentNode.parentElement?.closest("a, code, pre")) texts.push(walk.currentNode as Text);
  for (const node of texts) {
    if (!/\[\d+(?:\s*,\s*\d+)*\]/.test(node.data)) continue;
    const span = document.createElement("span");
    span.innerHTML = esc(node.data).replace(/\[(\d+(?:\s*,\s*\d+)*)\]/g, (_, list: string) => sup(list.split(",").map(Number)));
    node.replaceWith(...span.childNodes);
  }
  return t.innerHTML;
}

/// A note, headed `title`, with its citations.
function noteHtml(n: Entry & Note, title: string) {
  const src = n.sources.length ? ` ${sup(n.sources)}` : "";
  return `<div class="rnote" style="--c:${look(n.agent).c}"><b>${title}</b><span>${esc(n.content)}${src}</span></div>`;
}

function drawWork() {
  const r = R.research, st = r?.status ?? "planning", t = R.tab || (r?.draft ? "draft" : "notes");
  const notes = R.entries.filter(e => e.kind === "note");
  const b = (id: string, label: string) => `<button role="tab" aria-selected="${t === id}" data-act="tab:${id}">${label}</button>`;
  const sources = r?.sources.length ? ` ${r.sources.length}` : "";
  $$("#tabs").innerHTML = b("draft", "Draft") + b("notes", "Notes") + b("sources", `Sources${sources}`) + '<span class="sp"></span>'
    + (r?.draft ? '<button class="dbtn quiet" data-act="copy">Copy</button>' + exportButton("dbtn quiet") : "");
  let body;
  if (!r || (st === "planning" && !notes.length)) body = '<div class="dmuted rempty">The workspace fills in once the team starts.</div>';
  else if (t === "draft" && !r.draft) body = `<div class="dmuted rempty">${RUNNING.has(st) ? "The writer starts after the critic's review." : "No paper was written."}</div>`;
  else if (t === "draft") {
    const by = `Written by ${esc(nameOf("writer"))}, reviewed by ${esc(nameOf("critic"))}. ${r.sources.length} sources.`;
    body = `<div class="rdoc"><span class="by">${by}</span>${cite(r.draft_html)}</div>`;
  } else if (t === "notes") {
    body = notes.length ? `<div class="rdoc">${notes.map(n => noteHtml(n, esc(nameOf(n.agent)))).join("")}</div>` : '<div class="dmuted rempty">No notes yet.</div>';
  } else {
    body = r.sources.length ? r.sources.map(s => `<div class="rsrc"><span class="n">${s.number}</span><a href="${esc(s.url)}">${esc(s.title)}</a>
      <small>${esc(host(s.url))}, kept by ${esc(nameOf(s.agent))}</small><q>${esc(s.quote)}</q></div>`).join("") : '<div class="dmuted rempty">No sources yet.</div>';
  }
  const work = $$("#work"), top = work.scrollTop;
  work.innerHTML = body;
  work.scrollTop = top;
}

function host(url: string) {
  try { return new URL(url).host.replace(/^www\./, ""); } catch { return url; }
}

// ---------- Agents panel ----------

function msg(e: Entry & Said) {
  const to = e.to ? ` <span class="to">to ${esc(nameOf(e.to))}</span>` : "";
  return `<div class="rmsg">${rav(e.agent)}<b>${esc(nameOf(e.agent))}${to}</b><p>${esc(e.content)}</p></div>`;
}

const BACK = `<button data-act="back" aria-label="Back to agents">${ICON.x}</button>`;

/// One agent's steps, notes and messages, and what it does now.
function threadHtml(a: Agent) {
  const mine = R.entries.filter(e => e.agent === a.id || (e.kind === "said" && e.to === a.id));
  let html = "", steps: string[] = [];
  const flush = () => { if (steps.length) html += `<div class="dsteps">${steps.join("")}</div>`; steps = []; };
  for (const e of mine) {
    if (e.kind === "step" && e.agent === a.id) {
      steps.push(`<div class="dstep"><span class="s">${e.success ? ok() : `<span class="dbad">${ICON.x}</span>`}</span><span>${esc(e.label)}</span></div>`);
    } else if (e.kind === "said") { flush(); html += msg(e); }
    else if (e.kind === "note") { flush(); html += noteHtml(e, "Note"); }
  }
  flush();
  const [now, st] = doing(a.id);
  if (st === "run") html += `<div class="dsteps"><div class="dstep run"><span class="s"><span class="dspin"></span></span><span>${esc(now)}</span></div></div>`;
  return html;
}

/// The agents of the round picked, the room's button and the round menu.
function agentsHtml() {
  const r = R.research;
  const row = (a: Agent) => {
    const [now, st] = doing(a.id);
    return `<button class="rag" data-act="agent:${esc(a.id)}">${rav(a.id)}<b>${esc(a.name)}</b>${stateIcon(st)}<small>${esc(now || a.role)}</small><span class="model">${esc(a.model)}</span></button>`;
  };
  // One round's scouts at a time, picked in the header; the lead, critic and writer work in every round.
  const agents = r?.agents ?? [{ id: "lead", name: "Lead", role: "Plans and coordinates", model: "", round: 0 }];
  const rounds = round(), pick = Math.min(R.pick ?? rounds, rounds);
  const scoutOf = (a: Agent) => kind(a.id) === "scout" ? Math.max(a.round ?? 0, 1) : 0;
  const rows = [
    ...agents.filter(a => a.id === "lead"),
    ...agents.filter(a => scoutOf(a) === pick),
    ...agents.filter(a => a.id === "critic" || a.id === "writer"),
  ].map(row).join("");
  const scouts = (n: number) => count(agents.filter(a => scoutOf(a) === n).length, "scout");
  const menu = R.rounds ? `<span class="dmenu" role="menu">${Array.from({ length: rounds }, (_, i) => rounds - i).map(n =>
    `<button class="mi" role="menuitemradio" aria-checked="${n === pick}" data-act="round:${n}"><b>Round ${n}</b>${n === pick ? `<span class="ok">${ICON.check}</span>` : ""}<small>${scouts(n)}</small></button>`).join("")}</span>` : "";
  const picker = rounds > 1 ? `<span class="rround"><button class="dbtn" data-act="rounds" aria-haspopup="menu" aria-expanded="${R.rounds}">Round ${pick}${ICON.down}</button>${menu}</span>` : "";
  const said = R.entries.filter(e => e.kind === "said").length;
  const room = r && r.status !== "planning" ? count(said, "message") : "Opens when the team starts";
  return `<div class="rpane"><div class="rsec rhead">Agents${picker}</div>${rows}<div class="rhr"></div>
      <button class="rag" data-act="room"><span class="rav" style="background:var(--elev);color:var(--fg)">#</span><b>Room</b><span></span><small>${room}</small></button></div>`;
}

function drawSide() {
  const side = $$("#side"), view = R.side.view;
  $$("#grid").classList.toggle("wide", view !== "agents");
  side.classList.toggle("dim", view === "agents");
  if (R.side.view === "thread") {
    const a = agent(R.side.agent);
    side.innerHTML = `<div class="rthead">${rav(a.id)}<span class="who"><b>${esc(a.name)}</b><small>${esc(a.role)} · ${esc(a.model)}</small></span>${BACK}</div>
      <div class="rpane thread">${threadHtml(a) || '<div class="dmuted">Nothing yet.</div>'}</div>`;
  } else if (view === "room") {
    const said = R.entries.filter(e => e.kind === "said");
    side.innerHTML = `<div class="rthead"><b>Room</b><small>where the agents talk</small>${BACK}</div>
      <div class="rpane thread">${said.map(msg).join("") || '<div class="dmuted">Nobody has said anything yet.</div>'}</div>`;
  } else side.innerHTML = agentsHtml();
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

/// Takes a research from Rust: the entries are only those after R.after.
function take(r: ResearchView) {
  const { entries, ...research } = r;
  R.entries.push(...entries);
  if (entries.length) R.after = entries.at(-1)!.seq;
  R.research = research;
}

let timer: ReturnType<typeof setTimeout> | undefined, polls = 0;
const busy = () => !R.research || R.research.status === "planning" || RUNNING.has(R.research.status);
async function poll(now = false) {
  clearTimeout(timer);
  if (!now && (!busy() || ++polls % CHAT_EVERY === 0)) await loadChat();
  try {
    const r = await invoke<ResearchView | null>("get_research", { session: SESSION, after: R.after });
    if (r) {
      const was = R.research?.status;
      take(r);
      if (was && was !== r.status) await loadChat();
      draw();
    }
  } catch (err) {
    console.warn("research poll", err);
  }
  timer = setTimeout(poll, busy() ? POLL_BUSY : POLL_IDLE);
}

// ---------- Input ----------

async function act(a: string, v: string | undefined) {
  if (a === "go") return send("Go");
  if (a === "stop") {
    await invoke("stop_research", { session: SESSION }).catch(err => { R.error = String(err); });
    return poll(true);
  }
  if (a === "tab") { R.tab = v!; return drawWork(); }
  if (a === "agent") { if (R.research?.status === "planning" && v !== "lead") return; R.side = { view: "thread", agent: v! }; return drawSide(); }
  if (a === "room") { if (!R.research || R.research.status === "planning") return; R.side = { view: "room", agent: null }; return drawSide(); }
  if (a === "back") { R.side = { view: "agents", agent: null }; return drawSide(); }
  if (a === "rounds") { R.rounds = !R.rounds; return drawSide(); }
  if (a === "round") { R.pick = Number(v) === round() ? null : Number(v); R.rounds = false; return drawSide(); }
  if (a === "copy") return copy(R.research?.draft ?? "");
  if (a === "export") return exportPdf();
  if (a === "quote") return quote();
}

/// The Export PDF button, which shows the export while it runs.
function exportButton(cls: string) {
  return R.exporting
    ? `<button class="${cls}" disabled><span class="dspin"></span>Exporting</button>`
    : `<button class="${cls}" data-act="export">Export PDF</button>`;
}

async function exportPdf() {
  if (R.exporting) return;
  R.exporting = true;
  notice("Typesetting the PDF", { busy: true });
  draw();
  try {
    const path = await invoke<string>("export_research", { session: SESSION });
    notice(`Saved ${path.split("/").pop()} to Downloads`, { path });
  } catch (err) {
    notice(String(err), { bad: true });
  }
  R.exporting = false;
  draw();
}

/// A short note over the workspace; one that names a saved file can show it
/// in Finder. It goes after a while, unless it is waiting on something.
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
function notice(text: string, { path, bad, busy }: { path?: string; bad?: boolean; busy?: boolean } = {}) {
  clearTimeout(noticeTimer);
  $("#saved")?.remove();
  const el = document.createElement("div");
  el.className = `dsaved${bad ? " bad" : ""}`;
  el.id = "saved";
  el.setAttribute("role", "status");
  el.innerHTML = `${busy ? '<span class="dspin"></span>' : ""}<span>${esc(text)}</span>${path ? '<button class="dbtn">Show in Finder</button>' : ""}`;
  el.querySelector("button")?.addEventListener("click", () => invoke("reveal_file", { path }).catch(ignore));
  $$(".rwork").append(el);
  if (!busy) noticeTimer = setTimeout(() => el.remove(), bad ? 10000 : 6000);
}

// ---------- Quoting ----------

/// The selected text in the workspace or an agent's thread, if any.
function selectedText() {
  const sel = getSelection();
  if (!sel || sel.isCollapsed || !sel.rangeCount) return "";
  const node = sel.getRangeAt(0).commonAncestorContainer;
  const el = node.nodeType === 1 ? node as Element : node.parentElement;
  if (!el?.closest("#work, #side, #chat") || el.closest(".dinput")) return "";
  return sel.toString().trim();
}

/// Shows the Quote button over the selection, or hides it.
function placeQuote() {
  $("#quote")?.remove();
  const text = selectedText();
  if (!text) return;
  const rect = getSelection()!.getRangeAt(0).getBoundingClientRect();
  const b = document.createElement("button");
  b.id = "quote";
  b.className = "dbtn pri rquote";
  b.dataset.act = "quote";
  b.innerHTML = "Quote<kbd>⌘'</kbd>";
  document.body.append(b);
  const w = b.offsetWidth, h = b.offsetHeight;
  const left = Math.max(8, Math.min(innerWidth - w - 8, rect.left + rect.width / 2 - w / 2));
  const top = rect.top - h - 8 < 46 ? rect.bottom + 8 : rect.top - h - 8;
  b.style.left = `${left}px`;
  b.style.top = `${top}px`;
}

/// Adds the selection to the message as a quote and puts the cursor under
/// it, so several quotes with replies make one message.
function quote() {
  const text = selectedText();
  $("#quote")?.remove();
  if (!text) return;
  const quoted = text.split(/\n+/).map(line => `> ${line.trim()}`).filter(line => line !== ">").join("\n");
  const before = ask.value.replace(/\s+$/, "");
  ask.value = `${before ? before + "\n\n" : ""}${quoted}\n\n`;
  getSelection()!.removeAllRanges();
  ask.focus();
  ask.setSelectionRange(ask.value.length, ask.value.length);
  growAsk();
}

// Pressing the button must not clear the selection it quotes.
document.addEventListener("mousedown", e => { if ((e.target as Element).closest("#quote")) e.preventDefault(); });
document.addEventListener("mouseup", () => setTimeout(placeQuote));
document.addEventListener("selectionchange", () => { if (!selectedText()) $("#quote")?.remove(); });
for (const id of ["work", "side", "chat"]) document.getElementById(id)!.addEventListener("scroll", () => $("#quote")?.remove(), true);

async function copy(text: string) {
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
  notice("Copied the paper as Markdown");
}

document.addEventListener("click", e => {
  const target = e.target as Element;
  const b = target.closest<HTMLElement>("[data-act]");
  const name = b?.dataset.act;
  if (R.rounds && name !== "rounds" && !name?.startsWith("round:")) { R.rounds = false; drawSide(); }
  if (b) {
    const [a, v] = b.dataset.act!.split(/:(.*)/);
    return void act(a, v);
  }
  const link = target.closest<HTMLAnchorElement>("a[href]");
  if (!link) return;
  e.preventDefault();
  invoke("open_url", { url: link.href }).catch(ignore);
});

const ASK_HEIGHT = "research.askHeight";
// A height the user dragged the field to, kept for every research window.
let askHeight: number | null = null;
try { askHeight = Number(localStorage.getItem(ASK_HEIGHT)) || null; } catch {}
function maxAsk() { return Math.round(innerHeight * 0.6); }
function growAsk() {
  if (askHeight) {
    ask.style.height = Math.min(askHeight, maxAsk()) + "px";
    return;
  }
  ask.style.height = "auto";
  ask.style.height = Math.min(ask.scrollHeight, 220) + "px";
  ask.scrollTop = ask.scrollHeight;
}
ask.addEventListener("input", growAsk);
addEventListener("resize", growAsk);
const grip = $$("#grip");
grip.addEventListener("pointerdown", e => {
  e.preventDefault();
  grip.setPointerCapture(e.pointerId);
  grip.classList.add("on");
  const startY = e.clientY, startH = ask.offsetHeight;
  const move = (m: PointerEvent) => {
    askHeight = Math.max(20, Math.min(startH + startY - m.clientY, maxAsk()));
    growAsk();
  };
  const up = () => {
    grip.classList.remove("on");
    grip.removeEventListener("pointermove", move);
    grip.removeEventListener("pointerup", up);
    try { localStorage.setItem(ASK_HEIGHT, String(askHeight)); } catch {}
  };
  grip.addEventListener("pointermove", move);
  grip.addEventListener("pointerup", up);
});
grip.addEventListener("dblclick", () => {
  askHeight = null;
  try { localStorage.removeItem(ASK_HEIGHT); } catch {}
  growAsk();
});
document.addEventListener("keydown", e => {
  if (e.metaKey && e.key === "Enter" && R.research?.status === "planning" && R.sending === null) {
    e.preventDefault();
    return void send("Go");
  }
  if (e.key === "Escape" && R.rounds) { R.rounds = false; return drawSide(); }
  if (e.metaKey && e.key.toLowerCase() === "w") { e.preventDefault(); return window.close(); }
  if (e.metaKey && e.key === "'" && selectedText()) { e.preventDefault(); return quote(); }
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.target === ask) {
    e.preventDefault();
    // The lead is still answering: keep what was typed for after.
    if (R.sending !== null) return;
    const text = ask.value;
    ask.value = "";
    growAsk();
    return void send(text);
  }
});

(async () => {
  // What this Mac already holds draws at once; the poll adds what is new.
  const cached = await invoke<ResearchView | null>("cached_research", { session: SESSION }).catch(() => null);
  if (cached) take(cached);
  draw();
  await loadChat();
  const topic = await invoke<string | null>("research_pending", { session: SESSION }).catch(() => null);
  if (topic && !R.turns.length) send(topic, true);
  poll(true);
  ask.focus();
})();
