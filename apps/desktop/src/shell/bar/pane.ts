// The pane: the bar's views drawn inside the command center window, which
// drives them through `pane`.
import "./events.ts";
import type { AgentPlace } from "../../parts/session/agentnew.tsx";
import { renderAgents } from "../../parts/session/agents-list.ts";
import { type AgentRow } from "../../stores/agents/agents-state.ts";
import { openSession } from "../../parts/session/open-state.ts";
import { type AssistantOpen, openAssistantPage, openedChat } from "../../screens/agents/assistant-page.tsx";
import { closePaneTabs, newTerminal, openTerminal } from "../../parts/session/media.ts";
import { showChanges } from "../../parts/session/changes.ts";
import type { TerminalRow } from "../../stores/terminals/terminal-state.ts";
import { sessions } from "../../stores/agents/sessions.ts";
import { type BackTo, windowRequests } from "../../stores/bar/core.ts";
import { mountPane } from "./keys.ts";
import { loadHosts } from "./views.ts";
import { setPaneShown } from "../../ui/active.ts";

/// What the window can open in the pane by name.
type PanePage = "agents" | "settings" | "machines";

/// What the window can open in the pane: a page, or an agent view.
export type PaneTarget =
  | PanePage
  | null
  | { agent: AgentRow & { kind?: string }; from?: BackTo; adopt?: boolean }
  | { draft: { project: string; machine: string | null; terminal?: true | TerminalRow } }
  | { agentnew: { prompt?: string; at?: AgentPlace | null; assistant: boolean }; from?: BackTo }
  | { assistant: AssistantOpen };

export const pane = {
  /// Shows `target`.
  open(target: PaneTarget) {
    loadHosts();

    if (target && typeof target === "object") {
      // Agent views are not a CenterTarget: Home and the sidebar open them.
      // A notice names a session with no folder: the loaded list has the row, when it has one.
      if ("assistant" in target) return openAssistantPage(target.assistant);

      if ("agent" in target) {
        openedChat(target.agent);
        const { machine_id, agent, id } = target.agent;
        const row = sessions.find(s => s.machine_id === machine_id && s.agent === agent && s.id === id);
        openSession(row ?? target.agent, { back: target.from ?? "agents", adopt: target.adopt });
        // A reviewer opens beside the changes it reviewed.
        if (target.agent.reviewer) showChanges(false);
        return;
      }

      if ("draft" in target) {
        // Loaded when needed: it brings in the project page, which only the command center has.
        return import("../../parts/session/draft.tsx").then(m => {
          m.openDraft(target.draft.project, target.draft.machine);
          const { terminal } = target.draft;
          if (terminal === true) newTerminal(undefined, true); else if (terminal) openTerminal(terminal);
        });
      }

      return import("../../parts/session/agentnew.tsx").then(m => m.renderAgentNew(target.agentnew.prompt ?? "", target.from ?? "agents", target.agentnew.at ?? null, target.agentnew.assistant));
    }

    if (target === "settings") return import("../../screens/settings/settings.tsx").then(m => m.renderSettings());
    if (target === "machines") return import("../../screens/machines/machines.tsx").then(m => m.renderMachines());
    return renderAgents();
  },
  /// The pane goes off screen.
  leave() {
    closePaneTabs();
  },
  /// Whether the pane is on screen and takes the keys.
  mounted(on: boolean) {
    mountPane(on);
    setPaneShown(on);
  },
};

windowRequests.follow(request => {
  if (request.kind === "open-agent") void pane.open({ agent: request.agent });
});
