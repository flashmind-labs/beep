import { AG, type AgentRow } from "../agents/agents-state.ts";
import { mainTarget } from "../project/projectwindow.ts";
import { PANE, toWindow } from "./core.ts";
import type { HintViewModel } from "../../parts/bar/hints-view.tsx";

// ---------- Hints ----------

/// A key and what it does, as the hint row and the shortcuts sheet show it.
export type Hint = readonly string[];

export function hintModel(items: readonly Hint[]): HintViewModel {
  return { items: forPane(items), window: false };
}

/// The window does not hide, and ⌘K searches there.
function forPane(items: readonly Hint[]) {
  if (!PANE) return items;
  return items
    .filter(([, t]) => t !== "to hide")
    .map(([k, t]) => k === "⌘K" ? [k, "search"] : [k, t]);
}

/// Opens agent session `session` in the window.
export const openAgentInWindow = (session: AgentRow) => toWindow(mainTarget({ view: "pane", arg: { agent: session } }));

/// What a button in the hint row does: the open session moves to the window.
export function openInWindow() {
  if (AG.page && !AG.draft) void openAgentInWindow(AG.page.session);
}
