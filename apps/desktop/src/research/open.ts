import { commandKey, savesName } from "../ui/platform.ts";
import { createElement, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactElement } from "react";
import { ResearchHead } from "./head-view.tsx";
import { ResearchTabs } from "./tabs-view.tsx";
import { ResearchLivePane } from "./live-pane-view.tsx";
import { ResearchChat, ResearchPaper } from "./chat-paper-view.tsx";
import { ResearchAgents, ResearchThread } from "./agents-pane-view.tsx";
import { ResearchShell, type ShellParts } from "./shell-view.tsx";
import { createDocumentScope } from "../ui/document-root.tsx";
import { ResearchNotice, ResearchQuote } from "./open-controls.tsx";
// An opened research, as a page of the command center laid out like a coding
// agent session: the lead's chat on the left, the paper, notes, sources and the
// team as the tabs of a side pane on the right. The page is drawn once and
// each update patches only what changed, so spinners keep turning.
import { copyCode } from "../agents/turnrows.ts";
import { atBottom, followsAfterScroll } from "../agents/logscroll.ts";
import { followEnd } from "../ui/followend.ts";
import { onZoomKey, paneSize } from "../agentpane/media-state.ts";
import { composeField, composeMessage } from "../chat/compose.ts";
import { reloadGroup } from "../center/sidebar.ts";
import { currentLocation, drawPage } from "../center/views.ts";
import type { CenterView } from "../center/state.ts";
import { writeClipboard } from "../ui/parts.ts";
import { Channel, invoke } from "../ui/tauri.ts";
import { type Open, type ResearchAct, type ResearchView, type Turn, RUNNING, agentOf, closeTab, followsEnd, openTab, placeholder, round, shownTab } from "./research-state.ts";
import { dragHandle } from "../ui/drag.ts";
import { onShownChange, pageShown } from "../ui/active.ts";

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

const fresh = (session: string): Open => ({
  session, research: null, entries: [], after: 0, turns: [], tabs: [], tab: null, pick: null, menu: false, sending: null, error: "", exporting: false,
});

let R = fresh("");
let page: ShellParts | null = null;
/// Counts the pages built, so a new research draws a new shell.
let built = 0;
let headNode: ReactElement | null = null, tabsNode: ReactElement | null = null, paneNode: ReactElement | null = null, chatNode: ReactElement | null = null;
let noticeNode: ReactElement | null = null;
/// The tab the pane last drew, so a newly opened one starts at its top.
let paneTab: string | null = null;
/// The side pane while its grip is dragged, and its width as drawn.
let resizing = false, paneBasis: number | null = null;
/// Where the reader left the lead's chat, to tell scrolling up from it growing.
let logTop = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let inflight = false;
let polls = 0;
/// The width the user dragged the side pane to.
let paneWidth: number | null = null;
try { paneWidth = Number(localStorage.getItem(WIDTH_KEY)) || null; } catch { /* kept while the window is open */ }

const ignore = () => {};
const active = () => currentLocation()?.view === "researchopen" && !!page?.root.isConnected;
/// The side pane is zoomed by ⇧esc. Not kept: another research starts unzoomed.
let zoomed = false;
let watch: ResizeObserver | null = null;

export const researchOpenView: CenterView<"researchopen"> = {
  title: () => ({ title: "Research" }),
  mount(el, arg) {
    const { session } = arg;
    if (R.session !== session || !page || page.root.parentElement !== el) build(session);
    if (!inflight) void poll(true);
    page?.ask.focus();
  },
  unmount() {
    clearTimeout(timer);
    hideQuote();
  },
};

// ---------- The page ----------

function build(session: string) {
  zoomed = resizing = false;
  headNode = tabsNode = paneNode = chatNode = noticeNode = null;
  paneTab = null;
  logTop = 0;
  R = fresh(session);
  polls = 0;
  built++;
  drawPage("researchopen");
  if (!page) return;
  const parts = page;
  composeField(parts.ask, { grow: 156 });
  // The lead's chat follows its end until the reader scrolls up, and stays there as it grows.
  followEnd(parts.log, parts.chat);
  wireDivider(parts);
  watch?.disconnect();
  watch = new ResizeObserver(place);
  watch.observe(parts.shell);
  place();
  void start();
}

