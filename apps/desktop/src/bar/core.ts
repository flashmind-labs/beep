// The bar's page, and the pane the command center window shows it in.
import type { GroupState } from "../chat/groups.ts";
import type { ChatBody, Turn } from "../ui/turn.ts";
import type { ViewName } from "../center/state.ts";

export const bar = document.getElementById("bar")!;
/// True when this page runs as a pane in the command center window rather
/// than as the bar. A few bar-only spots check it.
export const PANE = bar.dataset.mode === "window";

/// What the bar or the pane shows.
export type View =
  | "chat" | "chats" | "group" | "ginfo" | "invite" | "gnew" | "joincode" | "joining"
  | "link" | "login" | "revoked" | "invite-phone" | "starting" | "settings" | "apps" | "appperms" | "mcp"
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
  /// Signed out of the server in Settings. `link` reads "unlinked" meanwhile, so the paths
  /// that work with no account work the same; this says why.
  signedOut: boolean;
  /// Whether the desktop talks to a server at all, as Rust says (`status`). Off, nothing of one
  /// shows: no account, sign in, subscription or error about a server.
  server: boolean;
  /// Whether live mode can start, as Rust says (`status`). Off, no hint mentions it or the orb.
  live: boolean;
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
  signedOut: false,
  server: false,
  live: false,
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

export const $ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel);
/// An element `$` is sure to find, as the view on screen draws it.
export const $$ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel)!;

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
  /// Settings' Report a problem row: the sheet closes and the report sheet opens.
  openReport(): void;
  /// A page Settings leads to, such as Apps: the sheet closes and the pane
  /// shows it; null only closes.
  settingsAway(view: string | null): void;
  /// Draws window view `name` (such as "providers") inside `root`, a page of
  /// the Settings sheet, opened with `arg`.
  embedView?(name: ViewName, root: HTMLElement, arg: unknown, tools?: (nodes: Node[]) => void): () => void;
  /// ⌘N in the pane: the window picks where the agent starts.
  newAgent(): void;
  /// Esc from an agent view goes back to the view that opened it.
  goBack(from: BackTo): void;
  /// What shows atop chat `session`, such as the card of the pull request or
  /// issue it is about; null for nothing.
  chatTop?(session: string): HTMLElement | null;
  /// The project a coding session runs in, and how to open it.
  sessionProject?(s: { machine_id: string; cwd: string }): { label: string; name: string; open(): void } | null;
}

/// A window view to go back to, as the window opened an agent view from it.
export interface BackTo {
  view: ViewName;
  arg?: unknown;
}

/// Set by the window once it has loaded the pane.
export let windowHooks: WindowHooks | null = null;
export function connectWindow(hooks: WindowHooks) {
  windowHooks = hooks;
}
