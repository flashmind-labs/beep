// An opened research, as a page of the command center laid out like a coding
// agent session: the lead's chat on the left, the paper, notes, sources and the
// team as the tabs of a side pane on the right. The page is drawn once and
// each update patches only what changed, so spinners keep turning.
import { copyCode } from "../agents/turnrows.ts";
import { atBottom } from "../agents/logscroll.ts";
import { onZoomKey, paneSize } from "../agentpane/media-state.ts";
import { composeField, composeMessage } from "../chat/compose.ts";
import { reloadGroup } from "../center/sidebar.ts";
import type { ResearchArg } from "../center/state.ts";
import { registerView, shown } from "../center/views.ts";
import { h } from "../ui/dom.ts";
import { esc } from "../ui/format.ts";
import { writeClipboard } from "../ui/parts.ts";
import { patchHtml } from "../ui/patch.ts";
import { Channel, invoke } from "../ui/tauri.ts";
import { chatHtml, followsEnd, headHtml, paneHtml, placeholder, tabsHtml } from "./panes.ts";
import { type Open, type ResearchView, type Turn, RUNNING, closeTab, openTab, round, shownTab } from "./research-state.ts";
import { dragHandle } from "../ui/drag.ts";

/// What streams in while the lead replies (Rust's `BarEvent`), as far as this reads it.
type SendEvent =
  | { kind: "text"; html: string }
  | { kind: "error"; message: string }
  | { kind: "started" | "reasoning" | "tool_started" | "tool_result" | "sources" | "replies" | "approval" | "auto_picked" | "transcript" | "finished" };

/// How often the page asks for news while the team works, and once it
/// has finished (a follow-up from another device can start a new round).
const POLL_BUSY = 2500, POLL_IDLE = 30000;
/// While the team works the lead's chat rarely changes, so it is read on
/// every few polls, and whenever the research changes phase.
const CHAT_EVERY = 4;
/// Where the side pane's width is kept.
const WIDTH_KEY = "research.paneWidth";

/// The parts of the page, once built.
interface Page {
  root: HTMLElement;
  head: HTMLElement;
  shell: HTMLElement;
  log: HTMLElement;
  chat: HTMLElement;
  ask: HTMLTextAreaElement;
  pane: HTMLElement;
  tabs: HTMLElement;
  view: HTMLElement;
}

const fresh = (session: string): Open => ({
  session, research: null, entries: [], after: 0, turns: [], tabs: [], tab: null, pick: null, menu: false, sending: null, error: "", exporting: false,
});

let R = fresh("");
let page: Page | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
let inflight = false;
let polls = 0;
/// The width the user dragged the side pane to.
let paneWidth: number | null = null;
try { paneWidth = Number(localStorage.getItem(WIDTH_KEY)) || null; } catch { /* kept while the window is open */ }

const ignore = () => {};
const active = () => shown === "researchopen" && !!page?.root.isConnected;
/// The side pane is zoomed by ⇧esc. Not kept: another research starts unzoomed.
let zoomed = false;
let watch: ResizeObserver | null = null;

registerView("researchopen", {
  title: () => ({ title: "Research" }),
  mount(el, arg) {
    const { session } = arg as ResearchArg;
    if (R.session !== session || !page || page.root.parentElement !== el) build(el, session);
    if (!inflight) void poll(true);
    page!.ask.focus();
  },
  unmount() {
    clearTimeout(timer);
    document.getElementById("rquote")?.remove();
  },
});

// ---------- The page ----------

