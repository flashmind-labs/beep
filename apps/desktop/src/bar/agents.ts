import { drawAgentNew, renderAgentNew, startAgent } from "./agentnew.ts";
import { fileBase64 } from "./attach.ts";
import { renderChat } from "./chat.ts";
import { $, bar, esc, h, ICON, invoke, listen, PANE, S, windowHooks } from "./core.ts";
import { hints } from "./hints.ts";
import { openSession } from "./history.ts";
import { back } from "./keys.ts";
import { loadPic, openViewer, pictures } from "./media.ts";
import { cap } from "./tools.ts";

// ---------- Coding agents ----------

// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike.
export const AG = {
  images: new Map(), readError: "",
  hosts: null, sessions: [], error: "", busy: false,
  form: { prompt: "", machine: "auto", agent: null, cwd: "", chat: null },
  open: null, page: null, why: "", back: "chat", timer: 0, inflight: null, note: "", sel: 0,
  // Polling: what the last answer looked like, when it last changed,
  // whether it stopped for quiet, and the question an answer is going to.
  sig: "", changed: 0, paused: false, answering: null, barHidden: false,
  // Images pasted or dropped into the session's field, the error from the
  // last one, and the session the server turned images down for.
  att: [], attError: "", noImages: null,
  // The tool entries opened to their full output, by place in the session,
  // and the module that reads tool entries, once loaded.
  opened: new Set(), log: null,
};
export const agentLog = () => import("../agentlog.ts").then(m => (AG.log = m));
/// What a coding agent takes with a message, as the server checks it.
export const AGENT_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
export const AGENT_MAX_IMAGES = 5;
export const AGENT_MAX_IMAGE = 5 * 1024 * 1024;
export const AGENT_MAX_IMAGES_TOTAL = 7 * 1024 * 1024;
/// Thumbnails of images sent from here, by session and name, so the
/// transcript's `[image: name]` lines show the picture.
export const agentPics = new Map();
export const AGENT_PICS_KEPT = 60;
export let pastedCount = 0;
/// How often the session view asks for its session while on screen.
export const AGENT_POLL_MS = 1500;
/// A quiet session (idle, or started outside Beep) unchanged this long is
/// asked every `AGENT_SLOW_MS`, and after `AGENT_PAUSE_AFTER` no more.
export const AGENT_SLOW_AFTER = 2 * 60_000;
export const AGENT_SLOW_MS = 10_000;
export const AGENT_PAUSE_AFTER = 5 * 60_000;
export const AGENT_ACTIVE = new Set(["asking", "working", "running", "starting"]);
export const AGENT_LAST = 60;
export const AGENT_KINDS = {
  claude: { ini: "CC", name: "Claude Code", cls: "claude" },
  codex: { ini: "CX", name: "Codex", cls: "codex" },
  gemini: { ini: "GE", name: "Gemini CLI", cls: "gemini" },
  pi: { ini: "PI", name: "pi", cls: "pi" },
  flsh: { ini: "FL", name: "flsh", cls: "flsh" },
  opencode: { ini: "OC", name: "OpenCode", cls: "opencode" },
};
export const AGENT_ORDER = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];
export const CAP_NAMES = { gpu: "GPU", cuda: "CUDA", blender: "Blender", unreal: "Unreal", xcode: "Xcode", android: "Android", docker: "Docker" };
export const AGENT_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

export const agentKind = a => AGENT_KINDS[a] ?? { ini: String(a || "?").slice(0, 2).toUpperCase(), name: a || "Agent", cls: "" };
export const agentAvatar = a => `<span class="dapp dcoderav ${agentKind(a).cls}">${esc(agentKind(a).ini)}</span>`;
export const folderName = p => String(p || "").split("/").filter(Boolean).pop() || p || "";
export const byPreference = (a, b) => (AGENT_ORDER.indexOf(a) + 1 || 99) - (AGENT_ORDER.indexOf(b) + 1 || 99);

/// The machines that can run an agent; none for an account that may not use machines.
export async function loadAgentHosts() {
  if (S.link !== "linked") return;
  try {
    AG.hosts = await invoke("agent_hosts");
  } catch {
    AG.hosts = [];
  }
  drawAgentButton();
  if (S.view === "agentnew") drawAgentNew();
}

