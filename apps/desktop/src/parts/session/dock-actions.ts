// What a click or a menu does to the pane's tabs and docks: the menu of a tab,
// picking a tab, moving a dock's tabs to the other and folding one. Above the
// commands in media.ts, which it calls, and used by dock-view.tsx.
import { openSession } from "./open-state.ts";
import { hasPreview } from "../../stores/agentpane/html-preview-state.ts";
import { type Dock } from "../../stores/agentpane/media-state.ts";
import { draw } from "./media-draw.ts";
import { paneModel, setOn } from "./media-layout.ts";
import { showTab } from "./media.ts";
import { focusTab, previewTab, setFolded } from "./pane-open.ts";
import { saveLayout } from "./pane-memory.ts";
import { P } from "../../stores/agentpane/pane-state.ts";
import { closeTab, noteTab } from "./pane-tabs.ts";
import { paneRoot } from "../../stores/agentpane/worktree.ts";
import { saveFile, saveWorkFile } from "../../stores/agentpane/read-file.ts";
import { togglePreview } from "./read.ts";
import { openMenu, type MenuItem } from "../../ui/menu.tsx";
import { writeClipboard } from "../../ui/clipboard.ts";

/// The menu of a tab the user opened: close it, save its file, copy its path. The
/// fixed tabs (Files, Changes, Subagents, Artifacts) have none.
export function tabMenu(e: MouseEvent, id: string) {
  const t = paneModel.getState().tabs.find(x => x.id === id);
  if (!t) return;
  e.preventDefault();
  const items: MenuItem[] = [];
  const note = (text: string) => noteTab(t, text);
  if (t.kind === "session" && t.splitSession) {
    const split = t.splitSession;
    items.push({ label: "Open in main pane", on: () => {
      closeTab(t.id);
      openSession({
        ...split, doing: null, started_at: 0, last_activity: 0, question: null, assistant: false, persona: null,
      });
    } });
  }
  const path = t.kind === "text" ? t.rel : t.path;
  if (t.kind === "text" && t.rel && hasPreview(t.rel) && !t.text?.binary) items.push({ label: t.preview?.active ? "Source" : "Preview", on: () => { focusTab(t.id); if (t.preview?.active) void togglePreview(t); else previewTab(t); } });
  if (path) items.push({ label: "Copy path", on: () => void writeClipboard(path) });

  if (t.kind === "text" && t.rel) {
    const rel = t.rel;
    items.push({ label: "Save", on: () => saveWorkFile(rel, note, t.root ?? paneRoot()) });
  } else if (t.path && P.session) {
    const { machine_id, id: session } = P.session, file = t.path;
    items.push({ label: "Save", on: () => saveFile("agent_file_save", { machineId: machine_id, path: file, session }, note) });
  }

  items.push({ label: "Close", on: () => closeTab(t.id) });
  openMenu(e, t.name, items);
}

/// A click on tab `id` in dock `d`: shows it, unless it ends a drag.
export function pickTab(d: Dock, id: string) {
  if (P.dragged) return;
  showTab(d, id);
}

/// This dock's tabs go to the other, which shows the one on screen here.
export function moveDock(d: Dock) {
  const to: Dock = d === "right" ? "bottom" : "right", on = paneModel.getState().on[d];
  // Moving the whole pane also places tabs that only appear in other sessions.
  paneModel.setState(s => ({ layout: { ...s.layout, dock: to, tabs: {} } }));
  P.moved.clear();
  setFolded(to, false);
  setOn(to, on);
  saveLayout();
  draw();
}

/// Folds dock `d` to its strip, or unfolds it.
export function foldDock(d: Dock, folded: boolean) {
  setFolded(d, folded);
  saveLayout();
  draw();
}