/// The shell hands over its elements as it mounts, and null as it goes.
function bind(parts: ShellParts | null) { page = parts; }

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
  const basis = paneSize("right", paneWidth, page.shell.clientWidth);
  if (basis === paneBasis) return;
  paneBasis = basis;
  drawShell();
};

// ⇧esc: the side pane fills the page over the chat, as in a session; only classes
// change, so nothing in it reloads. In the capture phase so no Esc handler sees it.
onZoomKey(active, () => { zoomed = !zoomed; drawShell(); });

/// The grip between the chat and the pane: drag to resize, double-click to reset.
/// While it moves only a CSS variable on the pane changes; the page draws once when it ends.
function wireDivider(p: ShellParts) {
  dragHandle(p.divider, () => {
    resizing = true;
    drawShell();
    const right = p.shell.getBoundingClientRect().right, total = p.shell.clientWidth;
    let width: number | null = null;
    return {
      move: ev => {
        width = paneSize("right", right - ev.clientX, total);
        p.pane.style.setProperty("--drag-size", `${width}px`);
      },
      end: () => {
        resizing = false;
        if (width !== null) paneWidth = paneBasis = width;
        drawShell();
        p.pane.style.removeProperty("--drag-size");
        keepPaneWidth();
      },
    };
  });
}

function setPaneWidth(width: number | null) {
  paneWidth = width === null || !page ? null : paneSize("right", width, page.shell.clientWidth);
  place();
}

function keepPaneWidth() {
  try { if (paneWidth) localStorage.setItem(WIDTH_KEY, String(paneWidth)); else localStorage.removeItem(WIDTH_KEY); } catch { /* kept while the window is open */ }
}

// ---------- Drawing ----------

function drawShell() {
  if (page) drawPage("researchopen");
}

/// The opened research, with what each part last drew.
export function ResearchOpenPage() {
  return createElement(ResearchShell, { key: built, head: headNode, chat: chatNode, tabs: tabsNode, pane: paneNode, notice: noticeNode,
    placeholder: placeholder(R), zoomed, resizing, paneBasis, act: run, bind, click, askKey, logScrolled, scrolled: hideQuote,
    resetPane: () => { setPaneWidth(null); keepPaneWidth(); } });
}

function logScrolled() {
  const log = page?.log;
  if (!log) return;
  log.stuck = followsAfterScroll(log.stuck !== false, logTop, log);
  logTop = log.scrollTop;
}

function drawHead() {
  headNode = createElement(ResearchHead, { research: R.research, entries: R.entries, sending: R.sending });
  drawShell();
}

function drawChat() {
  if (!page) return;
  const log = page.log;
  chatNode = createElement(ResearchChat, { research: R });
  drawShell();
  if (log.stuck !== false) log.scrollTop = log.scrollHeight;
}

function drawPane() {
  if (!page) return;
  const tab = shownTab(R), same = paneTab === tab;
  paneTab = tab;
  tabsNode = createElement(ResearchTabs, { research: R });
  const before = page.views.querySelector<HTMLElement>(".rpane");
  const stuck = !!before && atBottom(before);
  paneNode = tab === "draft" ? createElement(ResearchPaper, { research: R })
    : tab === "agents" ? createElement(ResearchAgents, { research: R })
    : tab === "sources" || tab === "notes" || tab === "room" ? createElement(ResearchLivePane, { research: R, tab })
    : createElement(ResearchThread, { research: R, agent: agentOf(R, tab) });
  drawShell();
  const body = page.views.querySelector<HTMLElement>(".rpane");
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
  if (page) page.log.stuck = true;
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
  if (active() && pageShown()) timer = setTimeout(poll, busy() ? POLL_BUSY : POLL_IDLE);
}

