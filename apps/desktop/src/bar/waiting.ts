// The bar's default view: the sessions waiting for the user, drawn from the
// sessions every page shares and the notifications this Mac holds. Return
// opens one in the bar, with its conversation and composer.
import { flushSync } from "react-dom";
import { type AgentSession } from "../agents/agents-state.ts";
import { folderName } from "../agents/agent-kind.ts";
import { onPlaces, placeOf } from "../agents/place.ts";
import { placeRepo } from "../agents/place-state.ts";
import { openAgent } from "../agents/session.tsx";
import { ensureSessions, sessions, sessionsRead } from "../agents/sessions.ts";
import { onClosing, sessionVisible } from "../agents/closing.ts";
import type { Note } from "../center/notices-state.ts";
import { formatAgo } from "../ui/format.ts";
import { ICON } from "../ui/icons.tsx";
import { invoke, listen } from "../ui/tauri.ts";
import { $, bar, newChat, PANE, S, setView } from "./core.ts";
import { hintModel } from "./hints.ts";
import { type BarPageSlot, openBarPage } from "./bar-page.ts";
import { waitingSessions, type WaitingSession } from "./waiting-state.ts";
import { sessionKey } from "../agents/conversation-state.ts";
import type { WaitingRow } from "./waiting-view.tsx";

const W = { sel: 0, query: "", rows: [] as WaitingSession[] };
/// Sessions with an unread finished turn, by `sessionKey`.
let unread = new Set<string>();
/// Whether the shared sessions were read once, so an empty list means none.
let known = false;
let page: BarPageSlot | null = null;

/// Shows the list. Known sessions draw at once; a read follows only when a
/// change came since.
export function renderWaiting() {
  setView("waiting");
  W.sel = 0;
  W.query = "";
  drawWaiting(true);
  ensureSessions().catch(() => {});
}

/// The project a session works in: its repository once its machine said, else its folder.
const project = (s: AgentSession) => {
  const place = placeOf(s);
  return place ? placeRepo(place) : folderName(s.cwd);
};

const pick = () => waitingSessions(sessions.filter(sessionVisible), unread, W.query, project);

function row({ session: s, kind }: WaitingSession): WaitingRow {
  const title = s.title || folderName(s.cwd) || s.agent;
  return { key: sessionKey(s), agent: s.agent, title, sub: [project(s), s.machine_name, formatAgo(s.last_activity, "short")].join(" · "), kind, doing: s.doing || "Working" };
}

/// Draws the list; the list takes the focus with `focus`, or when it had it.
function drawWaiting(focus = false) {
  if (S.view !== "waiting") return;
  W.rows = pick();
  W.sel = Math.min(W.sel, Math.max(0, W.rows.length - 1));
  const need = W.rows.filter(r => r.kind === "asking" || r.kind === "done").length;
  const summary = !known ? "Looking at your machines…" : need ? `${need} waiting for you` : "Nothing waiting";
  const active = document.activeElement;
  const focused = focus || active?.id === "wtlist" || !active || active === document.body;
  const slot = page ??= openBarPage(() => { page = null; });
  flushSync(() => slot.show({ view: "waiting", props: {
    rows: W.rows.map(row), selected: W.sel, summary, query: W.query,
    hints: hintModel([[ICON.ret, "open"], ["↑↓", "move"], ["⌘N", "new agent"], ["esc", W.query ? "clear" : "to hide"]]),
    open: openRow, newAgent: newChat,
  } }));
  // The command center's hidden pane draws too; WebKit would focus a list
  // it has not laid out yet and take the focus from the window's page.
  if (focused && bar.checkVisibility()) $("#wtlist")?.focus();
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
  listen<Note[]>("notes", e => readNotes(e.payload));
  invoke<Note[]>("notes_list").then(readNotes).catch(() => {});
  ensureSessions().catch(() => {});
}