function build(el: HTMLElement, session: string) {
  zoomed = false;
  R = fresh(session);
  polls = 0;
  const root = h(`<div class="rs"><div class="dhead"></div>
    <div class="agshell"><div class="agconv"><div class="dbody rs-log"><div class="dagmain"><div class="dcoderlog"></div></div></div>
      <div class="dinput small"><textarea rows="1" autocomplete="off" spellcheck="false" aria-label="Message the lead"></textarea></div></div>
      <div class="agdiv" role="separator" aria-orientation="vertical" tabindex="0" title="Drag to resize, double-click to reset"></div>
      <aside class="agpane rs-pane"><div class="agphead"><div class="agtabs" role="tablist"></div></div><div class="agpviews"></div></aside></div></div>`);
  el.replaceChildren(root);
  const q = <T extends HTMLElement>(sel: string) => root.querySelector<T>(sel)!;
  page = { root, head: q(".dhead"), shell: q(".agshell"), log: q(".rs-log"), chat: q(".dcoderlog"), ask: q("textarea"), pane: q(".rs-pane"), tabs: q(".agtabs"), view: q(".agpviews") };
  composeField(page.ask, { grow: 156 });
  root.addEventListener("click", onClick);
  page.ask.addEventListener("keydown", onAskKey);
  for (const part of [page.log, page.view]) part.addEventListener("scroll", () => document.getElementById("rquote")?.remove(), true);
  wireDivider(page);
  watch?.disconnect();
  watch = new ResizeObserver(place);
  watch.observe(page.shell);
  place();
  void start();
}

/// What this Mac already holds draws at once; the poll adds what is new.
async function start() {
  const session = R.session;
  const cached = await invoke<ResearchView | null>("cached_research", { session }).catch(() => null);
  if (R.session !== session) return;
  if (cached) take(cached);
  draw();
  await loadChat();
  const topic = await invoke<string | null>("research_pending", { session }).catch(() => null);
  if (R.session === session && topic && !R.turns.length) void send(topic, true);
}

const place = () => {
  if (!page) return;
  page.pane.style.flexBasis = `${paneSize("right", paneWidth, page.shell.clientWidth)}px`;
  page.shell.classList.toggle("zoomed", zoomed);
  page.pane.classList.toggle("zoomed", zoomed);
};

// ⇧esc: the side pane fills the page over the chat, as in a session; only classes
// change, so nothing in it reloads. In the capture phase so no Esc handler sees it.
onZoomKey(active, () => { zoomed = !zoomed; place(); });

/// The grip between the chat and the pane: drag to resize, double-click to reset.
function wireDivider(p: Page) {
  const div = p.shell.querySelector<HTMLElement>(".agdiv")!;
  const set = (width: number | null) => { paneWidth = width == null ? null : paneSize("right", width, p.shell.clientWidth); place(); };
  const keep = () => { try { if (paneWidth) localStorage.setItem(WIDTH_KEY, String(paneWidth)); else localStorage.removeItem(WIDTH_KEY); } catch { /* kept while the window is open */ } };
  dragHandle(div, () => {
    p.shell.classList.add("resizing");
    div.classList.add("active");
    return {
      move: ev => set(p.shell.getBoundingClientRect().right - ev.clientX),
      end: () => {
        p.shell.classList.remove("resizing");
        div.classList.remove("active");
        keep();
      },
    };
  });
  div.addEventListener("dblclick", () => { set(null); keep(); });
}

// ---------- Drawing ----------

function drawHead() {
  if (page) patchHtml(page.head, headHtml(R));
}

function drawChat() {
  if (!page) return;
  const stuck = atBottom(page.log);
  patchHtml(page.chat, chatHtml(R));
  if (stuck) page.log.scrollTop = page.log.scrollHeight;
  page.ask.placeholder = placeholder(R);
}

function drawPane() {
  if (!page) return;
  const tab = shownTab(R), same = page.view.dataset.tab === tab;
  page.view.dataset.tab = tab;
  patchHtml(page.tabs, tabsHtml(R));
  const before = page.view.querySelector<HTMLElement>(".rpane");
  const stuck = !!before && atBottom(before);
  patchHtml(page.view, paneHtml(R));
  const body = page.view.querySelector<HTMLElement>(".rpane");
  if (!body) return;
  // A thread or the room follows its newest line; a tab just opened starts at its top.
  if (!same) body.scrollTop = followsEnd(tab) ? body.scrollHeight : 0;
  else if (followsEnd(tab) && stuck) body.scrollTop = body.scrollHeight;
}

function draw() {
  drawHead();
  drawChat();
  drawPane();
}

// ---------- The lead's chat ----------

async function loadChat() {
  const session = R.session;
  const turns = await invoke<Turn[]>("research_history", { session }).catch(() => null);
  if (!turns || R.session !== session || R.sending !== null) return;
  if (JSON.stringify(turns) === JSON.stringify(R.turns)) return;
  R.turns = turns;
  drawChat();
}

