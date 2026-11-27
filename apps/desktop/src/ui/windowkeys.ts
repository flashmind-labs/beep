import type { ViewName } from "../stores/center/state.ts";
import { commandKey, WINDOWS } from "./platform.ts";

type WindowCommand =
  | { kind: "history"; forward: boolean }
  | { kind: "close" }
  | { kind: "project-window"; project: string }
  | { kind: "palette" | "settings" | "shortcuts" | "sidebar" | "assistant" | "new-agent" | "project-terminal" };

export interface WindowKeyContext {
  page: ViewName | null;
  project: string | null;
  /// The window shows a project or a chat alone.
  alone: boolean;
}

type WindowKeyEvent = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey" | "defaultPrevented">;

/** Resolve window commands in priority order; pane commands stay with the pane. */
export function windowCommand(event: WindowKeyEvent, at: WindowKeyContext, windows = WINDOWS): WindowCommand | null {
  if (!commandKey(event, windows)) return null;
  const key = event.key.toLowerCase();
  if ((key === "[" || key === "]") && !event.shiftKey) return { kind: "history", forward: key === "]" };
  if (key === "w" && !event.shiftKey) return { kind: "close" };
  if (key === "n" && event.shiftKey && at.page === "projects" && at.project && !at.alone) return { kind: "project-window", project: at.project };

  if (key === "k") return { kind: "palette" };
  if (key === ",") return { kind: "settings" };
  if (key === "/") return { kind: "shortcuts" };
  if (key === "e" && !event.shiftKey) return { kind: "sidebar" };
  if (key === "l" && !event.shiftKey && !at.alone) return { kind: "assistant" };
  if (key === "n" && at.page !== "pane") return { kind: "new-agent" };
  if (key === "t" && !event.shiftKey && !event.altKey && (!event.ctrlKey || windows) && !event.defaultPrevented) return { kind: "project-terminal" };
  return null;
}

/** Window-owned shortcuts: search, Settings, the shortcuts sheet, the sidebar and the Assistant. */
export function windowTakes(key: string): boolean {
  return key === "k" || key === "K" || key === "," || key === "/" || key === "e" || key === "E" || key === "l" || key === "L";
}
