// The pane: the bar's views drawn inside the command center window, which
// drives them through `pane`.
import "./events.ts";
import type { AgentPlace } from "../agents/agentnew.tsx";
import { renderAgents } from "../agents/agents-list.ts";
import { type AgentRow } from "../agents/agents-state.ts";
import { openSession } from "../agents/open-state.ts";
import { type AssistantOpen, openAssistantPage, openedChat } from "../agents/assistant-page.tsx";
import { closePaneTabs, newTerminal, openTerminal } from "../agentpane/media.ts";
import { showChanges } from "../agentpane/changes.ts";
import type { TerminalRow } from "../terminals/terminal-state.ts";
import { sessions } from "../agents/sessions.ts";
import { type BackTo, changeListeners, current, type PaneNow, windowRequests } from "./core.ts";
import { mountPane } from "./keys.ts";
import { loadHosts } from "./views.ts";
import { setPaneShown } from "../ui/active.ts";

let settingsLoaded: typeof import("../settings/settings.tsx") | null = null;
const ensureSettings = async () => settingsLoaded ??= await import("../settings/settings.tsx");

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
  /// Draws Settings into `el`, the window's sheet, leaving the pane as it
  /// is. It opens on `page` when given, else on the page picked last.
  settings(el: HTMLElement, page?: string | null) {
    loadHosts();
    return ensureSettings().then(m => m.renderSettings(el, page ?? undefined));
  },
  /// Esc in the sheet: back from a subpage to its page. False when the
  /// sheet shows a page, so Esc closes it.
  settingsBack: () => settingsLoaded ? settingsLoaded.settingsBack() : false,
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
        return import("../agents/draft.tsx").then(m => {
          m.openDraft(target.draft.project, target.draft.machine);
          const { terminal } = target.draft;
          if (terminal === true) newTerminal(undefined, true); else if (terminal) openTerminal(terminal);
        });
      }

      return import("../agents/agentnew.tsx").then(m => m.renderAgentNew(target.agentnew.prompt ?? "", target.from ?? "agents", target.agentnew.at ?? null, target.agentnew.assistant));
    }

    if (target === "settings") return ensureSettings().then(m => m.renderSettings());
    if (target === "machines") return import("../machines/machines.tsx").then(m => m.renderMachines());
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

windowRequests.follow(request => {
  if (request.kind === "open-agent") void pane.open({ agent: request.agent });
});