/// Sends `text` to the lead; `start` makes the chat a research first.
async function send(text: string, start = false) {
  text = text.trim();
  if (!text || R.sending !== null) return;
  const session = R.session;
  R.error = "";
  R.turns.push({ role: "user", text });
  R.sending = "";
  drawHead();
  drawChat();
  const channel = new Channel<SendEvent>();
  channel.onmessage = ev => {
    if (R.session !== session) return;
    if (ev.kind === "text") { R.sending = ev.html; drawChat(); }
    else if (ev.kind === "error") R.error = ev.message;
  };
  let error = "";
  try {
    await invoke("research_send", { session, message: text, start, onEvent: channel });
  } catch (err) {
    error = String(err);
  }
  if (R.session !== session) return;
  if (error) R.error = error;
  R.sending = null;
  await loadChat();
  draw();
  void poll(true);
}

// ---------- Polling ----------

/// Takes a research from Rust: the entries are only those after `R.after`.
function take(r: ResearchView) {
  const { entries, ...research } = r;
  // Opening reads the list while a poll may be reading it too: keep only what is new.
  const added = entries.filter(e => e.seq > R.after);
  R.entries = [...R.entries, ...added];
  if (added.length) R.after = added.at(-1)!.seq;
  R.research = research;
}

const busy = () => !R.research || R.research.status === "planning" || RUNNING.has(R.research.status);

async function poll(now = false) {
  clearTimeout(timer);
  const session = R.session;
  inflight = true;
  try {
    if (!now && (!busy() || ++polls % CHAT_EVERY === 0)) await loadChat();
    const r = await invoke<ResearchView | null>("get_research", { session, after: R.after });
    if (r && R.session === session) {
      const was = R.research?.status;
      take(r);
      if (was && was !== r.status) { await loadChat(); reloadGroup("research"); }
      draw();
    }
  } catch (err) {
    console.warn("research poll", err);
  } finally {
    inflight = false;
  }
  // Whichever research is open now keeps polling, also when this read was for another.
  clearTimeout(timer);
  if (active()) timer = setTimeout(poll, busy() ? POLL_BUSY : POLL_IDLE);
}

// ---------- Input ----------

function onAskKey(e: KeyboardEvent) {
  if (e.key !== "Enter" || e.shiftKey || e.isComposing) return;
  e.preventDefault();
  // The lead is still answering: keep what was typed for after.
  if (R.sending !== null) return;
  const ask = page!.ask;
  const text = composeMessage(ask.value);
  ask.value = "";
  void send(text);
}

function onClick(e: MouseEvent) {
  const target = e.target as Element;
  const b = target.closest<HTMLElement>("[data-act]");
  const name = b?.dataset.act;
  if (R.menu && name !== "rounds" && !name?.startsWith("round:")) { R.menu = false; drawPane(); }
  if (target.closest("[data-copy]")) return copyCode(target.closest<HTMLElement>("[data-copy]")!);
  if (b) {
    const [a, v] = b.dataset.act!.split(/:(.*)/);
    return void act(a, v);
  }
  const link = target.closest<HTMLAnchorElement>("a[href]");
  if (!link) return;
  e.preventDefault();
  invoke("open_url", { url: link.href }).catch(ignore);
}

async function act(a: string, v: string | undefined) {
  if (a === "go") return void send("Go");
  if (a === "stop") {
    await invoke("stop_research", { session: R.session }).catch(err => { R.error = String(err); });
    return void poll(true);
  }
  if (a === "tab") { R.tab = v!; return drawPane(); }
  if (a === "agent") { if (R.research?.status === "planning" && v !== "lead") return; openTab(R, v!); return drawPane(); }
  if (a === "room") { if (!R.research || R.research.status === "planning") return; openTab(R, "room"); return drawPane(); }
  if (a === "close") { closeTab(R, v!); return drawPane(); }
  if (a === "rounds") { R.menu = !R.menu; return drawPane(); }
  if (a === "round") { R.pick = Number(v) === round(R) ? null : Number(v); R.menu = false; return drawPane(); }
  if (a === "copy") return copy(R.research?.draft ?? "");
  if (a === "export") return exportPdf();
}