// A hidden window stops polling; on return one read catches up.
onShownChange(shown => {
  clearTimeout(timer);
  if (shown && active() && !inflight) void poll();
});

// ---------- Input ----------

function askKey(e: ReactKeyboardEvent<HTMLTextAreaElement>) {
  if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
  e.preventDefault();
  // The lead is still answering: keep what was typed for after.
  if (R.sending !== null) return;
  const ask = e.currentTarget;
  const text = composeMessage(ask.value);
  ask.value = "";
  void send(text);
}

/// A click anywhere closes the rounds menu. The reply's sanitized markup brings
/// its own code copy buttons and links, which open in the browser.
function click(e: ReactMouseEvent<HTMLElement>) {
  if (R.menu) { R.menu = false; drawPane(); }
  const target = e.target;
  if (!(target instanceof Element)) return;
  const code = target.closest<HTMLElement>("[data-copy]");
  if (code) return copyCode(code);

  const link = target.closest<HTMLAnchorElement>("a[href]");
  if (!link) return;
  e.preventDefault();
  invoke("open_url", { url: link.href }).catch(ignore);
}

function run(a: ResearchAct) {
  switch (a.kind) {
    case "go": return void send("Go");
    case "stop": return void stop();
    case "tab": R.tab = a.tab; return drawPane();
    case "agent": if (R.research?.status === "planning" && a.agent !== "lead") return; openTab(R, a.agent); return drawPane();
    case "room": if (!R.research || R.research.status === "planning") return; openTab(R, "room"); return drawPane();
    case "close": closeTab(R, a.tab); return drawPane();
    case "rounds": R.menu = !R.menu; return drawPane();
    case "round": R.pick = a.round === round(R) ? null : a.round; R.menu = false; return drawPane();
    case "copy": return void copy(R.research?.draft ?? "");
    case "export": return void exportPdf();
  }
}

async function stop() {
  await invoke("stop_research", { session: R.session }).catch(err => { R.error = String(err); });
  void poll(true);
}

async function exportPdf() {
  if (R.exporting) return;
  const session = R.session;
  R.exporting = true;
  notice("Typesetting the PDF", { busy: true });
  draw();

  try {
    const path = await invoke<string>("export_research", { session });
    notice(`Saved ${path.split("/").pop()} to ${savesName()}`, { path });
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
  if (!page) return;
  noticeNode = createElement(ResearchNotice, {
    text, path: path ?? null, bad: !!bad, busy: !!busy, reveal: value => { void invoke("reveal_file", { path: value }).catch(ignore); },
  });
  drawShell();
  if (!busy) noticeTimer = setTimeout(() => { noticeNode = null; drawShell(); }, bad ? 10000 : 6000);
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
const quoteScope = createDocumentScope<"quote">();
function hideQuote() { quoteScope.remove(["quote"]); }
function placeQuote() {
  const text = selectedText();
  if (!text) { hideQuote(); return; }
  const rect = getSelection()!.getRangeAt(0).getBoundingClientRect();
  quoteScope.set([{ slot: "quote", host: document.body, content: createElement(ResearchQuote, { rect, quote }) }]);
}

/// Adds the selection to the message as a quote and puts the cursor under
/// it, so several quotes with replies make one message.
function quote() {
  const text = selectedText();
  hideQuote();
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

document.addEventListener("mouseup", () => setTimeout(placeQuote));
document.addEventListener("selectionchange", () => { if (!selectedText()) hideQuote(); });

document.addEventListener("keydown", e => {
  if (!active()) return;

  if (commandKey(e) && e.key === "Enter" && R.research?.status === "planning" && R.sending === null) {
    e.preventDefault();
    return void send("Go");
  }

  if (e.key === "Escape" && R.menu) { R.menu = false; return drawPane(); }
  if (commandKey(e) && e.key === "'" && selectedText()) { e.preventDefault(); quote(); }
});