/// The composer's Agent button, once some machine offers an agent.
export function drawAgentButton() {
  const row = bar.querySelector(":scope > .dinput.dgrow");
  if (!row || S.view !== "chat" || $("#agentGo") || !AG.hosts?.length) return;
  const b = h(`<button class="dbtn quiet" id="agentGo" title="Start a coding agent on a machine (⌘⇧A)">${AGENT_ICON}Agent</button>`);
  b.onclick = () => renderAgentNew($("#ask")?.value.trim() ?? "");
  row.insertBefore(b, $("#grip"));
}

export function stateTag(status) {
  if (status === "asking") return '<span class="dstate ask">Waiting for you</span>';
  if (status === "working" || status === "running" || status === "starting") return `<span class="dstate run">${status === "starting" ? "Starting" : "Working"}</span>`;
  if (status === "waiting") return '<span class="dstate ask">Waiting in terminal</span>';
  if (status === "ended") return '<span class="dstate">Ended</span>';
  return '<span class="dstate">Idle</span>';
}

// ----- One session -----

/// Opens session `s` (a session row) and follows it while it is on screen.
export function openAgent(s, { why = "", back: from } = {}) {
  if (from === undefined) from = S.view === "agents" ? "agents" : "chat";
  S.view = "agent";
  AG.open = { machine_id: s.machine_id, agent: s.agent, session: s.id };
  AG.page = { session: s, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 };
  AG.why = why;
  AG.back = from;
  AG.note = "";
  AG.sig = "";
  AG.changed = Date.now();
  AG.paused = false;
  AG.answering = null;
  AG.att = [];
  AG.attError = "";
  AG.noImages = null;
  AG.images = new Map();
  AG.opened = new Set();
  drawAgentFrame();
  pollAgent();
  loadAgentImages(AG.open);
}

export const isOpen = o => S.view === "agent" && AG.open && o === AG.open;

/// Whether the session view can be seen: the window is not hidden, and
/// the bar, when this is the bar, is up.
export const agentOnScreen = () => document.visibilityState === "visible" && !AG.barHidden;

/// How long until the open session is asked again, or null to stop: an
/// ended session, or a quiet one that has not changed for a while.
export function agentDelay() {
  const s = AG.page?.session;
  if (!s || s.status === "ended") return null;
  if (!s.external && AGENT_ACTIVE.has(s.status)) return AGENT_POLL_MS;
  const still = Date.now() - AG.changed;
  if (still >= AGENT_PAUSE_AFTER) return null;
  return still >= AGENT_SLOW_AFTER ? AGENT_SLOW_MS : AGENT_POLL_MS;
}

/// Asks for the open session, then again after `agentDelay()` while it
/// stays on screen. Never two calls at once for one session.
export async function pollAgent() {
  clearTimeout(AG.timer);
  const o = AG.open;
  if (!isOpen(o) || AG.inflight === o || !agentOnScreen()) return;
  AG.inflight = o;
  try {
    const page = await invoke("agent_view", { machineId: o.machine_id, agent: o.agent, session: o.session, last: AGENT_LAST });
    if (isOpen(o)) {
      const sig = JSON.stringify(page);
      if (sig !== AG.sig) { AG.sig = sig; AG.changed = Date.now(); }
      if (page.session.question?.id !== AG.answering) AG.answering = null;
      AG.page = page;
      // What the last failed read said no longer holds.
      if (AG.note === "gone" || AG.note === AG.readError) AG.note = "";
      AG.readError = "";
      drawAgentBody();
    }
  } catch (e) {
    if (isOpen(o)) {
      // A dropped connection comes back by itself; the next read says so.
      AG.readError = /not found/i.test(String(e)) ? "" : /connection lost|closed by peer|timed out/i.test(String(e)) ? "Reconnecting to Beep…" : String(e);
      AG.note = AG.readError || "gone";
      drawAgentBody();
    }
  } finally {
    if (AG.inflight === o) AG.inflight = null;
  }
  // Another session opened while this one was asked: follow that one.
  if (AG.open !== o) { if (isOpen(AG.open) && !AG.inflight) pollAgent(); return; }
  if (!isOpen(o) || !agentOnScreen()) return;
  const delay = agentDelay();
  const paused = delay === null && AG.page?.session.status !== "ended";
  if (paused !== AG.paused) { AG.paused = paused; drawAgentBody(); }
  if (delay !== null) AG.timer = setTimeout(pollAgent, delay);
}

