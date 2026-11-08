import type { View } from "../bar/core.ts";
import type { ViewName } from "../center/state.ts";
import { keyHidden } from "../settings/subscription.ts";
import { commandKey, WINDOWS } from "./platform.ts";

export type WindowCommand =
  | { kind: "history"; forward: boolean }
  | { kind: "close" }
  | { kind: "project-window"; project: string }
  | { kind: "blocked" }
  | { kind: "palette" | "settings" | "shortcuts" | "research" | "new-agent" | "project-terminal" }
  | { kind: "pane"; view: "memories" | "routines" | "skills" };

export interface WindowKeyContext {
  page: ViewName | null;
  pane: View | null;
  project: string | null;
  projectWindow: boolean;
  subscribed: boolean;
}

type WindowKeyEvent = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey" | "defaultPrevented">;

/** Resolve window commands in priority order; pane commands stay with the pane. */
export function windowCommand(event: WindowKeyEvent, at: WindowKeyContext, windows = WINDOWS): WindowCommand | null {
  if (!commandKey(event, windows)) return null;
  const key = event.key.toLowerCase();
  if ((key === "[" || key === "]") && !event.shiftKey) return { kind: "history", forward: key === "]" };
  if (key === "w" && !event.shiftKey) return { kind: "close" };
  if (key === "n" && event.shiftKey && at.page === "projects" && at.project && !at.projectWindow) return { kind: "project-window", project: at.project };
  if (key !== "k" && keyHidden(key, at.subscribed)) return { kind: "blocked" };
  if (windowTakes(key, at.page === "pane" ? at.pane : null)) {
    switch (key) {
      case "k": return { kind: "palette" };
      case ",": return { kind: "settings" };
      case "/": return { kind: "shortcuts" };
      case "r": return { kind: "research" };
      case "m": return { kind: "pane", view: "memories" };
      case "j": return { kind: "pane", view: "routines" };
      case "l": return { kind: "pane", view: "skills" };
    }
  }
  if (key === "n" && at.page !== "pane") return { kind: "new-agent" };
  if (key === "t" && !event.shiftKey && !event.altKey && (!event.ctrlKey || windows) && !event.defaultPrevented) return { kind: "project-terminal" };
  return null;
}

/** Window-owned shortcuts, except model picking and renaming in their pane views. */
export function windowTakes(key: string, paneView: View | null = null): boolean {
  switch (key.toLowerCase()) {
    case "m": return paneView !== "agentnew";
    case "r": return paneView !== "chats";
    case "k": case ",": case "/": case "j": case "l": return true;
    default: return false;
  }
}
