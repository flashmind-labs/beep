// The pane: the bar's views drawn inside the command center window, which
// drives them through `pane`.
import "./events.ts";
import { type AgentPlace, renderAgentNew } from "../agents/agentnew.ts";
import { type AgentRow, renderAgents } from "../agents/agents.ts";
import { openAgent } from "../agents/session.ts";
import { closePaneTabs, newTerminal, openTerminal, showChanges } from "../agentpane/media.ts";
import type { TerminalRow } from "../terminals/terminals.ts";
import { sessions } from "../agents/sessions.ts";
import { type BackTo, changeListeners, connectWindow, current, type PaneNow } from "./core.ts";
import { mountPane } from "./keys.ts";
import { loadHosts } from "./views.ts";
import { renderMachines } from "../machines/machines.ts";
import { renderSettings, settingsBack } from "../settings/settings.ts";
import { setPaneShown } from "../ui/active.ts";

/// What the window can open in the pane by name.
export type PanePage = "agents" | "settings" | "machines";

/// What the window can open in the pane: a page, or an agent view.
export type PaneTarget =
  | PanePage
  | null
  | { agent: AgentRow & { kind?: string }; from?: BackTo; adopt?: boolean }
  | { draft: { project: string; machine: string | null; terminal?: true | TerminalRow } }
  | { agentnew: { prompt?: string; at?: AgentPlace | null }; from?: BackTo };

export const pane = {
  /// Hands the pane what the window does for it.
  connect: connectWindow,
  /// Draws Settings into `el`, the window's sheet, leaving the pane as it
  /// is. It opens on `page` when given, else on the page picked last.
  settings(el: HTMLElement, page?: string) {
    loadHosts();
    return renderSettings(el, page);
  },
  /// Esc in the sheet: back from a subpage to its page. False when the
  /// sheet shows a page, so Esc closes it.
  settingsBack,
  /// Shows `target`.
  open(target: PaneTarget) {
    loadHosts();

    if (target && typeof target === "object") {
      // Agent views are not a CenterTarget: Home and the sidebar open them.
      // A notice names a session with no folder: the loaded list has the row, when it has one.
      if ("agent" in target) {
        const { machine_id, agent, id } = target.agent;
        const row = sessions.find(s => s.machine_id === machine_id && s.agent === agent && s.id === id);
        openAgent(row ?? target.agent, { back: target.from ?? "agents", adopt: target.adopt });
        // A reviewer opens beside the changes it reviewed.
        if (target.agent.reviewer) showChanges(false);
        return;
      }

      if ("draft" in target) {
        // Loaded when needed: it brings in the project page, which only the command center has.
        return import("../agents/draft.ts").then(m => {
          m.openDraft(target.draft.project, target.draft.machine);
          const { terminal } = target.draft;
          if (terminal === true) newTerminal(undefined, true); else if (terminal) openTerminal(terminal);
        });
      }

      return renderAgentNew(target.agentnew.prompt ?? "", target.from ?? "agents", target.agentnew.at ?? null);
    }

    if (target === "settings") return renderSettings();
    if (target === "machines") return renderMachines();
    return renderAgents();
  },
  /// The pane goes off screen.
  leave() {
    closePaneTabs();
  },
  current,
  onChange(fn: (now: PaneNow) => void) { changeListeners.push(fn); },
  /// Whether the pane is on screen and takes the keys.
  mounted(on: boolean) {
    mountPane(on);
    setPaneShown(on);
  },
};
