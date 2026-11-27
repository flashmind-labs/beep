// What every tab does to the pane's tabs: draw one again, say something in its
// footer, close it, and the strip item it shows. Below the tabs, so they use it
// without reaching up to the pane's commands.
import { shownMessage } from "../../stores/agents/session-frame-state.ts";
import { confirmSheet } from "../../ui/menu.tsx";
import type { IconName } from "../../ui/icons.tsx";
import { AGENTS, ARTS, FILES, RUNNING, SUBS } from "../../stores/agentpane/media-state.ts";
import { dockHolding, dockOf, docked, openTabs, paneModel, place, setOn, type PaneCounts } from "./media-layout.ts";
import { rememberPane } from "./pane-memory.ts";
import { P, redraw, type MediaTab, type TabKind } from "../../stores/agentpane/pane-state.ts";
import { dropTerm, killTerm } from "./terminal.ts";
import type { PaneTabItem } from "./tabs-view.tsx";

/// Draws tab `t` again after its own state changed: its load, terminal or note.
export function touchTab(t: MediaTab) {
  t.rev++;
  if (paneModel.getState().tabs.includes(t)) paneModel.setState({ tabs: [...paneModel.getState().tabs] });
}

/// Says `text` in tab `t`'s footer, as a save or copy did.
export function noteTab(t: MediaTab, text: string) {
  t.note = text;
  touchTab(t);
}

/// Asks before dropping a text tab's unsaved edits; `discard` drops them.
export function discardSheet(tab: MediaTab, discard: () => void) {
  confirmSheet({ title: `Discard changes to ${tab.name}?`, text: "Your edits since the last save are lost.", action: "Discard", run: async () => discard() });
}

export function closeTab(id: string) {
  const i = openTabs().findIndex(t => t.id === id);
  if (i < 0) return;
  // The tab goes once its terminal stopped, and says why when it could not.
  const tab = openTabs()[i];

  if (tab.edit?.dirty) {
    discardSheet(tab, () => { tab.edit = null; closeTab(id); });
    return;
  }

  if (tab.term) {
    if (tab.closed) return;
    void stopTerminalTab(tab).catch(e => {
      noteTab(tab, `Could not stop it: ${e instanceof Error ? e.message : e}`);
    });
    return;
  }

  // A shell still starting is stopped once it is there.
  if (tab.kind === "term") tab.closed = true;
  const d = dockOf(id);
  dropTab(tab);
  paneModel.setState(s => ({ tabs: s.tabs.filter(t => t !== tab) }));
  P.moved.delete(id);

  // The neighbour in the same dock shows next.
  if (paneModel.getState().on[d] === id) {
    const left = docked()[d];
    const source = tab.exported?.source.tab;
    setOn(d, source && left.includes(source) ? source : openTabs().slice(i).find(t => left.includes(t.id))?.id ?? left.at(-1) ?? FILES);
  }

  rememberPane();
  redraw();
}

/// Stops a terminal and removes its tab after the machine acknowledges.
export async function stopTerminalTab(tab: MediaTab) {
  const term = tab.term;
  if (!term) return;
  tab.closed = true;

  try {
    await killTerm(term);
  } catch (e) {
    if (tab.term === term) {
      tab.closed = false;
      touchTab(tab);
    }
    throw e;
  }

  if (tab.term !== term) return;
  tab.term = undefined;
  closeTab(tab.id);
}

export function dropTab(t: MediaTab) {
  t.player?.pause();
  if (t.term) dropTerm(t.term);
  t.model?.dispose();
  if (t.url?.startsWith("blob:")) URL.revokeObjectURL(t.url);
}

/// Esc in the window with a zoomed or focused pane: the pane steps back and the
/// conversation's input takes the keys. False when no pane has them.
export function leavePane() {
  const focus = document.activeElement;
  if (!paneModel.getState().zoom && !dockHolding(focus)) return false;
  paneModel.setState({ zoom: null });
  place();
  rememberPane();
  const input = shownMessage();
  if (input) input.focus(); else if (focus instanceof HTMLElement) focus.blur();
  return true;
}

/// The tab strip item of tab `id`.
export function paneTabItem(id: string, { tabs, counts, subCount } = paneModel.getState()): PaneTabItem {
  if (isFixed(id)) return { id, name: FIXED_NAMES[id], icon: TAB_ICONS[id], count: fixedCount(id, counts, subCount), closeIcon: null };
  const tab = tabs.find(t => t.id === id);
  return { id, name: tab?.name ?? id, icon: TAB_ICONS[tab?.kind ?? "text"], count: null, closeIcon: "x" };
}

type Fixed = "subagents" | "agents" | "files" | "changes" | "artifacts" | "running" | "device";

const FIXED_NAMES: Record<Fixed, string> = { device: "Device", running: "Running", subagents: "Subagents", agents: "Agents", artifacts: "Artifacts", changes: "Changes", files: "Files" };

function isFixed(id: string): id is Fixed {
  return Object.hasOwn(FIXED_NAMES, id);
}

function fixedCount(id: Fixed, counts: PaneCounts, subCount: number) {
  if (id === SUBS) return subCount;
  if (id === RUNNING) return counts.running;
  if (id === ARTS) return counts.artifacts;
  if (id === AGENTS) return counts.agents;
  return null;
}

const TAB_ICONS: Record<TabKind | Fixed, IconName> = {
  files: "paneFiles", changes: "paneChanges", text: "paneText", term: "paneTerm", session: "paneSubagents", image: "paneImage", model: "paneModel",
  video: "paneVideo", audio: "paneAudio", pdf: "panePdf", artifacts: "paneArtifacts", running: "paneRunning", subagents: "paneSubagents", agents: "paneSubagents", device: "paneDevice",
};

