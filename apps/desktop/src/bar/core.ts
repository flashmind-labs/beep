// The bar's page, and the pane the command center window shows it in.
import type { GroupState } from "./groups.ts";

export { invoke, Channel, listen } from "../tauri.ts";

export const ICON = {
  check: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  x: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  chev: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>',
  ret: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M20 5v7a3 3 0 0 1-3 3H5m4-4l-4 4 4 4"/></svg>',
  pin: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  group: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0113 0"/><circle cx="17" cy="9" r="2.5"/><path d="M16 14a5 5 0 016 5"/></svg>',
  doc: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>',
  phone: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18h2"/></svg>',
  image: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="M21 15l-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/></svg>',
  save: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>',
};
export const COPY_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/></svg>';

export const bar = document.getElementById("bar")!;
/// True when this page runs as a pane in the command center window rather
/// than as the bar. A few bar-only spots check it.
export const PANE = bar.dataset.mode === "window";

/// What the bar or the pane shows.
export type View =
  | "chat" | "chats" | "group" | "ginfo" | "invite" | "gnew" | "joincode" | "joining"
  | "link" | "revoked" | "invite-phone" | "starting" | "settings" | "apps" | "appperms" | "mcp"
  | "machines" | "memories" | "routines" | "rnew" | "grants" | "skills" | "research" | "model"
  | "agent" | "agentnew" | "agents";

/// Whether this Mac is linked to an account.
export type LinkState = "unlinked" | "linked" | "revoked";

/// A row of the chat list: a chat or a group.
export interface ChatRow {
  kind: "chat" | "group";
  id: string;
  title: string;
  pinned: boolean;
  updated_at: number;
  snippet?: string;
  /// Beep is typing in the group.
  typing?: boolean;
}

export type ChatsMode = "list" | "actions" | "rename" | "delete" | "share";

export interface ChatsState {
  list: ChatRow[];
  sel: number;
  query: string;
  mode?: ChatsMode;
  /// The group the list was opened from, for Esc.
  back?: string | null;
  share?: { id: string; url?: string; error?: string } | null;
}

export interface BarState {
  view: View;
  link: LinkState;
  session: string | null;
  chats: ChatsState;
  /// The last chat list, drawn at once while a fresh one loads.
  cache: ChatRow[];
  timer: ReturnType<typeof setInterval> | undefined;
}

/// What the pane shows, for the window's sidebar.
export interface PaneNow {
  view: View;
  session: string | null;
  group: string | null;
}

/// The group on screen. Drafts and approvals are kept per group.
export const G: GroupState = { id: null, chat: null, pending: [], error: "", mention: null, poll: undefined, leave: false, drafts: new Map(), approvals: new Map() };
export const GROUP_VIEWS = new Set<View>(["group", "ginfo", "invite", "gnew"]);

export function current(): PaneNow {
  return { view: S.view, session: S.session, group: GROUP_VIEWS.has(S.view) ? G.id : null };
}

/// Told when the view or the open chat changes, for the window's sidebar.
export const changeListeners: ((now: PaneNow) => void)[] = [];
let changeQueued = false;
export function changed() {
  if (changeQueued || !changeListeners.length) return;
  changeQueued = true;
  queueMicrotask(() => {
    changeQueued = false;
    const now = current();
    for (const fn of changeListeners) { try { fn(now); } catch {} }
  });
}
export const S: BarState = new Proxy<BarState>({
  view: "chat",
  link: "unlinked",
  session: null,
  chats: { list: [], sel: 0, query: "" },
  cache: [],
  timer: undefined,
}, {
  set(t, k, v) {
    const key = k as keyof BarState;
    const old = t[key];
    (t as unknown as Record<string, unknown>)[key] = v;
    if ((key === "view" || key === "session") && old !== v) changed();
    return true;
  },
});

