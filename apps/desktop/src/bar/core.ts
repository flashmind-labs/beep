// The bar's page, and the pane the command center window shows it in.
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { signal } from "../ui/signal.ts";
import { invoke } from "../ui/tauri.ts";
import type { ViewName, ViewArg, Location } from "../center/state.ts";
import type { AgentRow } from "../agents/agents-state.ts";

export const bar = document.getElementById("bar")!;
/// True when this page runs as a pane in the command center window rather
/// than as the bar. A few bar-only spots check it.
export const PANE = bar.dataset.mode === "window";

/// What the bar or the pane shows.
export type View = "settings" | "machines" | "agent" | "agentnew" | "agents" | "waiting";

export interface BarState {
  view: View;
}

/// What the pane shows, for the window's sidebar.
export type PaneNow = BarState;

export function current(): PaneNow {
  return { view: S.view };
}

/// Told when the view changes, for the window's sidebar.
export const changeListeners: ((now: PaneNow) => void)[] = [];
let changeQueued = false;
function changed() {
  if (changeQueued) return;
  changeQueued = true;
  queueMicrotask(() => {
    changeQueued = false;
    const now = current();
    refreshTopicSubscriptions();
    for (const fn of changeListeners) { try { fn(now); } catch {} }
  });
}
const route: BarState = { view: "waiting" };
/// What the bar or the pane shows now; `setView` is the one place that changes it.
export const S: Readonly<BarState> = route;

export function setView(view: View) {
  if (route.view === view) return;
  route.view = view;
  // The pane's layout keys on its view: marked at once, before the view draws and measures.
  if (PANE) bar.dataset.view = view;
  changed();
}
if (PANE) bar.dataset.view = S.view;

export const $ = <T extends Element = HTMLElement>(sel: string) => bar.querySelector<T>(sel);

/// What the pane asks of the command center window, which follows `windowRequests`. The bar has
/// no window: nothing follows there.
export type WindowRequest =
  /// Opens the window's Settings sheet.
  | { kind: "settings" }
  /// Settings' Report a problem row: the sheet closes and the report sheet opens.
  | { kind: "report" }
  /// Settings' Keyboard shortcuts row: the sheet closes and the shortcuts sheet opens.
  | { kind: "shortcuts" }
  /// ⌘N in the pane: the window picks where the agent starts.
  | { kind: "new-agent" }
  /// Draws window view `name` (such as "providers") inside `root`, a page of the Settings sheet,
  /// opened with `arg`. `unembed` with the same token mounts it back into its own element.
  | { kind: "embed"; name: ViewName; root: HTMLElement; arg: ViewArg; token: object }
  | { kind: "unembed"; token: object }
  /// Opens machine `id`'s page in Settings.
  | { kind: "machine-settings"; id: string }
  /// Opens the Skills of the chat in `folder`.
  | { kind: "chat-skills"; folder: string; title: string }
  /// Shows the agent view of `agent` in the pane.
  | { kind: "open-agent"; agent: AgentRow };

export const windowRequests = signal<WindowRequest>();

/// Draws window view `name` in `root`; what it returns mounts the view back into its own element.
export function embedInWindow(name: ViewName, root: HTMLElement, arg: ViewArg): () => void {
  const token = {};
  windowRequests.notify({ kind: "embed", name, root, arg, token });
  return () => windowRequests.notify({ kind: "unembed", token });
}

/// A window view to go back to, as the window opened an agent view from it.
export type BackTo = Location;
/// Where Esc from an agent view goes: Every session, the bar's waiting list, a window view,
/// or with "hide" nowhere: the bar hides and the window goes Home.
export type AgentReturn = "agents" | "waiting" | "hide" | BackTo;

/// Hands a view to the command center window, which hides the bar.
export function toWindow(target: unknown) {
  return invoke("center_open", { target }).catch(() => {});
}

/// A new chat starts in a project: the window's own pick, else Projects.
export function newChat() {
  if (PANE) return windowRequests.notify({ kind: "new-agent" });
  void toWindow({ projects: { project: null } });
}