async function exportPdf() {
  if (R.exporting) return;
  const session = R.session;
  R.exporting = true;
  notice("Typesetting the PDF", { busy: true });
  draw();
  try {
    const path = await invoke<string>("export_research", { session });
    notice(`Saved ${path.split("/").pop()} to Downloads`, { path });
  } catch (err) {
    notice(String(err), { bad: true });
  }
  R.exporting = false;
  if (R.session === session) draw();
}

/// A short note over the pane; one that names a saved file can show it in
/// Finder. It goes after a while, unless it is waiting on something.
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
function notice(text: string, { path, bad, busy }: { path?: string; bad?: boolean; busy?: boolean } = {}) {
  clearTimeout(noticeTimer);
  page?.pane.querySelector(".dsaved")?.remove();
  const el = h(`<div class="dsaved${bad ? " bad" : ""}" role="status">${busy ? '<span class="dspin"></span>' : ""}<span>${esc(text)}</span>${path ? '<button class="dbtn">Show in Finder</button>' : ""}</div>`);
  el.querySelector("button")?.addEventListener("click", () => invoke("reveal_file", { path }).catch(ignore));
  page?.pane.append(el);
  if (!busy) noticeTimer = setTimeout(() => el.remove(), bad ? 10000 : 6000);
}

async function copy(text: string) {
  await writeClipboard(text);
  notice("Copied the paper as Markdown");
}

// ---------- Quoting ----------

/// The selected text in the pane or the chat, if any.
function selectedText() {
  const sel = getSelection();
  if (!active() || !sel || sel.isCollapsed || !sel.rangeCount) return "";
  const node = sel.getRangeAt(0).commonAncestorContainer;
  const el = node.nodeType === 1 ? node as Element : node.parentElement;
  if (!el?.closest(".rs-pane .rpane, .rs-log") || el.closest(".dinput")) return "";
  return sel.toString().trim();
}

/// Shows the Quote button over the selection, or hides it.
function placeQuote() {
  document.getElementById("rquote")?.remove();
  const text = selectedText();
  if (!text) return;
  const rect = getSelection()!.getRangeAt(0).getBoundingClientRect();
  const b = h('<button id="rquote" class="dbtn pri rquote">Quote<kbd>⌘\'</kbd></button>');
  document.body.append(b);
  const w = b.offsetWidth, height = b.offsetHeight;
  const left = Math.max(8, Math.min(innerWidth - w - 8, rect.left + rect.width / 2 - w / 2));
  const top = rect.top - height - 8 < 46 ? rect.bottom + 8 : rect.top - height - 8;
  b.style.left = `${left}px`;
  b.style.top = `${top}px`;
  b.addEventListener("click", quote);
}

/// Adds the selection to the message as a quote and puts the cursor under
/// it, so several quotes with replies make one message.
function quote() {
  const text = selectedText();
  document.getElementById("rquote")?.remove();
  if (!text || !page) return;
  const ask = page.ask;
  const quoted = text.split(/\n+/).map(line => `> ${line.trim()}`).filter(line => line !== ">").join("\n");
  const before = ask.value.replace(/\s+$/, "");
  ask.value = `${before ? before + "\n\n" : ""}${quoted}\n\n`;
  getSelection()!.removeAllRanges();
  ask.focus();
  ask.setSelectionRange(ask.value.length, ask.value.length);
  ask.dispatchEvent(new Event("input"));
}

// Pressing the button must not clear the selection it quotes.
document.addEventListener("mousedown", e => { if ((e.target as Element).closest("#rquote")) e.preventDefault(); });
document.addEventListener("mouseup", () => setTimeout(placeQuote));
document.addEventListener("selectionchange", () => { if (!selectedText()) document.getElementById("rquote")?.remove(); });

document.addEventListener("keydown", e => {
  if (!active()) return;
  if (e.metaKey && e.key === "Enter" && R.research?.status === "planning" && R.sending === null) {
    e.preventDefault();
    return void send("Go");
  }
  if (e.key === "Escape" && R.menu) { R.menu = false; return drawPane(); }
  if (e.metaKey && e.key === "'" && selectedText()) { e.preventDefault(); quote(); }
});
