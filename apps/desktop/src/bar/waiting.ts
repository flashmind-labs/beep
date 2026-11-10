// The bar's default view: the sessions waiting for the user, drawn from the
// sessions every page shares and the notifications this Mac holds. Return
// opens one in the bar, with its conversation and composer.
import { createElement } from "react";
import { flushSync } from "react-dom";
import type { Root } from "react-dom/client";
import { agentAvatar, type AgentSession, folderName } from "../agents/agents.ts";
import { renderAgentNew } from "../agents/agentnew.ts";
import { onPlaces, placeOf } from "../agents/place.ts";
import { placeRepo } from "../agents/place-state.ts";
import { openAgent } from "../agents/session.ts";
import { ensureSessions, sessions, sessionsRead } from "../agents/sessions.ts";
import { onClosing, sessionVisible } from "../agents/closing.ts";
import type { Note } from "../center/notices-state.ts";
import { ambient, AMBIENT_MS } from "../ui/active.ts";
import { formatAgo } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { subscribeTopics, topic } from "../ui/mesh-events.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { $, bar, PANE, S } from "./core.ts";
import { hintModel } from "./hints.ts";
import { openBarPage } from "./page-root.ts";
import { sessionKey, waitingSessions, type WaitingSession } from "./waiting-state.ts";
import { WaitingList, type WaitingRow } from "./waiting-view.tsx";

const W = { sel: 0, query: "", rows: [] as WaitingSession[] };
/// Sessions with an unread finished turn, by `sessionKey`.
let unread = new Set<string>();
/// Whether the shared sessions were read once, so an empty list means none.
let known = false;
let page: { host: HTMLElement; root: Root } | null = null;

/// Shows the list, or the new agent form when nothing is recent. Known
/// sessions draw at once; a read follows only when a change came since.
export function renderWaiting() {
  S.view = "waiting";
  W.sel = 0;
  W.query = "";
  if (known && !pick().length) return renderAgentNew("", "chat");
  drawWaiting(true);

  ensureSessions()
    .then(() => { if (S.view === "waiting" && !W.query && !W.rows.length) renderAgentNew("", "chat"); })
    .catch(() => { if (S.view === "waiting" && !W.rows.length) renderAgentNew("", "chat"); });
}

/// The project a session works in: its repository once its machine said, else its folder.
const project = (s: AgentSession) => {
  const place = placeOf(s, Infinity);
  return place ? placeRepo(place) : folderName(s.cwd);
};

const pick = () => waitingSessions(sessions.filter(sessionVisible), unread, W.query, project);

function row({ session: s, kind }: WaitingSession): WaitingRow {
  const title = s.title || folderName(s.cwd) || s.agent;
  return { key: sessionKey(s), avatar: agentAvatar(s.agent), title, sub: [project(s), s.machine_name, formatAgo(s.last_activity, "short")].join(" · "), kind, doing: s.doing || "Working" };
}

/// Draws the list; the list takes the focus with `focus`, or when it had it.
export function drawWaiting(focus = false) {
  if (S.view !== "waiting") return;
  W.rows = pick();
  W.sel = Math.min(W.sel, Math.max(0, W.rows.length - 1));
  const need = W.rows.filter(r => r.kind === "asking" || r.kind === "done").length;
  const summary = !known ? "Looking at your machines…" : need ? `${need} waiting for you` : "Nothing waiting";
  const active = document.activeElement;
  const focused = focus || active?.id === "wtlist" || !active || active === document.body;
  if (!page || page.host.parentElement !== bar) page = openBarPage(() => { page = null; });
  const root = page.root;
  flushSync(() => root.render(createElement(WaitingList, {
    rows: W.rows.map(row), selected: W.sel, summary, query: W.query,
    hints: hintModel([[ICON.ret, "open"], ["↑↓", "move"], ["⌘N", "new agent"], ["esc", W.query ? "clear" : "to hide"]]),
  })));
  // The command center's hidden pane draws too; WebKit would focus a list
  // it has not laid out yet and take the focus from the window's page.
  if (focused && bar.checkVisibility()) $("#wtlist")?.focus();
  $(`#wt-${W.sel}`)?.scrollIntoView({ block: "nearest" });
  bar.onclick = e => {
    if (S.view !== "waiting") return;
    const t = e.target as HTMLElement;
    const r = t.closest<HTMLElement>("[data-wti]");
    if (r) return openRow(Number(r.dataset.wti));
    if (t.closest("#wtNew")) renderAgentNew("", "waiting");
  };
}

/// Opens row `i` in the bar. Opening it reads its notifications on every device.
function openRow(i: number) {
  const r = W.rows[i];
  if (r) openAgent(r.session, { back: "waiting" });
}

/// Keys of the list; true when one was taken.
export function waitingKeys(e: KeyboardEvent, cmd: boolean) {
  if (S.view !== "waiting" || e.isComposing) return false;
  const n = W.rows.length;

  if ((e.key === "ArrowDown" || e.key === "ArrowUp") && n) {
    e.preventDefault();
    W.sel = (W.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
    drawWaiting();
    return true;
  }

  if (e.key === "Enter" && !cmd) { e.preventDefault(); openRow(W.sel); return true; }
  if (cmd || e.ctrlKey || e.altKey) return false;
  if (e.key === "Backspace" && W.query) { e.preventDefault(); W.query = W.query.slice(0, -1); W.sel = 0; drawWaiting(); return true; }

  if (e.key.length === 1 && (W.query || e.key !== " ")) {
    e.preventDefault();
    W.query += e.key;
    W.sel = 0;
    drawWaiting();
    return true;
  }

  return false;
}

/// Esc on the list: clears the filter; false when there is none, and the bar hides.
export function waitingBack() {
  if (!W.query) return false;
  W.query = "";
  W.sel = 0;
  drawWaiting();
  return true;
}

function readNotes(list: Note[]) {
  unread = new Set(list.flatMap(n => n.kind === "finished" && !n.read && n.target && typeof n.target === "object" && "agent" in n.target ? [sessionKey(n.target.agent)] : []));
  drawWaiting();
}

// The bar keeps what it needs warm, so a double tap of Option draws at once.
if (!PANE) {
  sessionsRead.add(() => { known = true; drawWaiting(); });
  onPlaces(drawWaiting);
  onClosing(drawWaiting);
  // While the list shows, a change to the sessions reads them again, once a second at most.
  const reread = ambient(() => ensureSessions(), AMBIENT_MS, () => S.view === "waiting");
  subscribeTopics(topic("home"), () => reread(), null, () => S.view === "waiting");
  for (const name of ["sessions-changed", "agent-title-changed", "reconnected"]) listen(name, () => reread());
  listen<Note[]>("notes", e => readNotes(e.payload));
  invoke<Note[]>("notes_list").then(readNotes).catch(() => {});
  ensureSessions().catch(() => {});
}