/// Follows the open session again after a pause or a message.
export function resumeAgent() {
  AG.changed = Date.now();
  AG.paused = false;
  pollAgent();
}

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") { if (S.view === "agent") pollAgent(); } else clearTimeout(AG.timer);
});
if (!PANE) {
  listen("bar-hidden", () => { AG.barHidden = true; clearTimeout(AG.timer); });
  listen("bar-shown", () => { AG.barHidden = false; if (S.view === "agent") pollAgent(); });
}

/// Whether the server turned images down for the open session.
export const noAgentImages = () => AG.noImages != null && AG.noImages === AG.open?.session;

export function agentHints() {
  const s = AG.page?.session, q = s?.question;
  if (q) return hints([[`⌘${ICON.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]]);
  if (s?.external) return hints([["esc", "back"]]);
  if (s?.status === "ended") return hints([[ICON.ret, "continue"], ["esc", "back"]]);
  // Once one is attached its chip says enough.
  const paste = !AG.att.length && !noAgentImages() ? [["⌘V", "paste an image"]] : [];
  const stop = AGENT_ACTIVE.has(s?.status) ? [["⌘.", "stop the turn"]] : [];
  return hints([...paste, [ICON.ret, "send"], ...stop, ["esc", "back"]]);
}

// ----- Images for the agent -----

/// A name for an image, unique among those waiting; a pasted one is
/// named "Pasted image 1.png" and so on rather than "image.png".
export function agentImageName(f) {
  const ext = { "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" }[f.type] || "png";
  let name = f.name && f.name !== `image.${ext}` && f.name !== "image.png" ? f.name : `Pasted image ${++pastedCount}.${ext}`;
  const dot = name.lastIndexOf(".");
  const base = name;
  for (let n = 2; AG.att.some(a => a.name === name); n++) name = dot > 0 ? `${base.slice(0, dot)} ${n}${base.slice(dot)}` : `${base} ${n}`;
  return name;
}

export function agentAttachError(text) {
  AG.attError = text;
  drawAgentAttached();
  clearTimeout(AG.attTimer);
  AG.attTimer = setTimeout(() => { AG.attError = ""; drawAgentAttached(); }, 5000);
}

/// Adds pasted or dropped images to the session's next message, refusing
/// what the agent cannot take rather than shrinking or dropping it.
export async function agentAttach(files) {
  if (noAgentImages()) return agentAttachError("This Beep server does not take images yet.");
  for (const f of [...files]) {
    if (!AGENT_IMAGE_TYPES.includes(f.type)) { agentAttachError(`${f.name || "That file"} is not a PNG, JPEG, GIF or WebP image`); continue; }
    if (AG.att.length >= AGENT_MAX_IMAGES) { agentAttachError(`Up to ${AGENT_MAX_IMAGES} images in one message`); break; }
    if (f.size > AGENT_MAX_IMAGE) { agentAttachError(`${f.name || "That image"} is over ${AGENT_MAX_IMAGE >> 20} MB`); continue; }
    const used = AG.att.reduce((n, a) => n + a.size, 0);
    if (used + f.size > AGENT_MAX_IMAGES_TOTAL) { agentAttachError(`Images up to ${AGENT_MAX_IMAGES_TOTAL >> 20} MB in all in one message`); continue; }
    try {
      const data = await fileBase64(f);
      AG.att.push({ name: agentImageName(f), mime: f.type, size: f.size, data, url: `data:${f.type};base64,${data}` });
    } catch {
      agentAttachError(`${f.name || "That image"} could not be read`);
    }
  }
  drawAgentAttached();
}

/// The chips of waiting images above the field, as in the mock.
export function drawAgentAttached() {
  const field = $("#agfield");
  if (!field) return;
  const old = $("#agatt");
  const el = AG.att.length || AG.attError ? h(`<div class="ag-att" id="agatt">${AG.att.map((a, i) =>
    `<span class="chip"><img src="${esc(a.url)}" alt=""><span class="nm">${esc(a.name)}</span><button aria-label="Remove ${esc(a.name)}" data-agatt="${i}">${ICON.x}</button></span>`).join("")}${AG.attError ? `<span class="derr">${esc(AG.attError)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll("[data-agatt]").forEach(b => b.addEventListener("click", () => {
    AG.att.splice(Number(b.dataset.agatt), 1);
    drawAgentAttached();
    $("#agmsg")?.focus();
  }));
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) field.before(el);
  const hint = bar.querySelector(":scope > .dhints");
  if (hint && S.view === "agent") hint.replaceWith(h(agentHints()));
}

/// Reads the images the server kept for the open session, by the name its
/// log gives each and by its key, for an agent that replays it without one.
export async function loadAgentImages(o) {
  let got = [];
  try { got = await invoke("agent_images", { machineId: o.machine_id, session: o.session }); } catch { return; }
  if (!isOpen(o)) return;
  AG.images = new Map();
  for (const i of got) { AG.images.set(i.name, i); AG.images.set(`image-${i.key}`, i); }
  if (AG.page) drawAgentBody();
}

/// Fills the session pictures drawn before their bytes were here.
export function fillAgentPics(b) {
  for (const img of b.querySelectorAll("img.pic[data-file]:not([src])")) {
    const id = img.dataset.file;
    loadPic(id, img.dataset.mime).then(url => { if (url) img.src = url; });
  }
}

/// Keeps the thumbnails of images just sent, for the transcript.
export function keepAgentPics(o, images) {
  for (const a of images) {
    const key = `${o.session}\n${a.name}`;
    agentPics.delete(key);
    agentPics.set(key, a.url);
  }
  while (agentPics.size > AGENT_PICS_KEPT) agentPics.delete(agentPics.keys().next().value);
}

/// The parts that stay while the session updates: the head, the body and the field.
export function drawAgentFrame() {
  const s = AG.page.session, k = agentKind(s.agent);
  const field = s.external
    ? `<div class="dmuted" style="display:block;padding:10px 16px;border-top:1px solid var(--border)">Started outside Beep. Beep follows it here; to steer it, ask Beep in a chat to take it over once it stops.</div>`
    : `<div class="dinput small" id="agfield"><input id="agmsg" autocomplete="off" spellcheck="false" placeholder="Message ${esc(k.name)}"><button class="dbtn" id="agstop" aria-label="Stop the turn"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop<kbd>⌘.</kbd></button><button class="dbtn quiet" id="agclose">Close session</button></div>`;
  bar.innerHTML = `<div class="dhead">${agentAvatar(s.agent)}<span class="tt"><b>${esc(k.name)} on ${esc(s.machine_name)}</b><small style="font-family:var(--mono)">${esc(s.cwd)}</small></span><span id="agstate">${stateTag(s.status)}</span></div>
    <div class="dbody" id="agbody" style="padding-top:10px"></div>${field}${agentHints()}`;
  drawAgentAttached();
  const b = $("#agbody");
  b.stuck = true;
  b.onscroll = () => { b.stuck = b.scrollHeight - b.scrollTop - b.clientHeight < 40; };
  drawAgentBody();
  $("#agmsg")?.focus();
  bar.onclick = e => {
    if (S.view !== "agent") return;
    const opt = e.target.closest("[data-agopt]");
    if (opt) return answerAgentQuestion(Number(opt.dataset.agopt));
    const tool = e.target.closest("#agbody .dcoderen.tool[data-at]");
    if (tool && !getSelection().toString()) {
      const at = Number(tool.dataset.at);
      if (!AG.opened.delete(at)) AG.opened.add(at);
      return drawAgentBody();
    }
    const pic = e.target.closest("#agbody img.pic");
    if (pic) {
      const all = [...pic.parentElement.querySelectorAll("img.pic")];
      // A kept one opens as the file it is, to save or copy.
      const shown = all.map(i => i.dataset.file
        ? { file: { id: i.dataset.file, name: i.alt, mime: i.dataset.mime }, caption: "" }
        : { url: i.src, name: i.alt, caption: "" });
      return openViewer(shown, all.indexOf(pic));
    }
    if (e.target.closest("#agresume")) return resumeAgent();
    if (e.target.closest("#agstop")) return steerAgent({ cancel: null });
    if (e.target.closest("#agclose")) return steerAgent({ close: null });
  };
}

/// A user entry's text and the images it carried: the node writes each
/// as a line `[image: name]` after the text.
export function userImages(text) {
  const lines = String(text).split("\n"), names = [];
  while (lines.length) {
    const m = /^\[image: (.+)\]$/.exec(lines[lines.length - 1]);
    if (!m) break;
    names.unshift(m[1]);
    lines.pop();
  }
  return { text: lines.join("\n"), names };
}

/// A tool entry as a row: a mark for how it went, its title in mono and
/// the first lines of its output. `at` is its place in the session, which
/// keeps it open across redraws.
export function toolRow(en, at) {
  const { title, status, output } = AG.log.parseTool(en.text);
  const open = AG.opened.has(at), { shown, more } = AG.log.toolLines(output, open);
  const running = AGENT_ACTIVE.has(AG.page?.session.status);
  const mark = status === "completed" ? `<span class="dok" title="Done">${ICON.check}</span>`
    : status === "failed" ? `<span class="dbad" title="Failed">${ICON.x}</span>`
    : status && running ? `<span class="dspin" title="Running"></span>`
    : status ? '<span class="ddot off" title="Did not finish"></span>' : "";
  const long = title.length > 140 || title.includes("\n");
  const opens = more > 0 || open || long;
  const toggle = more > 0 ? `<span class="more">${more === 1 ? "1 more line" : `${more} more lines`}</span>`
    : open && (output.includes("\n") || long) ? '<span class="more">Show less</span>' : "";
  return `<div class="dcoderen tool${status === "failed" ? " failed" : ""}${open ? " open" : ""}"${opens ? ` data-at="${at}"` : ""}>`
    + `<span class="mk">${mark}</span><code class="tt">${esc(title)}</code>`
    + (shown ? `<pre class="out">${esc(shown)}</pre>` : "") + toggle + "</div>";
}

export function entryRow(en, at) {
  const text = esc(en.text);
  if (en.kind === "user") {
    const { text: said, names } = userImages(en.text);
    if (!names.length) return `<div class="dcoderen user"><b>You</b><span>${text}</span></div>`;
    const pics = names.map(n => {
      const kept = AG.images.get(n), label = kept?.name || (/^image-[0-9a-f]{16}$/.test(n) ? "Image" : n);
      const loaded = kept && pictures.get(kept.file_id);
      const url = (AG.open && agentPics.get(`${AG.open.session}\n${n}`)) || (typeof loaded === "string" ? loaded : "");
      const file = kept ? ` data-file="${esc(kept.file_id)}" data-mime="${esc(kept.mime)}"` : "";
      if (url) return `<img class="pic" src="${esc(url)}"${file} alt="${esc(label)}" title="${esc(label)}">`;
      if (kept) return `<img class="pic"${file} alt="${esc(label)}" title="${esc(label)}">`;
      return `<span class="pic none" title="${esc(label)}">${ICON.image}${esc(label)}</span>`;
    }).join("");
    return `<div class="dcoderen user"><b>You</b>${said ? `<span>${esc(said)}</span>` : ""}<span class="pics">${pics}</span></div>`;
  }
  // The HTML is the desktop's own Markdown render, cleaned of anything that runs.
  if (en.kind === "agent") return en.html ? `<div class="dcoderen agent md">${en.html}</div>` : `<div class="dcoderen agent">${text}</div>`;
  if (en.kind === "tool") return AG.log ? toolRow(en, at) : `<div class="dcoderen tool">${text}</div>`;
  return `<div class="dcoderen other">${text}</div>`;
}

export function drawAgentBody() {
  const b = $("#agbody"), p = AG.page;
  if (!b || !p) return;
  const s = p.session, q = s.question;
  const st = $("#agstate");
  if (st) st.innerHTML = stateTag(s.status);
  const mark = x => x === "completed" ? `<span class="dok">${ICON.check}</span>` : x === "in_progress" ? (s.status === "asking" ? '<span class="ddot" style="background:var(--coral)"></span>' : '<span class="dspin"></span>') : '<span class="ddot off"></span>';
  const doing = s.doing && s.status !== "ended" ? `<div class="row" style="padding:8px 12px 0;color:var(--muted)">${esc(s.doing)}</div>` : "";
  const question = q ? `<div class="q"><b style="font-weight:600">${esc(q.text)}</b>${q.detail ? `<code style="font-family:var(--mono);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere">${esc(q.detail)}</code>` : ""}
      <div class="dups">${q.options.map((o, i) => `<button class="dbtn${o.allow && i === q.options.findIndex(x => x.allow) ? " pri" : !o.allow && i === q.options.findIndex(x => !x.allow) ? " warn" : ""}" data-agopt="${i}"${q.id === AG.answering ? " disabled" : ""}>${esc(o.name)}${o.allow && i === q.options.findIndex(x => x.allow) ? `<kbd>⌘${ICON.ret}</kbd>` : !o.allow && i === q.options.findIndex(x => !x.allow) ? "<kbd>⌘D</kbd>" : ""}</button>`).join("")}</div></div>` : "";
  const plan = p.plan.length ? `<div class="sec">Plan</div><div class="plan">${p.plan.map(x => `<div class="row"><span style="width:16px;display:grid;place-items:center">${mark(x.status)}</span><span style="${x.status === "pending" ? "color:var(--muted)" : ""}">${esc(x.content)}</span></div>`).join("")}</div>` : "";
  const files = p.files.length ? `<div class="sec">Files changed</div><div class="files">${p.files.slice(-12).map(f => `<span><b>${esc(f)}</b></span>`).join("")}${p.files.length > 12 ? `<span style="color:var(--muted)">and ${p.files.length - 12} more</span>` : ""}</div>` : "";
  const queued = p.queued ? `<div class="row" style="padding:8px 12px 0;color:var(--muted)">${p.queued === 1 ? "1 message waits" : `${p.queued} messages wait`} for the turn to end</div>` : "";
  const card = doing || question || plan || files || queued
    ? `<div class="dcard dcoder">${doing}${question}${plan}${files}${queued}<div style="height:12px"></div></div>` : "";
  const why = AG.why ? `<div class="dmuted">Auto picked ${esc(s.machine_name)}. ${esc(AG.why)}</div>` : "";
  const earlier = p.earlier ? `<div class="dmuted">${p.earlier} earlier ${p.earlier === 1 ? "entry" : "entries"} not shown</div>` : "";
  if (!AG.log) agentLog().then(() => { if (S.view === "agent") drawAgentBody(); }).catch(console.warn);
  const entries = p.entries.length ? `<div class="dcoderlog">${earlier}${p.entries.map((en, i) => entryRow(en, p.earlier + i)).join("")}</div>`
    : s.status === "starting" ? '<div class="dmuted"><span class="dspin"></span> Starting the agent…</div>' : "";
  const err = p.error ? `<div class="derr">${esc(p.error)}</div>` : "";
  const note = AG.note === "gone" ? '<div class="dmuted">This session is no longer on the machine, or the machine went offline.</div>'
    : AG.note && AG.note === AG.readError && AG.note.startsWith("Reconnecting") ? `<div class="dmuted">${esc(AG.note)}</div>`
    : AG.note ? `<div class="derr">${esc(AG.note)}</div>` : "";
  const paused = AG.paused ? '<button class="dbtn quiet" id="agresume">Paused, click to refresh</button>' : "";
  b.innerHTML = why + card + entries + err + note + paused;
  fillAgentPics(b);
  const hint = bar.querySelector(":scope > .dhints");
  if (hint) hint.replaceWith(h(agentHints()));
  const field = $("#agfield");
  // An ended session goes on from the field: the next message resumes it.
  const msg = $("#agmsg"), k = agentKind(s.agent);
  if (msg) msg.placeholder = s.status === "ended" ? `Message ${k.name} to continue` : `Message ${k.name}`;
  const close = $("#agclose");
  if (close) close.style.display = s.status === "ended" ? "none" : "";
  // Only a turn that runs can be stopped.
  const stop = $("#agstop");
  if (stop) stop.style.display = AGENT_ACTIVE.has(s.status) ? "" : "none";
  if (b.stuck !== false) b.scrollTop = b.scrollHeight;
}

/// Answers the open question once: its buttons stay disabled, and a poll
/// that still shows it does not bring them back, until the node moves on.
export async function answerAgentQuestion(i) {
  const o = AG.open, q = AG.page?.session.question, opt = q?.options[i];
  if (!opt || AG.answering === q.id) return;
  AG.answering = q.id;
  drawAgentBody();
  try {
    await invoke("center_answer_agent", { machineId: o.machine_id, agent: o.agent, session: o.session, question: q.id, reply: opt.id });
    if (isOpen(o) && AG.page.session.question?.id === q.id) {
      AG.page.session.question = null;
      AG.page.session.status = "working";
    }
  } catch (e) {
    // A conflict means it was answered already, so it stays answered.
    const conflict = /^conflict/i.test(String(e));
    if (!conflict && AG.answering === q.id) AG.answering = null;
    if (isOpen(o)) AG.note = conflict ? "" : String(e);
  }
  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
}
/// The first option of the open question that allows, or that does not.
export function answerFirst(allow) {
  const q = AG.page?.session.question;
  const i = q ? q.options.findIndex(o => o.allow === allow) : -1;
  if (i >= 0) answerAgentQuestion(i);
  return i >= 0;
}

/// Sends `steer` to the open session; true when the node took it.
export async function steerAgent(steer) {
  const o = AG.open;
  if (!o || AG.page?.session.external) return false;
  let ok = false;
  try {
    await invoke("agent_steer", { machineId: o.machine_id, agent: o.agent, session: o.session, steer: "prompt" in steer ? steer : Object.keys(steer)[0] });
    AG.note = "";
    ok = true;
  } catch (e) {
    AG.note = String(e).replace(/^bad request: /, "");
  }
  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
  return ok;
}

export async function sendAgentMessage() {
  const input = $("#agmsg"), text = input?.value.trim() ?? "", images = AG.att;
  if (!text && !images.length) return;
  const o = AG.open;
  if (AG.page.session.status === "ended") return continueAgent(input, text, images);
  input.value = "";
  if (!images.length) {
    AG.page.entries.push({ kind: "user", text });
    drawAgentBody();
    return steerAgent({ prompt: { text } });
  }
  AG.att = [];
  drawAgentAttached();
  keepAgentPics(o, images);
  const page = AG.page, mine = { kind: "user", text: [text, ...images.map(a => `[image: ${a.name}]`)].filter(Boolean).join("\n") };
  page.entries.push(mine);
  drawAgentBody();
  const sent = await steerAgent({ prompt: { text, images: images.map(({ name, mime, data }) => ({ name, mime, data })) } });
  if (sent || !isOpen(o)) return;
  // Nothing went: the message and its images wait in the field again.
  if (/does not take images yet/.test(AG.note)) AG.noImages = o.session;
  // A poll may have drawn more since: take out this message, not the last.
  const at = page.entries.indexOf(mine);
  if (at >= 0) page.entries.splice(at, 1);
  const field = $("#agmsg");
  if (field && !field.value) field.value = text;
  AG.att = images;
  drawAgentBody();
  drawAgentAttached();
}

/// Resumes the ended session with `text`, then follows the session that
/// goes on from it. Images go once it runs again.
export async function continueAgent(input, text, images) {
  const o = AG.open;
  if (images.length && !text) { AG.note = "Say something to continue the session, then send the images."; return drawAgentBody(); }
  input.value = "";
  input.disabled = true;
  AG.note = "";
  AG.page.entries.push({ kind: "user", text });
  drawAgentBody();
  try {
    const started = await invoke("agent_resume", { machineId: o.machine_id, agent: o.agent, session: o.session, prompt: text });
    if (!isOpen(o)) return;
    const back = AG.back;
    openAgent(started.session, { back });
    if (images.length) { AG.att = images; drawAgentAttached(); }
  } catch (e) {
    if (!isOpen(o)) return;
    AG.page.entries.pop();
    input.disabled = false;
    input.value = text;
    AG.note = String(e).replace(/^(bad request|error):\s*/i, "");
    drawAgentBody();
  }
}

// ----- Every session -----

export async function renderAgents() {
  S.view = "agents";
  AG.sel = 0;
  drawAgents(true);
  try {
    AG.sessions = await invoke("agent_sessions");
    AG.error = "";
  } catch (e) {
    AG.error = String(e);
  }
  if (S.view === "agents") drawAgents(false);
}

export function drawAgents(loading) {
  const list = AG.sessions;
  const machines = new Set(list.map(s => s.machine_id)).size;
  const sum = loading && !list.length ? "Looking at your machines…" : list.length ? `${list.length} on ${machines} ${machines === 1 ? "machine" : "machines"}` : "Nothing running";
  const row = (s, i) => {
    const k = agentKind(s.agent), where = folderName(s.cwd);
    const tail = s.status === "asking" ? '<span class="dstate ask">Needs you</span>'
      : s.status === "working" || s.status === "running" || s.status === "starting" ? `<span class="run"><span class="dspin"></span>${esc(s.doing || "Working")}</span>`
      : `<span class="w">${s.external ? "In terminal" : esc(cap(s.status))}</span>`;
    return `<button class="dchat" role="option" aria-selected="${i === AG.sel}" data-agi="${i}">${agentAvatar(s.agent)}
      <span class="t"><span>${esc(s.title || where || k.name)}</span><small>${esc(`${where ? where + ", " : ""}${k.name} on ${s.machine_name}`)}</small></span>${tail}</button>`;
  };
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Coding agents</b><small>${esc(sum)}</small></span><button class="dbtn" id="agNew">${AGENT_ICON}New<kbd>⌘⇧A</kbd></button></div>
    ${AG.error ? `<div class="derr" style="padding:8px 18px 0">${esc(AG.error)}</div>` : ""}
    ${list.length ? `<div class="dlist" role="listbox" style="padding-top:6px">${list.map(row).join("")}</div>`
      : loading ? "" : `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${AGENT_ICON}</span><b style="font-size:14px">No sessions running.</b>
        <span class="dmuted" style="display:block">Start one here, ask Beep in a chat, or run Claude Code in a terminal on a linked machine.</span></div>`}
    ${hints([[ICON.ret, "open"], ["⌘⇧A", "new"], ["esc", "back"]])}`;
  bar.onclick = e => {
    if (S.view !== "agents") return;
    const r = e.target.closest("[data-agi]");
    if (r) return openAgent(AG.sessions[Number(r.dataset.agi)], { back: "agents" });
    if (e.target.closest("#agNew")) renderAgentNew("", "agents");
  };
}

/// Keys of the agent views; true when one was taken.
export function agentKeys(e, cmd) {
  if (S.view === "agentnew") {
    if (cmd && e.key === "Enter") { e.preventDefault(); startAgent(); return true; }
    return false;
  }
  if (S.view === "agent") {
    if (cmd && e.key === "Enter" && answerFirst(true)) { e.preventDefault(); return true; }
    if (cmd && e.key.toLowerCase() === "d" && answerFirst(false)) { e.preventDefault(); return true; }
    if (cmd && e.key === ".") { e.preventDefault(); steerAgent({ cancel: null }); return true; }
    if (e.key === "Enter" && !e.isComposing && e.target.id === "agmsg") { e.preventDefault(); sendAgentMessage(); return true; }
    return false;
  }
  if (S.view === "agents") {
    const n = AG.sessions.length;
    if (!n) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      AG.sel = (AG.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      drawAgents(false);
      return true;
    }
    if (e.key === "Enter") { e.preventDefault(); openAgent(AG.sessions[AG.sel], { back: "agents" }); return true; }
  }
  return false;
}

/// Esc from an agent view.
export function agentBack() {
  if (AG.back?.view && windowHooks) return windowHooks.goBack(AG.back);
  if (S.view === "agent" && AG.back === "agents") return renderAgents();
  if (S.view === "agentnew" && AG.back === "agents") return renderAgents();
  return S.session ? openSession(S.session) : renderChat();
}