export { esc } from "../components/format.ts";
/// The first element of `html`.
export function h<T extends Element = HTMLElement>(html: string): T {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild as T;
}
export const $ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel);
/// An element `$` is sure to find, as the view on screen draws it.
export const $$ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel)!;

/// A tool step of a turn, as the chat shows it. A step still running when
/// its turn ends is marked stopped.
export interface Step {
  id: string;
  label: string;
  state: "run" | "ok" | "bad" | "stop";
}

/// A turn of a chat: a prompt and its reply. The element keeps what the
/// chat needs to redraw and answer it.
export interface Turn extends HTMLDivElement {
  steps: Step[];
  folded: boolean;
  done?: boolean;
  user?: boolean;
  failed?: boolean;
  stopped?: boolean;
  /// Resolves the run id of a turn stopped before it started.
  started?: (runId: string | null) => void;
  agent?: string;
  model?: string;
  status?: string;
}

/// A chat's scrolling body, which knows whether it follows the newest turn.
export interface ChatBody extends HTMLElement {
  stuck?: boolean;
}

export interface LiveRun {
  id: string | null;
  turn: Turn;
}
export interface PendingApproval {
  queryId: string;
  turn: Turn;
  card: HTMLElement;
  grant?: boolean;
}
export interface LiveEntry {
  run: LiveRun | null;
  approval: PendingApproval | null;
  body: ChatBody | null;
}

// Chats with a run streaming or an approval waiting, by session id. Each keeps
// its own body, so the bar can show other chats and come back to it.
export const live = new Map<string | null, LiveEntry>();
// Runs the bar let go of and is still telling the server to stop, by session id.
// The chat's next prompt waits for that, so the server has the stop first.
export const stopping = new Map<string | null, Promise<unknown>>();
export const here = () => live.get(S.session);
// Prompts typed while a chat's reply runs, by session id, oldest first. Each
// sends as its own turn once the reply before it finishes cleanly; a stopped
// or failed reply holds them until the user sends them with Cmd-Return.
export const queued = new Map<string | null, { list: string[]; held: boolean }>();
export function entry(id: string | null): LiveEntry {
  let e = live.get(id);
  if (!e) live.set(id, e = { run: null, approval: null, body: null });
  return e;
}
// Made here so a new chat's run can be tracked before the server answers.
export function newId() {
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = b[6] & 0x0f | 0x40;
  b[8] = b[8] & 0x3f | 0x80;
  const x = [...b].map(n => n.toString(16).padStart(2, "0")).join("");
  return `${x.slice(0, 8)}-${x.slice(8, 12)}-${x.slice(12, 16)}-${x.slice(16, 20)}-${x.slice(20)}`;
}

/// What the command center window does for the pane. The bar has none.
export interface WindowHooks {
  /// Opens the window's Settings sheet.
  openSettings(): void;
  /// Settings' Instructions row: the sheet closes and the view shows.
  openInstructions(): void;
  /// Settings' Coding agents row: the sheet closes and the page shows.
  openProviders(): void;
  /// A page Settings leads to, such as Apps: the sheet closes and the pane
  /// shows it; null only closes.
  settingsAway(view: string | null): void;
  /// Draws window view `name` (such as "providers") inside `root`, a page of
  /// the Settings sheet, opened with `arg`.
  embedView?(name: string, root: HTMLElement, arg: unknown, tools?: (nodes: Node[]) => void): () => void;
  /// ⌘N in the pane: the window picks the chat.
  newChat(): void;
  /// Esc from an agent view goes back to the view that opened it.
  goBack(from: BackTo): void;
  /// What shows atop chat `session`, such as the card of the pull request or
  /// issue it is about; null for nothing.
  chatTop?(session: string): HTMLElement | null;
}

/// A window view to go back to, as the window opened an agent view from it.
export interface BackTo {
  view: string;
  arg?: unknown;
}

/// Set by the window once it has loaded the pane.
export let windowHooks: WindowHooks | null = null;
export function connectWindow(hooks: WindowHooks) {
  windowHooks = hooks;
}
